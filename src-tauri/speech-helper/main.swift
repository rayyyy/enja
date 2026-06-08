import AVFoundation
import CoreMedia
import Foundation
import Speech

struct HelperResponse: Encodable {
    var ok: Bool
    var status: String?
    var supported: Bool?
    var authorization: String?
    var reason: String?
    var error: String?
    var details: [String]?
    var transcript: String?
}

struct ContextPayload: Decodable {
    var contextualStrings: [String]?
}

@main
struct EnjaSpeechHelper {
    static func main() async {
        let args = Array(CommandLine.arguments.dropFirst())
        guard let command = args.first else {
            emit(HelperResponse(ok: false, error: "Missing command."))
            return
        }

        guard #available(macOS 26.0, *) else {
            emit(HelperResponse(
                ok: true,
                status: "unsupported",
                supported: false,
                authorization: authorizationName(SFSpeechRecognizer.authorizationStatus()),
                reason: "macOS 26 or later is required."
            ))
            return
        }

        do {
            switch command {
            case "status":
                let localeID = args.count > 1 ? args[1] : "ja-JP"
                let requestAuthorization = args.contains("--request-authorization")
                let response = try await status(localeID: localeID, requestAuthorization: requestAuthorization)
                emit(response)
            case "install":
                let localeID = args.count > 1 ? args[1] : "ja-JP"
                let response = try await install(localeID: localeID)
                emit(response)
            case "transcribe":
                guard args.count >= 3 else {
                    emit(HelperResponse(ok: false, error: "Usage: transcribe <wav-path> <locale-id> [context-json-path]"))
                    return
                }
                let contextPath = args.count >= 4 ? args[3] : nil
                let response = try await transcribe(
                    audioPath: args[1],
                    localeID: args[2],
                    contextPath: contextPath
                )
                emit(response)
            case "stream-transcribe":
                guard args.count >= 4,
                      let sampleRate = Double(args[1]),
                      let channelsValue = UInt32(args[2]),
                      channelsValue > 0
                else {
                    emit(HelperResponse(ok: false, error: "Usage: stream-transcribe <sample-rate> <channels> <locale-id> [context-json-path]"))
                    return
                }
                let contextPath = args.count >= 5 ? args[4] : nil
                let response = try await streamTranscribe(
                    sampleRate: sampleRate,
                    channels: AVAudioChannelCount(channelsValue),
                    localeID: args[3],
                    contextPath: contextPath
                )
                emit(response)
            default:
                emit(HelperResponse(ok: false, error: "Unknown command: \(command)"))
            }
        } catch {
            emit(HelperResponse(ok: false, error: String(describing: error)))
        }
    }
}

