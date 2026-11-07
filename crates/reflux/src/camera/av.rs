//! macOS camera capture with AVFoundation: an `AVCaptureSession` (preset closest to the
//! configured size) into an `AVCaptureVideoDataOutput` delivering BGRA to our delegate on a
//! serial dispatch queue. The first capture triggers the OS camera permission prompt; a bundled
//! build needs `NSCameraUsageDescription` in its Info.plist.

use std::sync::Mutex;
use std::time::Instant;

use dispatch2::{DispatchQueue, DispatchRetained};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{AllocAnyThread, DefinedClass, define_class, msg_send};
use objc2_av_foundation::*;
use objc2_core_media::CMSampleBuffer;
use objc2_core_video::*;
use objc2_foundation_03::{NSDictionary, NSNumber, NSString};

use super::pick::{Pacer, bgra_to_rgba};

pub fn available() -> bool {
    true
}

fn video() -> Option<&'static AVMediaType> {
    unsafe { AVMediaTypeVideo }
}

#[allow(deprecated)] // AVCaptureDeviceDiscoverySession needs a device type list per macOS version
fn devices() -> Vec<Retained<AVCaptureDevice>> {
    video().map_or(Vec::new(), |m| unsafe { AVCaptureDevice::devicesWithMediaType(m) }.to_vec())
}

pub fn cameras() -> Vec<String> {
    let mut names: Vec<String> = devices().iter().map(|d| unsafe { d.localizedName() }.to_string()).collect();
    names.dedup();
    names
}

type Sink = Box<dyn FnMut(u32, u32, Vec<u8>) + Send>;

struct Ivars {
    sink: Mutex<(Sink, Pacer)>,
    start: Instant,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "RefluxCameraDelegate"]
    #[ivars = Ivars]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl AVCaptureVideoDataOutputSampleBufferDelegate for Delegate {
        #[unsafe(method(captureOutput:didOutputSampleBuffer:fromConnection:))]
        fn did_output(&self, _output: &AVCaptureOutput, buffer: &CMSampleBuffer, _connection: &AVCaptureConnection) {
            self.frame(buffer);
        }
    }
);

impl Delegate {
    fn new(sink: Sink, fps: u32) -> Retained<Self> {
        let this = Self::alloc().set_ivars(Ivars { sink: Mutex::new((sink, Pacer::new(fps))), start: Instant::now() });
        unsafe { msg_send![super(this), init] }
    }

    fn frame(&self, buffer: &CMSampleBuffer) {
        let Ok(mut guard) = self.ivars().sink.lock() else { return };
        let (sink, pacer) = &mut *guard;
        if !pacer.due(self.ivars().start.elapsed()) {
            return;
        }
        let Some(image) = (unsafe { buffer.image_buffer() }) else { return };
        if unsafe { CVPixelBufferLockBaseAddress(&image, CVPixelBufferLockFlags::ReadOnly) } != kCVReturnSuccess {
            return;
        }
        let (w, h, stride) = (CVPixelBufferGetWidth(&image), CVPixelBufferGetHeight(&image), CVPixelBufferGetBytesPerRow(&image));
        let base = CVPixelBufferGetBaseAddress(&image) as *const u8;
        let rgba = (!base.is_null()).then(|| bgra_to_rgba(unsafe { std::slice::from_raw_parts(base, stride * h) }, w, h, stride)).flatten();
        unsafe { CVPixelBufferUnlockBaseAddress(&image, CVPixelBufferLockFlags::ReadOnly) };
        if let Some(rgba) = rgba {
            sink(w as u32, h as u32, rgba);
        }
    }
}

/// A running camera: BGRA frames at the session preset closest to the size, at most `fps`.
pub struct CameraCapture {
    session: Retained<AVCaptureSession>,
    _delegate: Retained<Delegate>,
    _queue: DispatchRetained<DispatchQueue>,
}

// AVFoundation sessions, delegates and dispatch queues are thread-safe; the handles are moved
// to the UI thread while frames flow on the capture queue.
unsafe impl Send for CameraCapture {}

impl CameraCapture {
    /// `device`: a name from [`cameras`] (`None` = the default camera).
    pub fn start(device: Option<&str>, _width: u32, height: u32, fps: u32, sink: impl FnMut(u32, u32, Vec<u8>) + Send + 'static) -> Option<Self> {
        let media = video()?;
        let status = unsafe { AVCaptureDevice::authorizationStatusForMediaType(media) };
        if status == AVAuthorizationStatus::Denied || status == AVAuthorizationStatus::Restricted {
            eprintln!("reflux: camera: access denied in System Settings › Privacy & Security › Camera");
            return None;
        }
        let dev = device.and_then(|n| devices().into_iter().find(|d| unsafe { d.localizedName() }.to_string() == n)).or_else(|| unsafe { AVCaptureDevice::defaultDeviceWithMediaType(media) })?;
        // Prompts for camera access the first time (frames are black until it's granted).
        let input = unsafe { AVCaptureDeviceInput::deviceInputWithDevice_error(&dev) }.map_err(|e| eprintln!("reflux: camera: {}", e.localizedDescription())).ok()?;
        unsafe {
            let session = AVCaptureSession::new();
            session.beginConfiguration();
            let preset = match height {
                ..=480 => AVCaptureSessionPreset640x480,
                ..=540 => AVCaptureSessionPreset960x540,
                ..=720 => AVCaptureSessionPreset1280x720,
                _ => AVCaptureSessionPreset1920x1080,
            };
            if session.canSetSessionPreset(preset) {
                session.setSessionPreset(preset);
            }
            if !session.canAddInput(&input) {
                return None;
            }
            session.addInput(&input);
            let output = AVCaptureVideoDataOutput::new();
            // CFString is toll-free bridged to NSString.
            let key = &*(kCVPixelBufferPixelFormatTypeKey as *const _ as *const NSString);
            let format = NSNumber::new_u32(kCVPixelFormatType_32BGRA);
            let settings = NSDictionary::<NSString, AnyObject>::from_slices(&[key], &[&*format as &AnyObject]);
            output.setVideoSettings(Some(&settings));
            output.setAlwaysDiscardsLateVideoFrames(true);
            let delegate = Delegate::new(Box::new(sink), fps);
            let queue = DispatchQueue::new("app.reflux.camera", None);
            output.setSampleBufferDelegate_queue(Some(ProtocolObject::from_ref(&*delegate)), Some(&queue));
            if !session.canAddOutput(&output) {
                return None;
            }
            session.addOutput(&output);
            session.commitConfiguration();
            session.startRunning();
            Some(Self { session, _delegate: delegate, _queue: queue })
        }
    }
}

impl Drop for CameraCapture {
    fn drop(&mut self) {
        // Synchronous: no delegate call runs after this.
        unsafe { self.session.stopRunning() };
    }
}
