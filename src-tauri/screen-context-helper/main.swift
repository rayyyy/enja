import AppKit
import CoreGraphics
import Foundation
import Vision

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
    let window = try frontWindowInfo(for: app.processIdentifier)
    let display = try displayInfo(containing: window.bounds)
    guard let image = CGWindowListCreateImage(
        display.bounds,
        [.optionOnScreenOnly, .excludeDesktopElements],
        kCGNullWindowID,
        [.bestResolution]
    ) else {
        throw ScreenContextError.captureFailed
    }

    let text = try recognizeText(in: image)
    if text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
        throw ScreenContextError.emptyText
    }

    return ScreenContextResponse(
        ok: true,
        appName: app.localizedName ?? window.ownerName,
        windowTitle: window.title,
        text: text,
        details: [
            "windowId: \(window.id)",
            "displayId: \(display.id)",
            "displayBounds: \(Int(display.bounds.width))x\(Int(display.bounds.height))",
        ]
    )
}

struct WindowInfo {
    var id: CGWindowID
    var ownerName: String?
    var title: String?
    var bounds: CGRect
}

struct DisplayInfo {
    var id: CGDirectDisplayID
    var bounds: CGRect
}

func frontWindowInfo(for pid: pid_t) throws -> WindowInfo {
    let options: CGWindowListOption = [.optionOnScreenOnly, .excludeDesktopElements]
    guard let rawList = CGWindowListCopyWindowInfo(options, kCGNullWindowID) as? [[String: Any]] else {
        throw ScreenContextError.noWindow
    }

    for item in rawList {
        guard
            let ownerPid = item[kCGWindowOwnerPID as String] as? pid_t,
            ownerPid == pid,
            let layer = item[kCGWindowLayer as String] as? Int,
            layer == 0,
            let windowNumber = item[kCGWindowNumber as String] as? UInt32,
            let boundsValue = item[kCGWindowBounds as String] as? [String: Any],
            let bounds = CGRect(dictionaryRepresentation: boundsValue as CFDictionary),
            bounds.width >= 40,
            bounds.height >= 40
        else {
            continue
        }

        return WindowInfo(
            id: CGWindowID(windowNumber),
            ownerName: item[kCGWindowOwnerName as String] as? String,
            title: item[kCGWindowName as String] as? String,
            bounds: bounds
        )
    }

    throw ScreenContextError.noWindow
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

func recognizeText(in image: CGImage) throws -> String {
    let request = VNRecognizeTextRequest()
    request.recognitionLevel = .accurate
    request.usesLanguageCorrection = true
    request.recognitionLanguages = ["ja-JP", "en-US"]

    let handler = VNImageRequestHandler(cgImage: image, options: [:])
    try handler.perform([request])

    var seen = Set<String>()
    var lines: [String] = []
    var totalCharacters = 0
    for observation in request.results ?? [] {
        guard let value = observation.topCandidates(1).first?.string else {
            continue
        }
        let line = normalizeLine(value)
        guard line.count >= 2 else {
            continue
        }
        let key = line.lowercased()
        guard seen.insert(key).inserted else {
            continue
        }
        lines.append(line)
        totalCharacters += line.count + 1
        if lines.count >= 240 || totalCharacters >= 12_000 {
            break
        }
    }
    return lines.joined(separator: "\n")
}

func normalizeLine(_ value: String) -> String {
    value
        .split(whereSeparator: { $0.isWhitespace })
        .joined(separator: " ")
        .trimmingCharacters(in: .whitespacesAndNewlines)
}
