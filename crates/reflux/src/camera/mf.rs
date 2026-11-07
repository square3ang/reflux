//! Windows camera capture with Media Foundation: `MFEnumDeviceSources` for the list, the
//! device's `IMFMediaSource` behind an `IMFSourceReader` whose video processor turns the chosen
//! native mode (NV12 / YUY2 / MJPG …) into RGB32, read on a thread. Frame parsing is `mf.rs`'s.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use windows::Win32::Foundation::E_FAIL;
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree};
use windows::core::{Error, PWSTR, Result};

use crate::mf::VIDEO;

pub fn available() -> bool {
    crate::mf::available()
}

/// Video capture devices: friendly name and activation object.
unsafe fn devices() -> Result<Vec<(String, IMFActivate)>> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let mut attrs = None;
        MFCreateAttributes(&mut attrs, 1)?;
        let attrs = attrs.ok_or_else(Error::empty)?;
        attrs.SetGUID(&MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE, &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID)?;
        let (mut list, mut n) = (std::ptr::null_mut(), 0u32);
        MFEnumDeviceSources(&attrs, &mut list, &mut n)?;
        let mut out = Vec::new();
        for i in 0..n as usize {
            let Some(act) = (*list.add(i)).take() else { continue };
            let (mut name, mut len) = (PWSTR::null(), 0u32);
            let label = match act.GetAllocatedString(&MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME, &mut name, &mut len) {
                Ok(()) => {
                    let s = name.to_string().unwrap_or_default();
                    CoTaskMemFree(Some(name.0 as _));
                    s
                }
                Err(_) => format!("Camera {}", i + 1),
            };
            out.push((label, act));
        }
        if !list.is_null() {
            CoTaskMemFree(Some(list as _));
        }
        Ok(out)
    }
}

pub fn cameras() -> Vec<String> {
    if !available() {
        return Vec::new();
    }
    let mut names: Vec<String> = unsafe { devices() }.map_err(|e| eprintln!("reflux: cameras: {e}")).unwrap_or_default().into_iter().map(|d| d.0).collect();
    names.dedup();
    names
}

/// A running camera: RGB frames from the native mode closest to `width`×`height`, at most `fps`.
pub struct CameraCapture {
    stop: Arc<AtomicBool>,
}

impl CameraCapture {
    /// `device`: a name from [`cameras`] (`None` = the first one).
    pub fn start(device: Option<&str>, width: u32, height: u32, fps: u32, sink: impl FnMut(u32, u32, Vec<u8>) + Send + 'static) -> Option<Self> {
        if !available() {
            return None;
        }
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::sync_channel(1);
        let (device, st) = (device.map(str::to_string), stop.clone());
        std::thread::Builder::new()
            .name("reflux-camera".into())
            .spawn(move || match unsafe { open(device.as_deref(), (width, height, fps)) } {
                Ok((source, reader)) => {
                    let _ = tx.send(true);
                    if let Err(e) = unsafe { pump(&reader, &st, fps, sink) } {
                        eprintln!("reflux: camera: {e}");
                    }
                    drop(reader);
                    let _ = unsafe { source.Shutdown() };
                }
                Err(e) => {
                    eprintln!("reflux: camera: {e}");
                    let _ = tx.send(false);
                }
            })
            .ok()?;
        // A camera that takes this long to open is given up on (the thread stops if it opens later).
        let ok = rx.recv_timeout(Duration::from_secs(10)).unwrap_or(false);
        if !ok {
            stop.store(true, Ordering::Relaxed);
        }
        ok.then_some(Self { stop })
    }
}

impl Drop for CameraCapture {
    // The thread notices at its next frame; not joined, so a stalled camera can't hang the UI.
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

unsafe fn open(device: Option<&str>, want: (u32, u32, u32)) -> Result<(IMFMediaSource, IMFSourceReader)> {
    unsafe {
        let list = devices()?;
        let (_, act) = list.iter().find(|d| device.is_none_or(|n| d.0 == n)).or(list.first()).ok_or_else(|| Error::new(E_FAIL, "no camera"))?;
        let source: IMFMediaSource = act.ActivateObject()?;
        let mut attrs = None;
        MFCreateAttributes(&mut attrs, 1)?;
        let attrs = attrs.ok_or_else(Error::empty)?;
        attrs.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)?;
        let reader = MFCreateSourceReaderFromMediaSource(&source, &attrs)?;
        let mut natives = Vec::new();
        while let Ok(mt) = reader.GetNativeMediaType(VIDEO, natives.len() as u32) {
            natives.push(mt);
        }
        let modes: Vec<(u32, u32, f64)> = natives
            .iter()
            .map(|mt| {
                let size = mt.GetUINT64(&MF_MT_FRAME_SIZE).unwrap_or(0);
                let rate = mt.GetUINT64(&MF_MT_FRAME_RATE).unwrap_or(0);
                let (num, den) = ((rate >> 32) as u32, rate as u32);
                ((size >> 32) as u32, size as u32, if den == 0 { 0.0 } else { num as f64 / den as f64 })
            })
            .collect();
        if let Some(i) = super::pick::mode(&modes, want) {
            reader.SetCurrentMediaType(VIDEO, None, &natives[i])?;
        }
        let mt = MFCreateMediaType()?;
        mt.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        mt.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
        reader.SetCurrentMediaType(VIDEO, None, &mt)?;
        reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)?;
        reader.SetStreamSelection(VIDEO, true)?;
        Ok((source, reader))
    }
}

/// Reads frames until `stop` (or the camera goes away).
unsafe fn pump(reader: &IMFSourceReader, stop: &AtomicBool, fps: u32, mut sink: impl FnMut(u32, u32, Vec<u8>)) -> Result<()> {
    unsafe {
        let mut format = crate::mf::video_format(reader);
        let mut pacer = super::pick::Pacer::new(fps);
        while !stop.load(Ordering::Relaxed) {
            let (flags, sample) = crate::mf::read(reader, VIDEO)?;
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                return Ok(());
            }
            if flags & MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0 as u32 != 0 {
                format = crate::mf::video_format(reader);
            }
            if let (Some(s), Some(f)) = (sample, format)
                && pacer.due(Duration::from_secs_f64(s.ts.max(0.0)))
                && let Some(rgba) = crate::mf::frame(&s.sample, &f)
            {
                sink(f.crop.2, f.crop.3, rgba);
            }
        }
        Ok(())
    }
}