func emit(_ response: HelperResponse) {
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

func authorizationName(_ status: SFSpeechRecognizerAuthorizationStatus) -> String {
    switch status {
    case .notDetermined:
        return "notDetermined"
    case .denied:
        return "denied"
    case .restricted:
        return "restricted"
    case .authorized:
        return "authorized"
    @unknown default:
        return "unknown"
    }
}

func requestSpeechAuthorization() async -> SFSpeechRecognizerAuthorizationStatus {
    await withCheckedContinuation { continuation in
        SFSpeechRecognizer.requestAuthorization { status in
            continuation.resume(returning: status)
        }
    }
}

@available(macOS 26.0, *)
func transcriber(localeID: String) async -> (DictationTranscriber?, Locale) {
    let requested = Locale(identifier: localeID)
    guard let supported = await DictationTranscriber.supportedLocale(equivalentTo: requested) else {
        return (nil, requested)
    }
    return (DictationTranscriber(locale: supported, preset: .longDictation), supported)
}

@available(macOS 26.0, *)
func status(localeID: String, requestAuthorization: Bool) async throws -> HelperResponse {
    let authorization = requestAuthorization
        ? await requestSpeechAuthorization()
        : SFSpeechRecognizer.authorizationStatus()
    if authorization == .denied || authorization == .restricted {
        return HelperResponse(
            ok: true,
            status: "unknown",
            supported: true,
            authorization: authorizationName(authorization),
            reason: "Speech recognition permission is not authorized."
        )
    }
    let (maybeTranscriber, locale) = await transcriber(localeID: localeID)
    guard let transcriber = maybeTranscriber else {
        return HelperResponse(
            ok: true,
            status: "unsupported",
            supported: false,
            authorization: authorizationName(authorization),
            reason: "Locale \(locale.identifier) is not supported by DictationTranscriber."
        )
    }
    let assetStatus = await AssetInventory.status(forModules: [transcriber])
    return HelperResponse(
        ok: true,
        status: assetStatusName(assetStatus),
        supported: assetStatus != .unsupported,
        authorization: authorizationName(authorization),
        details: ["locale: \(locale.identifier)"]
    )
}

@available(macOS 26.0, *)
func install(localeID: String) async throws -> HelperResponse {
    let authorization = await requestSpeechAuthorization()
    if authorization == .denied || authorization == .restricted {
        return HelperResponse(
            ok: true,
            status: "unknown",
            supported: true,
            authorization: authorizationName(authorization),
            reason: "Speech recognition permission is not authorized."
        )
    }
    let (maybeTranscriber, locale) = await transcriber(localeID: localeID)
    guard let transcriber = maybeTranscriber else {
        return HelperResponse(
            ok: true,
            status: "unsupported",
            supported: false,
            authorization: authorizationName(authorization),
            reason: "Locale \(locale.identifier) is not supported by DictationTranscriber."
        )
    }

    _ = try await AssetInventory.reserve(locale: locale)
    if let request = try await AssetInventory.assetInstallationRequest(supporting: [transcriber]) {
        try await request.downloadAndInstall()
    }
    let assetStatus = await AssetInventory.status(forModules: [transcriber])
    return HelperResponse(
        ok: true,
        status: assetStatusName(assetStatus),
        supported: assetStatus != .unsupported,
        authorization: authorizationName(authorization),
        details: ["locale: \(locale.identifier)"]
    )
}

@available(macOS 26.0, *)
func transcribe(audioPath: String, localeID: String, contextPath: String?) async throws -> HelperResponse {
    let authorization = SFSpeechRecognizer.authorizationStatus()
    guard authorization == .authorized else {
        return HelperResponse(
            ok: false,
            authorization: authorizationName(authorization),
            error: "Speech recognition permission is not authorized."
        )
    }

    let (maybeTranscriber, locale) = await transcriber(localeID: localeID)
    guard let transcriber = maybeTranscriber else {
        return HelperResponse(
            ok: false,
            status: "unsupported",
            supported: false,
            authorization: authorizationName(authorization),
            error: "Locale \(locale.identifier) is not supported by DictationTranscriber."
        )
    }
    let assetStatus = await AssetInventory.status(forModules: [transcriber])
    guard assetStatus == .installed else {
        return HelperResponse(
            ok: false,
            status: assetStatusName(assetStatus),
            supported: assetStatus != .unsupported,
            authorization: authorizationName(authorization),
            error: "Japanese dictation model is not installed."
        )
    }

    let context = AnalysisContext()
    let contextualStrings = loadContextualStrings(path: contextPath)
    if !contextualStrings.isEmpty {
        context.contextualStrings = [.general: contextualStrings]
    }

    let audioFile = try AVAudioFile(forReading: URL(fileURLWithPath: audioPath))
    async let collectedTranscript = collectFinalTranscript(from: transcriber)
    let analyzer = try await SpeechAnalyzer(
        inputAudioFile: audioFile,
        modules: [transcriber],
        analysisContext: context,
        finishAfterFile: true
    )
    let transcript = try await collectedTranscript
    _ = analyzer
    return HelperResponse(
        ok: true,
        status: "installed",
        supported: true,
        authorization: authorizationName(authorization),
        details: contextualStrings.isEmpty ? [] : ["contextualStrings: \(contextualStrings.count)"],
        transcript: transcript
    )
}

@available(macOS 26.0, *)
func streamTranscribe(sampleRate: Double, channels: AVAudioChannelCount, localeID: String, contextPath: String?) async throws -> HelperResponse {
    let authorization = SFSpeechRecognizer.authorizationStatus()
    guard authorization == .authorized else {
        return HelperResponse(
            ok: false,
            authorization: authorizationName(authorization),
            error: "Speech recognition permission is not authorized."
        )
    }

    guard sampleRate > 0, channels > 0 else {
        return HelperResponse(ok: false, error: "Invalid PCM audio format.")
    }

    let (maybeTranscriber, locale) = await transcriber(localeID: localeID)
    guard let transcriber = maybeTranscriber else {
        return HelperResponse(
            ok: false,
            status: "unsupported",
            supported: false,
            authorization: authorizationName(authorization),
            error: "Locale \(locale.identifier) is not supported by DictationTranscriber."
        )
    }
    let assetStatus = await AssetInventory.status(forModules: [transcriber])
    guard assetStatus == .installed else {
        return HelperResponse(
            ok: false,
            status: assetStatusName(assetStatus),
            supported: assetStatus != .unsupported,
            authorization: authorizationName(authorization),
            error: "Japanese dictation model is not installed."
        )
    }

    let context = AnalysisContext()
    let contextualStrings = loadContextualStrings(path: contextPath)
    if !contextualStrings.isEmpty {
        context.contextualStrings = [.general: contextualStrings]
    }

    guard let analyzerFormat = AVAudioFormat(
        commonFormat: .pcmFormatFloat32,
        sampleRate: sampleRate,
        channels: channels,
        interleaved: false
    ) else {
        return HelperResponse(ok: false, error: "Failed to create analyzer audio format.")
    }

    let analyzer = SpeechAnalyzer(modules: [transcriber])
    try await analyzer.setContext(context)
    let inputSequence = stdinAnalyzerInputSequence(format: analyzerFormat)

    async let collectedTranscript = collectFinalTranscript(from: transcriber)
    let lastSampleTime = try await analyzer.analyzeSequence(inputSequence)
    if let lastSampleTime {
        try await analyzer.finalizeAndFinish(through: lastSampleTime)
    } else {
        await analyzer.cancelAndFinishNow()
    }
    let transcript = try await collectedTranscript

    return HelperResponse(
        ok: true,
        status: "installed",
        supported: true,
        authorization: authorizationName(authorization),
        details: contextualStrings.isEmpty ? [] : ["contextualStrings: \(contextualStrings.count)"],
        transcript: transcript
    )
}

@available(macOS 26.0, *)
func stdinAnalyzerInputSequence(format: AVAudioFormat) -> AsyncStream<AnalyzerInput> {
    AsyncStream { continuation in
        Task.detached {
            let channels = Int(format.channelCount)
            let bytesPerFrame = max(channels, 1) * MemoryLayout<Int16>.size
            let readSize = max(4096, Int(format.sampleRate / 10.0) * bytesPerFrame)
            var pending = Data()

            while true {
                let chunk = FileHandle.standardInput.readData(ofLength: readSize)
                if chunk.isEmpty {
                    break
                }
                pending.append(chunk)
                let usableBytes = pending.count - (pending.count % bytesPerFrame)
                if usableBytes <= 0 {
                    continue
                }
                let inputData = Data(pending.prefix(usableBytes))
                pending.removeFirst(usableBytes)
                if let buffer = pcm16Buffer(data: inputData, format: format) {
                    continuation.yield(AnalyzerInput(buffer: buffer))
                }
            }

            continuation.finish()
        }
    }
}

func pcm16Buffer(data: Data, format: AVAudioFormat) -> AVAudioPCMBuffer? {
    let channels = Int(format.channelCount)
    let bytesPerFrame = max(channels, 1) * MemoryLayout<Int16>.size
    let frameCount = data.count / bytesPerFrame
    if frameCount <= 0 {
        return nil
    }
    guard let buffer = AVAudioPCMBuffer(
        pcmFormat: format,
        frameCapacity: AVAudioFrameCount(frameCount)
    ) else {
        return nil
    }
    buffer.frameLength = AVAudioFrameCount(frameCount)

    guard let channelData = buffer.floatChannelData else {
        return nil
    }

    data.withUnsafeBytes { rawBuffer in
        let samples = rawBuffer.bindMemory(to: Int16.self)
        for frame in 0..<frameCount {
            for channel in 0..<channels {
                let sampleIndex = frame * channels + channel
                channelData[channel][frame] = Float(samples[sampleIndex]) / Float(Int16.max)
            }
        }
    }

    return buffer
}

@available(macOS 26.0, *)
func collectFinalTranscript(from transcriber: DictationTranscriber) async throws -> String {
    var parts: [String] = []
    var latestPending: String?
    for try await result in transcriber.results {
        let text = String(result.text.characters).trimmingCharacters(in: .whitespacesAndNewlines)
        if text.isEmpty {
            continue
        }
        if result.isFinal {
            if parts.last != text {
                parts.append(text)
            }
            latestPending = nil
        } else {
            latestPending = text
        }
    }

    if let pending = latestPending {
        appendPendingTranscript(pending, to: &parts)
    }

    return parts.joined(separator: "\n")
}

func appendPendingTranscript(_ pending: String, to parts: inout [String]) {
    let finalText = parts.joined(separator: "\n").trimmingCharacters(in: .whitespacesAndNewlines)
    if finalText.isEmpty {
        parts.append(pending)
        return
    }
    if finalText.contains(pending) || pending == finalText {
        return
    }
    if pending.hasPrefix(finalText) {
        let suffix = String(pending.dropFirst(finalText.count)).trimmingCharacters(in: .whitespacesAndNewlines)
        if !suffix.isEmpty {
            parts.append(suffix)
        }
        return
    }

    let overlap = suffixPrefixOverlap(finalText, pending)
    if overlap > 0 {
        let suffix = String(pending.dropFirst(overlap)).trimmingCharacters(in: .whitespacesAndNewlines)
        if !suffix.isEmpty {
            parts.append(suffix)
        }
        return
    }

    parts.append(pending)
}

func suffixPrefixOverlap(_ left: String, _ right: String) -> Int {
    let leftCharacters = Array(left)
    let rightCharacters = Array(right)
    let maxLength = min(leftCharacters.count, rightCharacters.count)
    if maxLength == 0 {
        return 0
    }

    for length in stride(from: maxLength, through: 1, by: -1) {
        let leftSuffix = leftCharacters[(leftCharacters.count - length)..<leftCharacters.count]
        let rightPrefix = rightCharacters[0..<length]
        if Array(leftSuffix) == Array(rightPrefix) {
            return length
        }
    }
    return 0
}

func loadContextualStrings(path: String?) -> [String] {
    guard let path else {
        return []
    }
    do {
        let data = try Data(contentsOf: URL(fileURLWithPath: path))
        let payload = try JSONDecoder().decode(ContextPayload.self, from: data)
        var seen = Set<String>()
        var values: [String] = []
        for value in payload.contextualStrings ?? [] {
            let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
            if trimmed.isEmpty || trimmed.count > 40 {
                continue
            }
            if seen.insert(trimmed.lowercased()).inserted {
                values.append(trimmed)
            }
            if values.count >= 100 {
                break
            }
        }
        return values
    } catch {
        return []
    }
}

@available(macOS 26.0, *)
func assetStatusName(_ status: AssetInventory.Status) -> String {
    switch status {
    case .unsupported:
        return "unsupported"
    case .supported:
        return "supported"
    case .downloading:
        return "downloading"
    case .installed:
        return "installed"
    @unknown default:
        return "unknown"
    }
}
