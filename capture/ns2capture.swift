// ns2capture: AVFoundation capture for the bench.
//
// Shares capture cards with OBS: it only reconfigures a device when asked for
// a specific format AND nobody else holds the configuration lock; otherwise
// it uses whatever format the device is already running.
//
//   ns2capture list
//   ns2capture formats                                      (JSON: devices and their formats)
//   ns2capture snap   <video> <out.png|out.jpg>
//   ns2capture stream <video> <fps> <quality> [width height] ([u32 len][jpeg] to stdout)
//   ns2capture stream2 ...same args...  ([u8 kind][u32 len][data]: 1 = JPEG, 2 = H.264 Annex B access unit)
//   ns2capture record <video> <audioUniqueID|-> <seconds> <out.mov>
//   ns2capture ocr [fast]       (image bytes on stdin -> JSON lines with text, confidence, box)
//   ns2capture audio <audioUniqueID|name>   (48 kHz stereo s16le PCM to stdout, shared with OBS)
import AVFoundation
import CoreImage
import ImageIO
import UniformTypeIdentifiers
import Vision
import VideoToolbox

func fail(_ code: Int32, _ msg: String) -> Never {
    FileHandle.standardError.write("ns2capture: \(msg)\n".data(using: .utf8)!)
    exit(code)
}

func allDevices() -> [AVCaptureDevice] {
    let types: [AVCaptureDevice.DeviceType] = [.external, .builtInWideAngleCamera, .microphone]
    return AVCaptureDevice.DiscoverySession(deviceTypes: types, mediaType: nil, position: .unspecified).devices
}

/// Unique ID first, then exact name, then the first external video device for "".
func device(_ spec: String, _ kind: String) -> AVCaptureDevice {
    if let d = AVCaptureDevice(uniqueID: spec) { return d }
    let media: AVMediaType = kind == "audio" ? .audio : .video
    let candidates = allDevices().filter { $0.hasMediaType(media) }
    if let d = candidates.first(where: { $0.localizedName == spec }) { return d }
    if spec.isEmpty, let d = candidates.first(where: { $0.deviceType == .external }) { return d }
    fail(3, "\(kind) device '\(spec)' not found (see: ns2capture list)")
}

func addInput(_ s: AVCaptureSession, _ d: AVCaptureDevice) {
    let media: AVMediaType = d.hasMediaType(.video) ? .video : .audio
    var allowed = AVCaptureDevice.authorizationStatus(for: media) == .authorized
    if AVCaptureDevice.authorizationStatus(for: media) == .notDetermined {
        let wait = DispatchSemaphore(value: 0)
        AVCaptureDevice.requestAccess(for: media) { granted in allowed = granted; wait.signal() }
        wait.wait()
    }
    guard allowed else {
        let setting = media == .video ? "Camera" : "Microphone"
        fail(8, "\(setting) access denied. Enable NS2Bench in System Settings > Privacy & Security > \(setting), then retry capture.")
    }
    do {
        let i = try AVCaptureDeviceInput(device: d)
        guard s.canAddInput(i) else { fail(6, "cannot add \(d.localizedName)") }
        s.addInput(i)
    } catch { fail(6, "open \(d.localizedName): \(error.localizedDescription)") }
}

func list() {
    let types: [AVCaptureDevice.DeviceType] = [.external, .builtInWideAngleCamera, .microphone]
    let found = AVCaptureDevice.DiscoverySession(deviceTypes: types, mediaType: nil, position: .unspecified).devices
    for d in found {
        let media = d.hasMediaType(.video) ? "video" : "audio"
        let dims = d.hasMediaType(.video) ? CMVideoFormatDescriptionGetDimensions(d.activeFormat.formatDescription) : CMVideoDimensions(width: 0, height: 0)
        let size = media == "video" ? " \(dims.width)x\(dims.height)" : ""
        print("\(media)\t\(d.uniqueID)\t\(d.localizedName)\(size)\(d.isInUseByAnotherApplication ? "\tin-use" : "")")
    }
}

