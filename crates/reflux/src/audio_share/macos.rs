//! ScreenCaptureKit system audio: an `SCStream` over the main display with `capturesAudio`
//! and a tiny 2×2/1 fps video stream (audio is the only output we read). The share's own call
//! audio is excluded through `excludesCurrentProcessAudio` and our application is excluded from
//! the filter, so the remote voices do not feed back.
//!
//! Needs macOS 13+ and the Screen Recording permission. Without permission `startCapture`
//! fails; the share continues video-only. Everything runs on a capture thread (SCK delivers on
//! its own dispatch queue), so a permission prompt cannot block the UI.

use std::ptr::NonNull;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchRetained, DispatchSemaphore, DispatchTime};
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{AllocAnyThread, DefinedClass, define_class, msg_send, sel};
use objc2_core_audio_types::{AudioBuffer, AudioBufferList, AudioStreamBasicDescription};
use objc2_core_foundation::CFRetained;
use objc2_core_media::{CMAudioFormatDescriptionGetStreamBasicDescription, CMBlockBuffer, CMSampleBuffer, CMTime, CMTimeFlags};
use objc2_foundation_03::{NSArray, NSError};
use objc2_screen_capture_kit::{
    SCContentFilter, SCRunningApplication, SCShareableContent, SCStream, SCStreamConfiguration, SCStreamOutput, SCStreamOutputType, SCWindow,
};

use super::{RATE, Resampler};

const TIMEOUT: Duration = Duration::from_secs(5);
/// `kCMSampleBufferFlag_AudioBufferList_Assure16ByteAlignment`.
const ALIGN_BUFFERS: u32 = 1;
const LINEAR_PCM: u32 = u32::from_le_bytes(*b"lpcm");
const FLAG_IS_FLOAT: u32 = 1 << 0;
const FLAG_IS_NON_INTERLEAVED: u32 = 1 << 5;

