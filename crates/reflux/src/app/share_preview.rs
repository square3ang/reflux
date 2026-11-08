//! `useScreensharePreviewUploader`: while our screen (or camera-as-screen) share is live, a JPEG
//! still of it goes to the instance (`POST /streams/{key}/preview/upload-url`, then a presigned
//! `PUT` with no auth), so the call tiles and Active Now can show a thumbnail.
//!
//! The schedule is Fluxer's `PreviewUploadSchedule` (`e2`): upload until the first success (ten
//! tries, a second apart), then every 240 s ± 60 s while anyone watches, else look again in 5 s.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use serde_json::json;

use super::{now_ms, spawn, update};
use crate::State;

/// `ez.Zn`: the largest preview a server takes (its answer's `max_bytes` usually says the same).
const MAX_BYTES: usize = 1_000_000;

/// One RGBA frame of our share: width, height, rows top first.
pub type Still = (u32, u32, Vec<u8>);

#[derive(Default)]
pub struct Upload {
    /// The stream key this state belongs to; a new share starts over.
    pub key: Option<String>,
    pub uploaded: bool,
    pub attempts: u8,
    pub busy: bool,
    /// When the next upload (or check) is due.
    pub due_ms: u64,
}

pub(super) fn tick(s: &mut State, tx: &mut rune::Tx) {
    let Some(inst) = s.app.instance.clone() else { return };
    let me = s.app.store.my_id();
    let Some((guild, channel)) = s.app.shell.voice.channel else {
        s.app.voice.preview_upload = Upload::default();
        return;
    };
    // `isOwnScreenShare`: the screen or device share we publish, while it's connected.
    let live = s.app.shell.voice.screen && s.app.voice.share.is_some() || s.app.voice.device_share.is_some();
    let Some(conn) = s.app.store.voice_states.get(&(guild, me)).filter(|v| v.self_stream).and_then(|v| v.connection_id.clone()) else {
        s.app.voice.preview_upload = Upload::default();
        return;
    };
    let key = format!("{}:{channel}:{conn}", guild.map_or("dm".to_string(), |g| g.to_string()));
    if !live {
        s.app.voice.preview_upload = Upload::default();
        return;
    }
    let now = now_ms();
    let watched = !crate::shell::voice::spectators_for(&s.app.store, guild, me).is_empty();
    let up = &mut s.app.voice.preview_upload;
    if up.key.as_deref() != Some(&key) {
        *up = Upload { key: Some(key.clone()), ..Default::default() };
    }
    if up.busy {
        return;
    }
    // `hasUploadedOnce` false: try now; true: only when due.
    let due = if !up.uploaded && up.attempts < 10 { true } else { now >= up.due_ms };
    if !due {
        tx.wake_after(std::time::Duration::from_millis((up.due_ms.saturating_sub(now)).clamp(250, 5_000)));
        return;
    }
    // Fluxer's `decide`: a watcher asks for a fresh still every four minutes, otherwise check
    // back in five seconds.
    let next = if watched {
        // 240 s ± 60 s, spread by the clock so clients don't upload together.
        240_000 + (now % 120_000).saturating_sub(60_000)
    } else if up.uploaded {
        5_000
    } else {
        1_000
    };
    up.busy = true;
    up.due_ms = now + next;
    if !up.uploaded {
        up.attempts = up.attempts.saturating_add(1);
    }
    // shortcut: Fluxer snapshots the live video element; the call tile's renderer tees one
    // frame into `own_frame` when asked (a pointer tee of every frame would copy 8 MB/s), and
    // the 40 ms wait below is a frame at 30 fps.
    s.app.voice.want_own_frame.store(true, Ordering::Relaxed);
    let (frame, want) = (s.app.voice.own_frame.clone(), s.app.voice.want_own_frame.clone());
    let generation = s.app.generation;
    spawn(async move {
        // A frame at 30 fps arrives within a frame time; a paused share may never give one.
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        let still = frame.lock().unwrap().take();
        want.store(false, Ordering::Relaxed);
        let Some(still) = still else { return };
        let body = json!({ "channel_id": channel.to_string(), "content_type": "image/jpeg" });
        let r: Result<bool, String> = async {
            let answer: serde_json::Value = inst
                .rest
                .request(reqwest::Method::POST, &format!("/streams/{key}/preview/upload-url"), Some(&body))
                .await
                .map_err(|e| e.to_string())?;
            let url = answer["upload_url"].as_str().ok_or("no upload_url")?;
            let content_type = answer["content_type"].as_str().unwrap_or("image/jpeg");
            let max = answer["max_bytes"].as_u64().map_or(MAX_BYTES, |n| n as usize);
            let bytes = tokio::task::spawn_blocking(move || still_preview(&still, max)).await.ok().flatten();
            let Some(bytes) = bytes else { return Ok(false) };
            let n = bytes.len();
            inst.rest
                .put_upload(url, Arc::new(bytes), 0..n, Some(content_type), Arc::new(std::sync::atomic::AtomicU64::new(0)), Arc::new(|_| {}))
                .await
                .map_err(|e| e.to_string())?;
            Ok(true)
        }
        .await;
        update(move |s| {
            if s.app.generation != generation {
                return;
            }
            let up = &mut s.app.voice.preview_upload;
            up.busy = false;
            if up.key.as_deref() != Some(&key) {
                return;
            }
            match r {
                Ok(true) => up.uploaded = true,
                Ok(false) => eprintln!("reflux: stream preview: no still to upload"),
                Err(e) => {
                    eprintln!("reflux: stream preview upload: {e}");
                    // A refused key (no share, no permission) shouldn't retry for ever.
                    up.attempts = up.attempts.saturating_add(1);
                }
            }
        });
    });
}

