import AppKit
import CoreGraphics
import Foundation
import Vision

let maxOCRWindows = 3
let minWindowDimension: CGFloat = 80
let minWindowArea: CGFloat = 12_000
let minOCRConfidence: Float = 0.45
let maxOCRLinesPerWindow = 70
let maxOCRCharactersPerWindow = 1_800
// Enja 自身のウィンドウも OCR 対象に含める(固定したメモを文脈として
// 読みたいケースがあるため)。ただし音声オーバーレイは録音 UI しか
// 映っていないため除外する。
let ignoredOwnWindowTitles: Set<String> = ["Enja Voice"]

struct ScreenContextResponse: Encodable {
    var ok: Bool
    var appName: String?
    var windowTitle: String?
    var text: String?
    var error: String?
    var details: [String]?
}

@main
struct EnjaScreenContextHelper {
    static func main() {
        let args = Array(CommandLine.arguments.dropFirst())
        guard args.first == "ocr-screen" || args.first == "ocr-front-window" else {
            emit(ScreenContextResponse(ok: false, error: "Usage: ocr-screen"))
            return
        }

        do {
            emit(try ocrActiveDisplay())
        } catch {
            emit(ScreenContextResponse(ok: false, error: String(describing: error)))
        }
    }
}

func emit(_ response: ScreenContextResponse) {
    let encoder = JSONEncoder()
    encoder.outputFormatting = [.withoutEscapingSlashes]
    do {
        let data = try encoder.encode(response)
        if let text = String(data: data, encoding: .utf8) {
            print(text)
        }
    } catch {
        print("{\"ok\":false,\"error\":\"Failed to encode helper response.\"}")
    }
}

enum ScreenContextError: Error, CustomStringConvertible {
    case noFrontApplication
    case noWindow
    case noDisplay
    case captureFailed
    case emptyText

    var description: String {
        switch self {
        case .noFrontApplication:
            return "No frontmost application was found."
        case .noWindow:
            return "No frontmost window was found."
        case .noDisplay:
            return "No active display was found."
        case .captureFailed:
            return "Screen capture failed. Screen Recording permission may be required."
        case .emptyText:
            return "Vision OCR returned no text."
        }
    }
}

func ocrActiveDisplay() throws -> ScreenContextResponse {
    guard let app = NSWorkspace.shared.frontmostApplication else {
        throw ScreenContextError.noFrontApplication
    }

    let allWindows = try onScreenWindowInfos()
    let referenceWindow = try referenceWindowInfo(for: app.processIdentifier, in: allWindows)
    let display = try displayInfo(containing: referenceWindow.bounds)
    let targetWindows = windowInfos(on: display, from: allWindows).prefix(maxOCRWindows)
    if targetWindows.isEmpty {
        throw ScreenContextError.noWindow
    }

    var sections: [String] = []
    var details = [
        "displayId: \(display.id)",
        "displayBounds: \(Int(display.bounds.width))x\(Int(display.bounds.height))",
        "referenceWindowId: \(referenceWindow.id)",
        "maxOCRWindows: \(maxOCRWindows)",
        "minOCRConfidence: \(minOCRConfidence)",
    ]

    for (index, window) in targetWindows.enumerated() {
        guard let image = captureWindowImage(window, in: display) else {
            details.append("window\(index + 1): \(window.debugLabel), captureFailed")
            continue
        }

        let result = try recognizeText(in: image)
        details.append(
            "window\(index + 1): \(window.debugLabel), lines=\(result.lines.count), observations=\(result.observationCount), lowConfidence=\(result.lowConfidenceCount), rejected=\(result.rejectedCount), duplicates=\(result.duplicateCount)"
        )
        guard !result.lines.isEmpty else {
            continue
        }
        sections.append(formatOCRWindowSection(window: window, index: index, lines: result.lines))
    }

    let text = sections.joined(separator: "\n\n")
    if text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
        throw ScreenContextError.emptyText
    }

    let responseAppName = referenceWindow.ownerPid == app.processIdentifier
        ? (app.localizedName ?? referenceWindow.ownerName)
        : referenceWindow.ownerName

    return ScreenContextResponse(
        ok: true,
        appName: responseAppName,
        windowTitle: referenceWindow.title,
        text: text,
        details: details
    )
}

struct WindowInfo {
    var id: CGWindowID
    var ownerPid: pid_t
    var ownerName: String?
    var title: String?
    var bounds: CGRect

