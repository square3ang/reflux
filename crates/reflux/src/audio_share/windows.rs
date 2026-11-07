//! WASAPI loopback of the default render device: the system mix minus silence on Windows.
//! Shared mode only, so the endpoint must be running; WASAPI delivers its mix format and we
//! convert/resample to 48 kHz stereo. Drop stops the capture thread.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::time::Duration;

use windows::Win32::Media::Audio::{
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, AUDCLNT_STREAMFLAGS_NOPERSIST,
    IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator, WAVEFORMATEX, WAVEFORMATEXTENSIBLE, eConsole, eRender,
};
use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize};

use super::{Resampler, SampleFormat, le_to_f32};

pub struct SystemAudio {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl SystemAudio {
    pub fn start(mut sink: impl FnMut(&[f32]) + Send + 'static) -> Option<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let (ready, wait) = channel();
        let thread = std::thread::Builder::new()
            .name("reflux-system-audio".into())
            .spawn(move || {
                if let Err(e) = unsafe { capture(&flag, &mut sink, &ready) } {
                    eprintln!("reflux: system audio: {e}");
                    let _ = ready.send(Err(e));
                }
            })
            .ok()?;
        match wait.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Some(Self { stop, thread: Some(thread) }),
            Ok(Err(_)) => {
                let _ = thread.join();
                None
            }
            Err(_) => {
                // Detach: a COM call inside the thread must not hang the UI.
                stop.store(true, Ordering::Relaxed);
                eprintln!("reflux: system audio: starting WASAPI timed out");
                None
            }
        }
    }
}

impl Drop for SystemAudio {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct Com;

impl Drop for Com {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

/// Runs on the capture thread: sets WASAPI up and sends `Ok` once the stream is running.
unsafe fn capture(stop: &AtomicBool, sink: &mut impl FnMut(&[f32]), ready: &Sender<Result<(), String>>) -> Result<(), String> {
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok().map_err(|e| format!("COM: {e}"))?;
    let _com = Com;
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.map_err(|e| format!("device enumerator: {e}"))?;
    let device = unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eConsole) }.map_err(|e| format!("default render device: {e}"))?;
    let client: IAudioClient = unsafe { device.Activate(CLSCTX_ALL, None) }.map_err(|e| format!("audio client: {e}"))?;
    let mix = unsafe { client.GetMixFormat() }.map_err(|e| format!("mix format: {e}"))?;
    let format = unsafe { mix_format(mix) };
    let init = unsafe {
        client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_NOPERSIST,
            0,
            0,
            mix,
            None,
        )
    };
    unsafe { CoTaskMemFree(Some(mix.cast())) };
    let (rate, channels, sample) = format?;
    init.map_err(|e| format!("loopback stream: {e}"))?;
    let capture: IAudioCaptureClient = unsafe { client.GetService() }.map_err(|e| format!("capture client: {e}"))?;
    unsafe { client.Start() }.map_err(|e| format!("start: {e}"))?;
    let _ = ready.send(Ok(()));

    let mut resampler = Resampler::new(rate, channels);
    let mut pcm = Vec::new();
    while !stop.load(Ordering::Relaxed) {
        let mut frames = match unsafe { capture.GetNextPacketSize() } {
            Ok(n) => n,
            Err(e) => {
                eprintln!("reflux: system audio: {e}");
                break;
            }
        };
        if frames == 0 {
            std::thread::sleep(Duration::from_millis(10));
            continue;
        }
        while frames > 0 {
            let mut data: *mut u8 = std::ptr::null_mut();
            let mut n = 0u32;
            let mut flags = 0u32;
            if let Err(e) = unsafe { capture.GetBuffer(&mut data, &mut n, &mut flags, None, None) } {
                eprintln!("reflux: system audio: {e}");
                let _ = unsafe { client.Stop() };
                return Ok(());
            }
            if n > 0 {
                if flags as i32 & AUDCLNT_BUFFERFLAGS_SILENT.0 != 0 || data.is_null() {
                    pcm.clear();
                    pcm.resize(n as usize * channels, 0.0);
                } else {
                    let bytes = unsafe { std::slice::from_raw_parts(data, n as usize * channels * sample.size()) };
                    pcm = le_to_f32(bytes, sample);
                }
                let out = resampler.push(&pcm);
                if !out.is_empty() {
                    sink(&out);
                }
            }
            let _ = unsafe { capture.ReleaseBuffer(n) };
            frames = unsafe { capture.GetNextPacketSize() }.unwrap_or(0);
        }
    }
    let _ = unsafe { client.Stop() };
    Ok(())
}

/// `(rate, channels, sample format)` of a shared-mode mix format.
unsafe fn mix_format(p: *const WAVEFORMATEX) -> Result<(u32, usize, SampleFormat), String> {
    let f = unsafe { &*p };
    let (raw_tag, channels, rate, bits, extra) = (f.wFormatTag, f.nChannels, f.nSamplesPerSec, f.wBitsPerSample, f.cbSize);
    if channels == 0 || rate == 0 {
        return Err("empty mix format".into());
    }
    let tag = if raw_tag == 0xfffe && extra >= 22 {
        let ext = unsafe { &*(p as *const WAVEFORMATEXTENSIBLE) };
        unsafe { std::ptr::addr_of!(ext.SubFormat).read_unaligned().data1 as u16 }
    } else {
        raw_tag
    };
    let sample = match (tag, bits) {
        (3, 32) => SampleFormat::F32,
        (1, 16) => SampleFormat::I16,
        (1, 32) => SampleFormat::I32,
        _ => return Err(format!("unsupported mix format: tag {tag}, {bits} bits")),
    };
    Ok((rate, channels as usize, sample))
}