/// Fluxer's `eH`: JPEG at 640 px and quality 0.7, dropping to 0.55 and 0.4, then shrinking by
/// 0.8 down to 160 px, until it fits `max_bytes` (six tries at most).
fn still_preview((w, h, rgba): &Still, max_bytes: usize) -> Option<Vec<u8>> {
    if *w == 0 || *h == 0 || rgba.len() != *w as usize * *h as usize * 4 {
        return None;
    }
    // JPEG has no alpha: the share's frame is opaque anyway.
    let mut rgb = Vec::with_capacity(*w as usize * *h as usize * 3);
    for px in rgba.as_chunks::<4>().0 {
        rgb.extend_from_slice(&px[..3]);
    }
    let img = image::RgbImage::from_raw(*w, *h, rgb)?;
    let (mut quality, mut max) = (0.7f32, 640u32);
    for _ in 0..6 {
        let scale = (max as f32 / (*w).max(*h) as f32).min(1.0);
        let (sw, sh) = (((*w as f32) * scale).round().max(1.0) as u32, ((*h as f32) * scale).round().max(1.0) as u32);
        let still = if (sw, sh) == (*w, *h) { img.clone() } else { image::imageops::resize(&img, sw, sh, image::imageops::FilterType::Triangle) };
        let mut out = Vec::new();
        let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, (quality * 100.0).round() as u8);
        enc.encode(still.as_raw(), sw, sh, image::ExtendedColorType::Rgb8).ok()?;
        if out.len() <= max_bytes {
            return Some(out);
        }
        if quality > 0.4 {
            quality = (quality - 0.15).max(0.4);
        } else if max > 160 {
            max = ((max as f32 * 0.8).round() as u32).max(160);
        } else {
            break;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    /// A gradient that shrinks to fit: the first try fits at 640, and a tight budget still
    /// returns a JPEG below 160 px rather than nothing.
    #[test]
    fn a_still_fits_the_budget() {
        let (w, h) = (1280u32, 720u32);
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                rgba.extend_from_slice(&[(x / 8) as u8, (y / 8) as u8, ((x + y) / 16) as u8, 255]);
            }
        }
        let still = (w, h, rgba);
        let big = super::still_preview(&still, 1_000_000).expect("fits at 640");
        assert!(big.len() <= 1_000_000);
        let jpeg = image::load_from_memory(&big).expect("decodes");
        assert!(jpeg.width() <= 640);
        // Six tries as Fluxer: three quality steps, then shrinking from 640 px by 0.8.
        let small = super::still_preview(&still, 6_000).expect("shrinks to fit 6 kB");
        assert!(small.len() <= 6_000);
        let jpeg = image::load_from_memory(&small).expect("decodes");
        assert!(jpeg.width() < 640, "shrank past the 640 px still: {}", jpeg.width());
        assert!(jpeg.width() >= 160);
    }
}