    var debugLabel: String {
        let owner = ownerName?.trimmingCharacters(in: .whitespacesAndNewlines)
        let title = title?.trimmingCharacters(in: .whitespacesAndNewlines)
        return "id=\(id), owner=\(owner?.isEmpty == false ? owner! : "(unknown)"), title=\(title?.isEmpty == false ? title! : "(untitled)"), bounds=\(Int(bounds.origin.x)),\(Int(bounds.origin.y)),\(Int(bounds.width))x\(Int(bounds.height))"
    }
}

struct DisplayInfo {
    var id: CGDirectDisplayID
    var bounds: CGRect
}

struct OCRLine {
    var text: String
    var confidence: Float
}

struct OCRResult {
    var lines: [OCRLine]
    var observationCount: Int
    var lowConfidenceCount: Int
    var rejectedCount: Int
    var duplicateCount: Int
}

func onScreenWindowInfos() throws -> [WindowInfo] {
    let options: CGWindowListOption = [.optionOnScreenOnly, .excludeDesktopElements]
    guard let rawList = CGWindowListCopyWindowInfo(options, kCGNullWindowID) as? [[String: Any]] else {
        throw ScreenContextError.noWindow
    }

    var windows: [WindowInfo] = []
    for item in rawList {
        guard
            let ownerPid = item[kCGWindowOwnerPID as String] as? pid_t,
            let layer = item[kCGWindowLayer as String] as? Int,
            layer == 0,
            let windowNumber = item[kCGWindowNumber as String] as? UInt32,
            let boundsValue = item[kCGWindowBounds as String] as? [String: Any],
            let bounds = CGRect(dictionaryRepresentation: boundsValue as CFDictionary),
            bounds.width >= minWindowDimension,
            bounds.height >= minWindowDimension,
            bounds.width * bounds.height >= minWindowArea
        else {
            continue
        }

        if let isOnScreen = item[kCGWindowIsOnscreen as String] as? Bool, !isOnScreen {
            continue
        }
        let alpha = (item[kCGWindowAlpha as String] as? NSNumber)?.doubleValue ?? 1.0
        if alpha <= 0.05 {
            continue
        }

        let ownerName = item[kCGWindowOwnerName as String] as? String
        let windowTitle = item[kCGWindowName as String] as? String
        if shouldIgnoreWindow(ownerName: ownerName, title: windowTitle) {
            continue
        }

        windows.append(WindowInfo(
            id: CGWindowID(windowNumber),
            ownerPid: ownerPid,
            ownerName: ownerName,
            title: windowTitle,
            bounds: bounds
        ))
    }

    if windows.isEmpty {
        throw ScreenContextError.noWindow
    }
    return windows
}

func referenceWindowInfo(for pid: pid_t, in windows: [WindowInfo]) throws -> WindowInfo {
    if let window = windows.first(where: { $0.ownerPid == pid }) {
        return window
    }
    if let window = windows.first {
        return window
    }
    throw ScreenContextError.noWindow
}

func windowInfos(on display: DisplayInfo, from windows: [WindowInfo]) -> [WindowInfo] {
    windows.filter { window in
        let intersection = display.bounds.intersection(window.bounds)
        return !intersection.isNull
            && intersection.width >= minWindowDimension
            && intersection.height >= minWindowDimension
            && intersection.width * intersection.height >= minWindowArea
    }
}

func shouldIgnoreWindow(ownerName: String?, title: String?) -> Bool {
    let owner = ownerName?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
    guard owner.lowercased() == "enja" else {
        return false
    }
    let title = title?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
    return ignoredOwnWindowTitles.contains(title)
}

func displayInfo(containing windowBounds: CGRect) throws -> DisplayInfo {
    var count: UInt32 = 0
    CGGetActiveDisplayList(0, nil, &count)
    guard count > 0 else {
        throw ScreenContextError.noDisplay
    }

    var displays = Array(repeating: CGDirectDisplayID(), count: Int(count))
    CGGetActiveDisplayList(count, &displays, &count)

    let center = CGPoint(x: windowBounds.midX, y: windowBounds.midY)
    var fallback: DisplayInfo?
    var fallbackIntersectionArea = CGFloat.zero

    for display in displays.prefix(Int(count)) {
        let bounds = CGDisplayBounds(display)
        if bounds.contains(center) {
            return DisplayInfo(id: display, bounds: bounds)
        }

        let intersection = bounds.intersection(windowBounds)
        if !intersection.isNull {
            let area = intersection.width * intersection.height
            if area > fallbackIntersectionArea {
                fallbackIntersectionArea = area
                fallback = DisplayInfo(id: display, bounds: bounds)
            }
        }
    }

    if let fallback {
        return fallback
    }
    throw ScreenContextError.noDisplay
}

