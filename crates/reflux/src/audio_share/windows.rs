//! WASAPI loopback: the system mix minus this process's own sound (process loopback, Windows 10
//! 2004+, as Chromium's `loopbackWithMute`), else the whole default render device. Shared mode
//! only; we convert/resample to 48 kHz stereo. Drop stops the capture thread.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Media::Audio::{
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_LOOPBACK,
    AUDCLNT_STREAMFLAGS_NOPERSIST, AUDIOCLIENT_ACTIVATION_PARAMS, AUDIOCLIENT_ACTIVATION_PARAMS_0, AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
    AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS, ActivateAudioInterfaceAsync, IActivateAudioInterfaceAsyncOperation, IActivateAudioInterfaceCompletionHandler,
    IActivateAudioInterfaceCompletionHandler_Impl, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator,
    PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE, VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, WAVEFORMATEX, WAVEFORMATEXTENSIBLE, eConsole, eRender,
};
use windows::Win32::System::Com::StructuredStorage::{PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0};
use windows::Win32::System::Com::{BLOB, CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::Win32::System::Variant::VT_BLOB;
use windows::core::Interface;

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
    let (client, (rate, channels, sample), event) = match unsafe { process_client() } {
        Ok(c) => c,
        Err(e) => {
            eprintln!("reflux: system audio: {e}; capturing the whole device, this app included");
            let (client, format) = unsafe { device_client() }?;
            (client, format, None)
        }
    };
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
            match &event {
                Some(e) => _ = unsafe { WaitForSingleObject(e.0, 100) },
                None => std::thread::sleep(Duration::from_millis(10)),
            }
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

/// The event a process-loopback client signals when it has audio.
struct Event(HANDLE);

impl Drop for Event {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

#[windows::core::implement(IActivateAudioInterfaceCompletionHandler)]
struct Activated(Mutex<Option<Sender<()>>>);

impl IActivateAudioInterfaceCompletionHandler_Impl for Activated_Impl {
    fn ActivateCompleted(&self, _: windows::core::Ref<IActivateAudioInterfaceAsyncOperation>) -> windows::core::Result<()> {
        if let Some(tx) = self.0.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = tx.send(());
        }
        Ok(())
    }
}

type Format = (u32, usize, SampleFormat);

/// Everything this machine plays except this process tree (its voice chat, sounds and the
/// stream it watches), as 48 kHz stereo float.
unsafe fn process_client() -> Result<(IAudioClient, Format, Option<Event>), String> {
    let params = AUDIOCLIENT_ACTIVATION_PARAMS {
        ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
        Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
            ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                TargetProcessId: std::process::id(),
                ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE,
            },
        },
    };
    // Borrows `params`; never cleared (PropVariantClear would free the stack blob).
    let prop = PROPVARIANT {
        Anonymous: PROPVARIANT_0 {
            Anonymous: std::mem::ManuallyDrop::new(PROPVARIANT_0_0 {
                vt: VT_BLOB,
                Anonymous: PROPVARIANT_0_0_0 {
                    blob: BLOB { cbSize: std::mem::size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32, pBlobData: (&raw const params).cast_mut().cast() },
                },
                ..Default::default()
            }),
        },
    };
    let (tx, rx) = channel();
    let handler: IActivateAudioInterfaceCompletionHandler = Activated(Mutex::new(Some(tx))).into();
    let op = unsafe { ActivateAudioInterfaceAsync(VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, &IAudioClient::IID, Some(&prop), &handler) }
        .map_err(|e| format!("process loopback: {e}"))?;
    rx.recv_timeout(Duration::from_secs(3)).map_err(|_| "process loopback: activation timed out".to_string())?;
    let mut hr = windows::core::HRESULT(0);
    let mut unknown = None;
    unsafe { op.GetActivateResult(&mut hr, &mut unknown) }.map_err(|e| format!("process loopback: {e}"))?;
    hr.ok().map_err(|e| format!("process loopback: {e}"))?;
    let client: IAudioClient = unknown.ok_or("process loopback: no client")?.cast().map_err(|e| format!("process loopback: {e}"))?;
    // Process loopback has no mix format (GetMixFormat is E_NOTIMPL): ask for ours.
    let format = WAVEFORMATEX {
        wFormatTag: 3, // WAVE_FORMAT_IEEE_FLOAT
        nChannels: 2,
        nSamplesPerSec: 48_000,
        nAvgBytesPerSec: 48_000 * 8,
        nBlockAlign: 8,
        wBitsPerSample: 32,
        cbSize: 0,
    };
    let flags = AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_EVENTCALLBACK | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM;
    unsafe { client.Initialize(AUDCLNT_SHAREMODE_SHARED, flags, 200_000, 0, &format, None) }.map_err(|e| format!("process loopback stream: {e}"))?;
    let event = Event(unsafe { CreateEventW(None, false, false, None) }.map_err(|e| format!("event: {e}"))?);
    unsafe { client.SetEventHandle(event.0) }.map_err(|e| format!("event handle: {e}"))?;
    Ok((client, (48_000, 2, SampleFormat::F32), Some(event)))
}

/// The whole default render device, in its mix format.
unsafe fn device_client() -> Result<(IAudioClient, Format), String> {
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.map_err(|e| format!("device enumerator: {e}"))?;
    let device = unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eConsole) }.map_err(|e| format!("default render device: {e}"))?;
    let client: IAudioClient = unsafe { device.Activate(CLSCTX_ALL, None) }.map_err(|e| format!("audio client: {e}"))?;
    let mix = unsafe { client.GetMixFormat() }.map_err(|e| format!("mix format: {e}"))?;
    let format = unsafe { mix_format(mix) };
    let init = unsafe { client.Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_NOPERSIST, 0, 0, mix, None) };
    unsafe { CoTaskMemFree(Some(mix.cast())) };
    let format = format?;
    init.map_err(|e| format!("loopback stream: {e}"))?;
    Ok((client, format))
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