func fourcc(_ code: FourCharCode) -> String {
    let bytes = [24, 16, 8, 0].map { UInt8((code >> $0) & 0xFF) }
    return String(bytes: bytes, encoding: .ascii)?.trimmingCharacters(in: .whitespaces) ?? "\(code)"
}

/// Every video device with every distinct format, as JSON for the server's settings panel.
func formats() {
    var out: [[String: Any]] = []
    for d in allDevices() where d.hasMediaType(.video) {
        var seen = Set<String>()
        var list: [[String: Any]] = []
        for f in d.formats {
            let dims = CMVideoFormatDescriptionGetDimensions(f.formatDescription)
            let maxFps = f.videoSupportedFrameRateRanges.map { $0.maxFrameRate }.max() ?? 0
            let minFps = f.videoSupportedFrameRateRanges.map { $0.minFrameRate }.min() ?? 0
            let pix = fourcc(CMFormatDescriptionGetMediaSubType(f.formatDescription))
            let key = "\(dims.width)x\(dims.height)@\(Int(maxFps.rounded()))/\(pix)"
            if seen.insert(key).inserted {
                list.append(["width": dims.width, "height": dims.height, "min_fps": minFps,
                             "max_fps": maxFps, "pixel": pix])
            }
        }
        let a = CMVideoFormatDescriptionGetDimensions(d.activeFormat.formatDescription)
        let dur = d.activeVideoMinFrameDuration
        let activeFps = dur.isValid && dur.seconds > 0 ? 1.0 / dur.seconds : 0
        out.append(["id": d.uniqueID, "name": d.localizedName, "in_use": d.isInUseByAnotherApplication,
                    "active": ["width": a.width, "height": a.height, "fps": activeFps.rounded()],
                    "formats": list])
    }
    let data = try! JSONSerialization.data(withJSONObject: out, options: [.sortedKeys])
    FileHandle.standardOutput.write(data)
    print("")
}

/// Try to switch the device to width x height at fps. Never steals: if another app holds
/// the configuration (OBS), keep the current format and say so on stderr.
func configure(_ d: AVCaptureDevice, width: Int32, height: Int32, fps: Double) {
    let candidates = d.formats.filter {
        let dims = CMVideoFormatDescriptionGetDimensions($0.formatDescription)
        return dims.width == width && dims.height == height &&
            $0.videoSupportedFrameRateRanges.contains { $0.maxFrameRate + 0.5 >= fps && $0.minFrameRate - 0.5 <= fps }
    }
    guard let f = candidates.first(where: { fourcc(CMFormatDescriptionGetMediaSubType($0.formatDescription)) != "dmb1" }) ?? candidates.first else {
        FileHandle.standardError.write("ns2capture: \(width)x\(height)@\(fps) not offered; using current format\n".data(using: .utf8)!)
        return
    }
    do {
        try d.lockForConfiguration()
        d.activeFormat = f
        let range = f.videoSupportedFrameRateRanges.first { $0.maxFrameRate + 0.5 >= fps && $0.minFrameRate - 0.5 <= fps }!
        // pick the exact supported duration closest to the request (e.g. 60.000240 for "60")
        let target = abs(range.maxFrameRate - fps) < 0.5 ? range.minFrameDuration : CMTime(value: 1000, timescale: CMTimeScale(fps * 1000))
        d.activeVideoMinFrameDuration = target
        d.activeVideoMaxFrameDuration = target
        d.unlockForConfiguration()
        FileHandle.standardError.write("ns2capture: configured \(width)x\(height)@\(fps)\n".data(using: .utf8)!)
    } catch {
        FileHandle.standardError.write("ns2capture: device busy (another app owns its format); sharing current format\n".data(using: .utf8)!)
    }
}