func captureWindowImage(_ window: WindowInfo, in display: DisplayInfo) -> CGImage? {
    let captureBounds = display.bounds.intersection(window.bounds)
    guard !captureBounds.isNull, captureBounds.width > 0, captureBounds.height > 0 else {
        return nil
    }
    return CGWindowListCreateImage(
        captureBounds,
        [.optionIncludingWindow],
        window.id,
        [.bestResolution, .boundsIgnoreFraming]
    )
}

func recognizeText(in image: CGImage) throws -> OCRResult {
    let request = VNRecognizeTextRequest()
    request.recognitionLevel = .accurate
    request.usesLanguageCorrection = false
    request.recognitionLanguages = ["ja-JP", "en-US"]

    let handler = VNImageRequestHandler(cgImage: image, options: [:])
    try handler.perform([request])

    var seen = Set<String>()
    var lines: [OCRLine] = []
    var totalCharacters = 0
    var lowConfidenceCount = 0
    var rejectedCount = 0
    var duplicateCount = 0
    let observations = request.results ?? []
    for observation in observations {
        guard let candidate = observation.topCandidates(1).first else {
            continue
        }
        if candidate.confidence < minOCRConfidence {
            lowConfidenceCount += 1
            continue
        }
        let line = normalizeLine(candidate.string)
        guard isUsefulOCRLine(line) else {
            rejectedCount += 1
            continue
        }
        let key = line.lowercased()
        guard seen.insert(key).inserted else {
            duplicateCount += 1
            continue
        }
        lines.append(OCRLine(text: line, confidence: candidate.confidence))
        totalCharacters += line.count + 1
        if lines.count >= maxOCRLinesPerWindow || totalCharacters >= maxOCRCharactersPerWindow {
            break
        }
    }
    return OCRResult(
        lines: lines,
        observationCount: observations.count,
        lowConfidenceCount: lowConfidenceCount,
        rejectedCount: rejectedCount,
        duplicateCount: duplicateCount
    )
}

func normalizeLine(_ value: String) -> String {
    value
        .split(whereSeparator: { $0.isWhitespace })
        .joined(separator: " ")
        .trimmingCharacters(in: .whitespacesAndNewlines)
}

func isUsefulOCRLine(_ value: String) -> Bool {
    guard value.count >= 2 else {
        return false
    }

    var meaningful = 0
    var visible = 0
    for scalar in value.unicodeScalars {
        if CharacterSet.whitespacesAndNewlines.contains(scalar) {
            continue
        }
        visible += 1
        if CharacterSet.alphanumerics.contains(scalar) || isJapaneseScalar(scalar) {
            meaningful += 1
        }
    }

    guard meaningful >= 2, visible > 0 else {
        return false
    }
    if visible >= 4 && Double(meaningful) / Double(visible) < 0.35 {
        return false
    }
    return true
}

func isJapaneseScalar(_ scalar: UnicodeScalar) -> Bool {
    switch scalar.value {
    case 0x3040...0x30FF, 0x3400...0x9FFF:
        return true
    default:
        return false
    }
}

func formatOCRWindowSection(window: WindowInfo, index: Int, lines: [OCRLine]) -> String {
    let label: String
    switch index {
    case 0:
        label = "最優先ウィンドウ"
    case 1:
        label = "補助ウィンドウ1"
    default:
        label = "補助ウィンドウ2"
    }

    var headerParts = [label]
    if let ownerName = window.ownerName?.trimmingCharacters(in: .whitespacesAndNewlines), !ownerName.isEmpty {
        headerParts.append("アプリ=\(ownerName)")
    }
    if let title = window.title?.trimmingCharacters(in: .whitespacesAndNewlines), !title.isEmpty {
        headerParts.append("ウィンドウ=\(title)")
    }

    let body = lines.map(\.text).joined(separator: "\n")
    return "\(headerParts.joined(separator: " / ")):\n\(body)"
}