pub struct SystemAudio {
    stop: Option<Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl SystemAudio {
    pub fn start(sink: impl FnMut(&[f32]) + Send + 'static) -> Option<Self> {
        let (ready, started) = channel();
        let (stop_tx, stopped) = channel();
        let thread = std::thread::Builder::new()
            .name("reflux-system-audio".into())
            .spawn(move || {
                let Some(running) = build(sink) else {
                    let _ = ready.send(false);
                    return;
                };
                let _ = ready.send(true);
                let _ = stopped.recv();
                stop(&running);
            })
            .ok()?;
        match started.recv_timeout(TIMEOUT) {
            Ok(true) => Some(Self { stop: Some(stop_tx), thread: Some(thread) }),
            Ok(false) => {
                let _ = thread.join();
                None
            }
            Err(_) => {
                // Still waiting for ScreenCaptureKit (likely a permission prompt); let it finish
                // and clean up on its own instead of blocking the UI any longer.
                let _ = stop_tx.send(());
                eprintln!("reflux: system audio: ScreenCaptureKit timed out");
                None
            }
        }
    }
}

impl Drop for SystemAudio {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct Running {
    stream: Retained<SCStream>,
    _output: Retained<Output>,
    _queue: DispatchRetained<DispatchQueue>,
}

fn build(sink: impl FnMut(&[f32]) + Send + 'static) -> Option<Running> {
    let cfg = unsafe { SCStreamConfiguration::new() };
    if !cfg.respondsToSelector(sel!(setCapturesAudio:)) {
        eprintln!("reflux: system audio: ScreenCaptureKit audio needs macOS 13");
        return None;
    }
    let content = shareable_content()?;
    let displays = unsafe { content.displays() };
    if displays.count() == 0 {
        eprintln!("reflux: system audio: no displays");
        return None;
    }
    let display = displays.objectAtIndex(0);
    let apps = unsafe { content.applications() };
    let pid = std::process::id() as i32;
    let excluded: Vec<Retained<SCRunningApplication>> = (0..apps.count())
        .map(|i| apps.objectAtIndex(i))
        .filter(|app| unsafe { app.processID() } == pid)
        .collect();
    let excluded: Vec<&SCRunningApplication> = excluded.iter().map(|a| a.as_ref()).collect();
    let excluded = NSArray::from_slice(&excluded);
    let windows: Retained<NSArray<SCWindow>> = NSArray::new();
    let filter =
        unsafe { SCContentFilter::initWithDisplay_excludingApplications_exceptingWindows(SCContentFilter::alloc(), &display, &excluded, &windows) };
    unsafe {
        cfg.setCapturesAudio(true);
        cfg.setExcludesCurrentProcessAudio(true);
        cfg.setSampleRate(RATE as isize);
        cfg.setChannelCount(2);
        cfg.setQueueDepth(8);
        cfg.setWidth(2);
        cfg.setHeight(2);
        cfg.setMinimumFrameInterval(CMTime { value: 1, timescale: 1, flags: CMTimeFlags(1), epoch: 0 });
        cfg.setShowsCursor(false);
    }
    let stream = unsafe { SCStream::initWithFilter_configuration_delegate(SCStream::alloc(), &filter, &cfg, None) };
    let output = Output::new(sink);
    let queue = DispatchQueue::new("reflux.system-audio", None);
    for kind in [SCStreamOutputType::Screen, SCStreamOutputType::Audio] {
        unsafe { stream.addStreamOutput_type_sampleHandlerQueue_error(output.protocol(), kind, Some(&*queue)) }
            .map_err(|e| eprintln!("reflux: system audio: {e}"))
            .ok()?;
    }
    start_capture(&stream)?;
    Some(Running { stream, _output: output, _queue: queue })
}

fn stop(running: &Running) {
    let sem = DispatchSemaphore::new(0);
    let signal = sem.clone();
    let block = RcBlock::new(move |_err: *mut NSError| {
        signal.signal();
    });
    unsafe { running.stream.stopCaptureWithCompletionHandler(Some(&block)) };
    let _ = sem.wait(deadline());
}

fn deadline() -> DispatchTime {
    DispatchTime::try_from(TIMEOUT).unwrap_or(DispatchTime::FOREVER)
}

fn shareable_content() -> Option<Retained<SCShareableContent>> {
    let sem = DispatchSemaphore::new(0);
    let signal = sem.clone();
    let content: Arc<Mutex<Option<Retained<SCShareableContent>>>> = Arc::default();
    let slot = content.clone();
    let block = RcBlock::new(move |content: *mut SCShareableContent, err: *mut NSError| {
        if err.is_null() && !content.is_null() {
            if let Ok(mut slot) = slot.lock() {
                *slot = unsafe { Retained::retain(content) };
            }
        }
        signal.signal();
    });
    unsafe { SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnly_completionHandler(false, false, &block) };
    let _ = sem.wait(deadline());
    content.lock().unwrap_or_else(|e| e.into_inner()).take()
}

/// Returns `Some(())` only if `startCapture` completed without an error.
fn start_capture(stream: &SCStream) -> Option<()> {
    let sem = DispatchSemaphore::new(0);
    let signal = sem.clone();
    let error: Arc<Mutex<Option<String>>> = Arc::default();
    let slot = error.clone();
    let block = RcBlock::new(move |err: *mut NSError| {
        if !err.is_null() {
            if let Ok(mut slot) = slot.lock() {
                *slot = Some(unsafe { (*err).localizedDescription() }.to_string());
            }
        }
        signal.signal();
    });
    unsafe { stream.startCaptureWithCompletionHandler(Some(&block)) };
    if sem.wait(deadline()) != 0 {
        eprintln!("reflux: system audio: startCapture timed out");
        return None;
    }
    match error.lock().unwrap_or_else(|e| e.into_inner()).take() {
        Some(e) => {
            eprintln!("reflux: system audio: {e} (Screen Recording permission?)");
            None
        }
        None => Some(()),
    }
}

struct Sink {
    f: Box<dyn FnMut(&[f32]) + Send>,
    resampler: Resampler,
}

struct Ivars {
    sink: Mutex<Sink>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "RefluxSystemAudioOutput"]
    #[ivars = Ivars]
    struct Output;

    unsafe impl NSObjectProtocol for Output {}