final class Snapper: NSObject, AVCaptureVideoDataOutputSampleBufferDelegate {
    let out: URL
    var frames = 0
    init(_ out: URL) { self.out = out }
    func captureOutput(_ o: AVCaptureOutput, didOutput sb: CMSampleBuffer, from c: AVCaptureConnection) {
        frames += 1
        if frames < 5 { return }   // skip warm-up frames
        guard let px = CMSampleBufferGetImageBuffer(sb) else { return }
        let ci = CIImage(cvPixelBuffer: px)
        let jpeg = ["jpg", "jpeg"].contains(out.pathExtension.lowercased())
        let type = (jpeg ? UTType.jpeg : UTType.png).identifier as CFString
        guard let cg = CIContext().createCGImage(ci, from: ci.extent),
              let dst = CGImageDestinationCreateWithURL(out as CFURL, type, 1, nil) else { fail(4, "encode failed") }
        CGImageDestinationAddImage(dst, cg, jpeg ? [kCGImageDestinationLossyCompressionQuality: 0.92] as CFDictionary : nil)
        guard CGImageDestinationFinalize(dst) else { fail(5, "write failed") }
        print("\(out.path)\t\(cg.width)x\(cg.height)")
        exit(0)
    }
}

/// Writes [u32 big-endian length][JPEG] frames to stdout, throttled to fps.
/// JPEG is encoded in hardware (VideoToolbox); Core Image is the fallback.
final class Streamer: NSObject, AVCaptureVideoDataOutputSampleBufferDelegate {
    let interval: Double
    let quality: Double
    let ctx = CIContext()
    let space = CGColorSpace(name: CGColorSpace.sRGB)!
    var next = 0.0
    var vt: VTCompressionSession?
    var vtSize = (0, 0)
    var vtFailed = false
    let out = FileHandle.standardOutput
    let typed: Bool               // stream2: [u8 kind][u32 len][data], kind 1 = JPEG, 2 = H.264 (Annex B)
    let lock = NSLock()           // JPEG and H.264 callbacks run on different threads
    var h264: VTCompressionSession?
    var h264Size = (0, 0)
    var h264Failed = false
    init(fps: Double, quality: Double, typed: Bool = false) {
        interval = 1.0 / fps; self.quality = quality; self.typed = typed
    }

    func emit(_ data: Data, kind: UInt8 = 1) {
        var len = UInt32(data.count).bigEndian
        lock.lock(); defer { lock.unlock() }
        do {
            if typed { try out.write(contentsOf: Data([kind])) }
            try out.write(contentsOf: Data(bytes: &len, count: 4))
            try out.write(contentsOf: data)
        } catch { exit(0) }   // reader went away
    }

    /// Hardware H.264 for the live view: real time, no B-frames, keyframe every second.
    func h264Session(for px: CVPixelBuffer, fps: Double) -> VTCompressionSession? {
        let w = CVPixelBufferGetWidth(px), h = CVPixelBufferGetHeight(px)
        if let s = h264, h264Size == (w, h) { return s }
        if h264Failed { return nil }
        if let s = h264 { VTCompressionSessionInvalidate(s); h264 = nil }
        var s: VTCompressionSession?
        guard VTCompressionSessionCreate(allocator: nil, width: Int32(w), height: Int32(h), codecType: kCMVideoCodecType_H264,
                encoderSpecification: nil, imageBufferAttributes: nil, compressedDataAllocator: nil,
                outputCallback: nil, refcon: nil, compressionSessionOut: &s) == noErr, let made = s else {
            h264Failed = true; return nil
        }
        VTSessionSetProperty(made, key: kVTCompressionPropertyKey_RealTime, value: kCFBooleanTrue)
        VTSessionSetProperty(made, key: kVTCompressionPropertyKey_AllowFrameReordering, value: kCFBooleanFalse)
        VTSessionSetProperty(made, key: kVTCompressionPropertyKey_ProfileLevel, value: kVTProfileLevel_H264_High_AutoLevel)
        VTSessionSetProperty(made, key: kVTCompressionPropertyKey_ExpectedFrameRate, value: fps as CFNumber)
        VTSessionSetProperty(made, key: kVTCompressionPropertyKey_MaxKeyFrameInterval, value: Int(fps) as CFNumber)
        let bitrate = Double(w * h) * fps * 0.1            // ~12 Mbit/s at 1080p60
        VTSessionSetProperty(made, key: kVTCompressionPropertyKey_AverageBitRate, value: bitrate as CFNumber)
        h264 = made; h264Size = (w, h)
        return made
    }

