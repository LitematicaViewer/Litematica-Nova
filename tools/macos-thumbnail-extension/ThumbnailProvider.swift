import AppKit
import Foundation
import QuickLookThumbnailing
import Darwin

/// Finder thumbnails reuse the existing Core embedded-preview decoder. This
/// extension is not a viewport, does not render a scene, and never requests a
/// Standard/Full render task or falls back to an invented placeholder image.
@objc(NovaThumbnailProvider)
final class ThumbnailProvider: QLThumbnailProvider {
    private static let work: OperationQueue = {
        let queue = OperationQueue()
        queue.name = "com.litematica.nova.thumbnail"
        queue.qualityOfService = .utility
        queue.maxConcurrentOperationCount = 1
        return queue
    }()

    override func provideThumbnail(for request: QLFileThumbnailRequest,
                                   _ handler: @escaping (QLThumbnailReply?, Error?) -> Void) {
        Self.work.addOperation {
            do {
                let image = try Self.embeddedPreview(request.fileURL)
                let maximum = request.maximumSize
                guard maximum.width > 0, maximum.height > 0,
                      image.size.width > 0, image.size.height > 0 else {
                    throw Self.failure(4, "The projection thumbnail has invalid dimensions.")
                }
                let factor = min(min(maximum.width / image.size.width,
                                     maximum.height / image.size.height), 1.0)
                let size = NSSize(width: max(1, image.size.width * factor),
                                  height: max(1, image.size.height * factor))
                let reply = QLThumbnailReply(contextSize: size, currentContextDrawing: {
                    image.draw(in: NSRect(origin: .zero, size: size),
                               from: .zero, operation: .copy, fraction: 1.0)
                    return true
                })
                handler(reply, nil)
            } catch {
                handler(nil, error)
            }
        }
    }

    private static func embeddedPreview(_ input: URL) throws -> NSImage {
        guard input.isFileURL, input.pathExtension.lowercased() == "litematic" else {
            throw failure(1, "The requested file is not a Litematica projection.")
        }
        // .app/Contents/PlugIns/NovaThumbnail.appex -> .app/Contents
        let contents = Bundle.main.bundleURL.deletingLastPathComponent().deletingLastPathComponent()
        let core = contents.appendingPathComponent("Resources/bin/viewer-backend/litematica_core")
        guard FileManager.default.isExecutableFile(atPath: core.path) else {
            throw failure(2, "The application Core preview helper is missing.")
        }
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("nova-thumbnail-" + UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: directory) }
        let output = directory.appendingPathComponent("preview.png")
        let access = input.startAccessingSecurityScopedResource()
        defer { if access { input.stopAccessingSecurityScopedResource() } }

        let process = Process()
        process.executableURL = core
        process.arguments = ["projection-preview", "--input", input.path, "--output", output.path]
        process.currentDirectoryURL = directory
        // No stdout/stderr pipes that could fill and stall a shell extension;
        // no input paths or projection payload are written to extension logs.
        process.standardInput = FileHandle.nullDevice
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        let ended = DispatchSemaphore(value: 0)
        process.terminationHandler = { _ in ended.signal() }
        try process.run()
        if ended.wait(timeout: .now() + 15.0) == .timedOut {
            process.terminate()
            if ended.wait(timeout: .now() + 1.0) == .timedOut {
                // Only this extension's own bounded Core child is affected.
                Darwin.kill(process.processIdentifier, SIGKILL)
                process.waitUntilExit()
            }
            throw failure(3, "The projection preview decoder timed out.")
        }
        guard process.terminationStatus == 0,
              let attributes = try? FileManager.default.attributesOfItem(atPath: output.path),
              let length = attributes[.size] as? NSNumber,
              length.uint64Value > 0, length.uint64Value <= 32 * 1024 * 1024,
              let image = NSImage(data: try Data(contentsOf: output)) else {
            // A valid projection without PreviewImageData has no thumbnail.
            throw failure(5, "This projection does not contain an embedded preview.")
        }
        // NSImage retains in-memory image data; the temporary file can be
        // removed before QuickLook calls the drawing closure.
        return image
    }

    private static func failure(_ code: Int, _ description: String) -> NSError {
        NSError(domain: "com.litematica.nova.thumbnail", code: code,
                userInfo: [NSLocalizedDescriptionKey: description])
    }
}