    unsafe impl SCStreamOutput for Output {
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        fn did_output(&self, _stream: &SCStream, buffer: &CMSampleBuffer, kind: SCStreamOutputType) {
            if kind != SCStreamOutputType::Audio {
                return;
            }
            let Some(samples) = audio_samples(buffer) else { return };
            let Ok(mut sink) = self.ivars().sink.lock() else { return };
            let out = sink.resampler.push(&samples);
            if !out.is_empty() {
                (sink.f)(&out);
            }
        }
    }
);

impl Output {
    fn new(sink: impl FnMut(&[f32]) + Send + 'static) -> Retained<Self> {
        let this = Self::alloc().set_ivars(Ivars { sink: Mutex::new(Sink { f: Box::new(sink), resampler: Resampler::new(RATE, 2) }) });
        unsafe { msg_send![super(this), init] }
    }

    fn protocol(&self) -> &ProtocolObject<dyn SCStreamOutput> {
        ProtocolObject::from_ref(self)
    }
}

/// Copies one audio sample buffer out as interleaved f32. ScreenCaptureKit is asked for 48 kHz
/// stereo float; anything else is dropped rather than guessed at.
fn audio_samples(buffer: &CMSampleBuffer) -> Option<Vec<f32>> {
    unsafe {
        let desc = buffer.format_description()?;
        let asbd = CMAudioFormatDescriptionGetStreamBasicDescription(&desc);
        if asbd.is_null() {
            return None;
        }
        let asbd = *asbd;
        if asbd.mFormatID != LINEAR_PCM || asbd.mFormatFlags & FLAG_IS_FLOAT == 0 || asbd.mBitsPerChannel != 32 {
            return None;
        }
        let frames = buffer.num_samples();
        if frames <= 0 {
            return None;
        }
        let frames = frames as usize;
        let mut size = 0usize;
        buffer.audio_buffer_list_with_retained_block_buffer(&mut size, std::ptr::null_mut(), 0, None, None, 0, std::ptr::null_mut());
        if size < std::mem::size_of::<AudioBufferList>() {
            return None;
        }
        // `AudioBufferList` needs pointer alignment; u64 storage guarantees it.
        let mut storage = vec![0u64; size.div_ceil(8)];
        let list = storage.as_mut_ptr().cast::<AudioBufferList>();
        let mut block: *mut CMBlockBuffer = std::ptr::null_mut();
        let status = buffer.audio_buffer_list_with_retained_block_buffer(std::ptr::null_mut(), list, size, None, None, ALIGN_BUFFERS, &mut block);
        if status != 0 {
            return None;
        }
        let _block = NonNull::new(block).map(|p| CFRetained::<CMBlockBuffer>::from_raw(p));
        read_samples(list, &asbd, frames)
    }
}

unsafe fn read_samples(list: *const AudioBufferList, asbd: &AudioStreamBasicDescription, frames: usize) -> Option<Vec<f32>> {
    let count = unsafe { (*list).mNumberBuffers } as usize;
    if count == 0 {
        return None;
    }
    let buffers = unsafe { std::slice::from_raw_parts(std::ptr::addr_of!((*list).mBuffers).cast::<AudioBuffer>(), count) };
    let channels = asbd.mChannelsPerFrame as usize;
    if channels == 0 {
        return None;
    }
    if asbd.mFormatFlags & FLAG_IS_NON_INTERLEAVED != 0 {
        if buffers.iter().take(channels).any(|b| b.mData.is_null()) {
            return None;
        }
        let planes: Vec<&[f32]> = buffers
            .iter()
            .take(channels)
            .map(|b| unsafe { std::slice::from_raw_parts(b.mData.cast::<f32>(), (b.mDataByteSize as usize / 4).min(frames)) })
            .collect();
        if planes.len() < channels || planes.iter().any(|p| p.len() < frames) {
            return None;
        }
        Some(super::interleave(&planes, frames, channels))
    } else {
        let b = buffers.first()?;
        let samples = b.mDataByteSize as usize / 4;
        if b.mData.is_null() || samples < frames * channels {
            return None;
        }
        Some(unsafe { std::slice::from_raw_parts(b.mData.cast::<f32>(), frames * channels) }.to_vec())
    }
}