    func encodeH264(_ px: CVPixelBuffer, pts: CMTime) {
        guard let s = h264Session(for: px, fps: 1.0 / interval) else { return }
        VTCompressionSessionEncodeFrame(s, imageBuffer: px, presentationTimeStamp: pts, duration: .invalid,
                                        frameProperties: nil, infoFlagsOut: nil) { [weak self] st, _, enc in
            guard st == noErr, let enc, let self, let au = Streamer.annexB(enc) else { return }
            self.emit(au, kind: 2)
        }
    }

    /// AVCC sample -> Annex B access unit; keyframes get SPS/PPS in front.
    static func annexB(_ sb: CMSampleBuffer) -> Data? {
        guard let block = CMSampleBufferGetDataBuffer(sb) else { return nil }
        var length = 0
        var ptr: UnsafeMutablePointer<CChar>?
        guard CMBlockBufferGetDataPointer(block, atOffset: 0, lengthAtOffsetOut: nil, totalLengthOut: &length,
                                          dataPointerOut: &ptr) == noErr, let ptr else { return nil }
        let start: [UInt8] = [0, 0, 0, 1]
        var out = Data()
        let attachments = CMSampleBufferGetSampleAttachmentsArray(sb, createIfNecessary: false) as? [[CFString: Any]]
        let keyframe = !(attachments?.first?[kCMSampleAttachmentKey_NotSync] as? Bool ?? false)
        if keyframe, let fmt = CMSampleBufferGetFormatDescription(sb) {
            var count = 0
            CMVideoFormatDescriptionGetH264ParameterSetAtIndex(fmt, parameterSetIndex: 0, parameterSetPointerOut: nil,
                parameterSetSizeOut: nil, parameterSetCountOut: &count, nalUnitHeaderLengthOut: nil)
            for i in 0..<count {
                var p: UnsafePointer<UInt8>?
                var n = 0
                if CMVideoFormatDescriptionGetH264ParameterSetAtIndex(fmt, parameterSetIndex: i, parameterSetPointerOut: &p,
                        parameterSetSizeOut: &n, parameterSetCountOut: nil, nalUnitHeaderLengthOut: nil) == noErr, let p {
                    out.append(contentsOf: start); out.append(p, count: n)
                }
            }
        }
        var off = 0
        while off + 4 <= length {
            var nal: UInt32 = 0
            memcpy(&nal, ptr + off, 4)
            let n = Int(UInt32(bigEndian: nal))
            off += 4
            if off + n > length { break }
            out.append(contentsOf: start)
            out.append(Data(bytes: ptr + off, count: n))
            off += n
        }
        return out
    }

    func session(for px: CVPixelBuffer) -> VTCompressionSession? {
        let w = CVPixelBufferGetWidth(px), h = CVPixelBufferGetHeight(px)
        if let s = vt, vtSize == (w, h) { return s }
        if vtFailed { return nil }
        if let s = vt { VTCompressionSessionInvalidate(s); vt = nil }
        var s: VTCompressionSession?
        let status = VTCompressionSessionCreate(allocator: nil, width: Int32(w), height: Int32(h),
            codecType: kCMVideoCodecType_JPEG, encoderSpecification: nil, imageBufferAttributes: nil,
            compressedDataAllocator: nil, outputCallback: nil, refcon: nil, compressionSessionOut: &s)
        guard status == noErr, let made = s else { vtFailed = true; return nil }
        VTSessionSetProperty(made, key: kVTCompressionPropertyKey_Quality, value: quality as CFNumber)
        VTSessionSetProperty(made, key: kVTCompressionPropertyKey_RealTime, value: kCFBooleanTrue)
        vt = made
        vtSize = (w, h)
        return made
    }

    func captureOutput(_ o: AVCaptureOutput, didOutput sb: CMSampleBuffer, from c: AVCaptureConnection) {
        // Keep a frame once 80% of the interval has passed: tolerant of capture jitter, so a
        // 60 fps source at a 60 fps target keeps every frame and at 30 keeps every other one.
        let now = CACurrentMediaTime()
        if now - next < interval * 0.8 { return }
        next = now
        guard let px = CMSampleBufferGetImageBuffer(sb) else { return }
        if typed { encodeH264(px, pts: CMSampleBufferGetPresentationTimeStamp(sb)) }
        if let s = session(for: px) {
            let pts = CMSampleBufferGetPresentationTimeStamp(sb)
            let status = VTCompressionSessionEncodeFrame(s, imageBuffer: px, presentationTimeStamp: pts,
                duration: .invalid, frameProperties: nil, infoFlagsOut: nil) { [weak self] st, _, enc in
                guard st == noErr, let enc, let block = CMSampleBufferGetDataBuffer(enc) else { return }
                var length = 0
                var ptr: UnsafeMutablePointer<CChar>?
                guard CMBlockBufferGetDataPointer(block, atOffset: 0, lengthAtOffsetOut: nil,
                                                  totalLengthOut: &length, dataPointerOut: &ptr) == noErr,
                      let ptr else { return }
                self?.emit(Data(bytes: ptr, count: length), kind: 1)
            }
            if status == noErr { return }
            vtFailed = true
        }
        let opts = [CIImageRepresentationOption(rawValue: kCGImageDestinationLossyCompressionQuality as String): quality]
        if let data = ctx.jpegRepresentation(of: CIImage(cvPixelBuffer: px), colorSpace: space, options: opts) {
            emit(data, kind: 1)
        }
    }
}

/// Raw PCM to stdout: 48 kHz, 2 channels, signed 16-bit little-endian, interleaved.
final class AudioPipe: NSObject, AVCaptureAudioDataOutputSampleBufferDelegate {
    let out = FileHandle.standardOutput
    func captureOutput(_ o: AVCaptureOutput, didOutput sb: CMSampleBuffer, from c: AVCaptureConnection) {
        guard let block = CMSampleBufferGetDataBuffer(sb) else { return }
        var length = 0
        var ptr: UnsafeMutablePointer<CChar>?
        guard CMBlockBufferGetDataPointer(block, atOffset: 0, lengthAtOffsetOut: nil, totalLengthOut: &length,
                                          dataPointerOut: &ptr) == noErr, let ptr else { return }
        do { try out.write(contentsOf: Data(bytes: ptr, count: length)) } catch { exit(0) }
    }
}

final class Recorder: NSObject, AVCaptureFileOutputRecordingDelegate {
    func fileOutput(_ o: AVCaptureFileOutput, didFinishRecordingTo url: URL, from c: [AVCaptureConnection], error: Error?) {
        if let e = error as NSError?, (e.userInfo[AVErrorRecordingSuccessfullyFinishedKey] as? Bool) != true {
            fail(8, "recording failed: \(e.localizedDescription)")
        }
        print("\(url.path)\t\(String(format: "%.1f", o.recordedDuration.seconds))s")
        exit(0)
    }
}

/// Text recognition with Apple Vision. Boxes are normalised, origin top-left: [x, y, w, h].
func ocr(fast: Bool) {
    let data = FileHandle.standardInput.readDataToEndOfFile()
    guard let src = CGImageSourceCreateWithData(data as CFData, nil),
          let img = CGImageSourceCreateImageAtIndex(src, 0, nil) else { fail(4, "cannot decode image") }
    let req = VNRecognizeTextRequest()
    req.recognitionLevel = fast ? .fast : .accurate
    req.usesLanguageCorrection = !fast
    do { try VNImageRequestHandler(cgImage: img).perform([req]) } catch { fail(5, "ocr failed: \(error)") }
    var out: [[String: Any]] = []
    for o in req.results ?? [] {
        guard let c = o.topCandidates(1).first else { continue }
        let b = o.boundingBox
        out.append(["text": c.string, "conf": Double(c.confidence),
                    "box": [b.minX, 1 - b.maxY, b.width, b.height]])
    }
    FileHandle.standardOutput.write(try! JSONSerialization.data(withJSONObject: ["width": img.width, "height": img.height, "lines": out]))
    print("")
}

let args = CommandLine.arguments
let session = AVCaptureSession()
var keep: AnyObject?

switch args.count > 1 ? args[1] : "" {
case "list":
    list()
    exit(0)
case "formats":
    formats()
    exit(0)
case "ocr":
    ocr(fast: args.count > 2 && args[2] == "fast")
    exit(0)
case "snap" where args.count == 4:
    addInput(session, device(args[2], "video"))
    let o = AVCaptureVideoDataOutput()
    o.videoSettings = [kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA]
    let s = Snapper(URL(fileURLWithPath: args[3]))
    keep = s
    o.setSampleBufferDelegate(s, queue: DispatchQueue(label: "snap"))
    session.addOutput(o)
    session.startRunning()
    DispatchQueue.main.asyncAfter(deadline: .now() + 10) { fail(7, "no frame within 10s (is the source powered and docked?)") }
case "stream" where args.count == 5 || args.count == 7, "stream2" where args.count == 5 || args.count == 7:
    guard let fps = Double(args[3]), fps > 0, fps <= 120,
          let q = Double(args[4]), q > 0, q <= 100 else { fail(2, "stream <video> <fps 1-120> <quality 1-100> [width height]") }
    signal(SIGPIPE, SIG_IGN)
    let dev = device(args[2], "video")
    addInput(session, dev)
    let o = AVCaptureVideoDataOutput()
    o.videoSettings = [kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange]
    o.alwaysDiscardsLateVideoFrames = true
    let s = Streamer(fps: fps, quality: q / 100, typed: args[1] == "stream2")
    keep = s
    o.setSampleBufferDelegate(s, queue: DispatchQueue(label: "stream"))
    session.addOutput(o)
    session.startRunning()
    // macOS resets the device format to the session preset when the input is added,
    // so apply the requested format only once the session runs.
    if args.count == 7, let w = Int32(args[5]), let h = Int32(args[6]) { configure(dev, width: w, height: h, fps: fps) }
case "audio" where args.count == 3:
    signal(SIGPIPE, SIG_IGN)
    addInput(session, device(args[2], "audio"))
    let o = AVCaptureAudioDataOutput()
    o.audioSettings = [AVFormatIDKey: kAudioFormatLinearPCM, AVSampleRateKey: 48000, AVNumberOfChannelsKey: 2,
                       AVLinearPCMBitDepthKey: 16, AVLinearPCMIsFloatKey: false,
                       AVLinearPCMIsNonInterleaved: false, AVLinearPCMIsBigEndianKey: false]
    let a = AudioPipe()
    keep = a
    o.setSampleBufferDelegate(a, queue: DispatchQueue(label: "audio"))
    guard session.canAddOutput(o) else { fail(6, "cannot add audio output") }
    session.addOutput(o)
    session.startRunning()
case "record" where args.count == 6:
    guard let secs = Double(args[4]), secs > 0, secs <= 600 else { fail(2, "seconds must be in (0, 600]") }
    addInput(session, device(args[2], "video"))
    if args[3] != "-" { addInput(session, device(args[3], "audio")) }
    let o = AVCaptureMovieFileOutput()
    o.maxRecordedDuration = CMTime(seconds: secs, preferredTimescale: 600)
    guard session.canAddOutput(o) else { fail(6, "cannot add movie output") }
    session.addOutput(o)
    let url = URL(fileURLWithPath: args[5])
    try? FileManager.default.removeItem(at: url)
    let r = Recorder()
    keep = r
    session.startRunning()
    o.startRecording(to: url, recordingDelegate: r)
    DispatchQueue.main.asyncAfter(deadline: .now() + secs + 15) { fail(7, "recording did not finish") }
default:
    fail(2, "usage: ns2capture list | formats | stream <video> <fps> <quality> [w h] | snap <videoID> <out.png|jpg> | record <videoID> <audioID|-> <seconds> <out.mov>")
}
_ = keep
dispatchMain()
