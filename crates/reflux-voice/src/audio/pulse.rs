//! Linux system audio through the PulseAudio protocol (PipeWire's pulse server, or
//! PulseAudio itself): what the device pickers list and what the call plays and records.
//!
//! cpal's only Linux host is ALSA, whose device list is the raw plugin/PCM set
//! ("default", "pipewire", "sysdefault", every HDA profile) rather than the user's sinks and
//! sources, and whose playback stream the ALSA plugin gives a small buffer. libpulse sees the
//! same devices as the browser (which is what Fluxer's picker shows) and lets the stream ask the
//! server for a queued playback buffer, so a late mixer wakeup does not become an underrun.
//!
//! cpal stays the fallback (`audio::open_output`/`open_input`) when no server answers, e.g. a
//! bare ALSA or JACK machine.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use libpulse_binding as pulse;
use libpulse_simple_binding::Simple;
use pulse::callbacks::ListResult;
use pulse::context::{Context, FlagSet, State};
use pulse::def::BufferAttr;
use pulse::mainloop::standard::{IterateResult, Mainloop};
use pulse::sample::{Format, Spec};
use pulse::stream::Direction;

use super::{Apm, IoThread, MicChain, Mixer, FRAME, RATE};

/// Playback queued in the server. The old cpal path used the ALSA plugin's small default
/// buffer, so a mixer lock or a scheduler hiccup between callbacks was an audible xrun; the
/// server now keeps this much ahead of the speaker (with a 10 ms request size).
const PLAY_TLENGTH_MS: u32 = 80;
const PLAY_PREBUF_MS: u32 = 40;
const PLAY_MINREQ_MS: u32 = 10;
/// Record chunk; the server hands us this much per read.
const REC_FRAGSIZE_MS: u32 = 20;
const CHANNELS: u8 = 2;

/// A picker entry: the pulse name (`sidealsa-line4`) and its label (`SideALSA Line 4`).
pub(super) type Device = (String, String);
pub(super) type Devices = Vec<Device>;

fn spec(channels: u8) -> Spec {
    Spec { format: Format::F32le, rate: RATE, channels }
}

fn bytes_per_ms(channels: u8) -> u32 {
    RATE / 1000 * channels as u32 * 4
}

/// Whether a sink/source is one of Fluxer's own screen-share loopbacks, which the browser
/// client hides too (`INTERNAL_VIRTUAL_AUDIO_DEVICE_LABELS`).
pub(super) fn is_virtual(name: &str, description: &str) -> bool {
    let both = format!("{} {}", name.to_lowercase(), description.to_lowercase());
    ["fluxer-screen-share", "fluxer screen share", "fluxer-direct-capture", "fluxer direct capture"].iter().any(|x| both.contains(x))
}

/// The picker entries for a server list: the description (what the browser calls the label),
/// deduplicated with the stable pulse name, sorted like Fluxer (by label).
// shortcut: the picker persists this label, not the immutable pulse name; a renamed sink reads as unplugged and falls back to the server default. Upgrade when the device lists carry (id, label) pairs.
pub(super) fn labels(mut list: Devices) -> Devices {
    list.retain(|(name, description)| !is_virtual(name, description));
    list.sort_by_key(|(_, label)| label.to_lowercase());
    let mut seen = std::collections::HashSet::new();
    for (name, description) in &mut list {
        if description.is_empty() {
            *description = name.clone();
        }
        if !seen.insert(description.to_lowercase()) {
            *description = format!("{description} ({name})");
        }
    }
    list
}

/// The pulse name (`sidealsa-line4`) selected by a picker label (or the name itself, or a
/// deduplicated `"label (name)"`); `None` = the server default.
pub(super) fn pick(list: &[Device], wanted: Option<&str>) -> Option<String> {
    let wanted = wanted?;
    list.iter()
        .find(|(name, label)| label == wanted || name == wanted)
        .or_else(|| list.iter().find(|(name, label)| label.starts_with(&format!("{wanted} (")) || name == wanted))
        .map(|(name, _)| name.clone())
}

fn connect() -> Option<(Mainloop, Context)> {
    let mut mainloop = Mainloop::new()?;
    let mut context = Context::new(&mainloop, "Reflux")?;
    // NOAUTOSPAWN: without a running server this must fail instead of starting one.
    context.connect(None, FlagSet::NOAUTOSPAWN, None).ok()?;
    let deadline = Instant::now() + Duration::from_secs(1);
    while context.get_state() != State::Ready {
        if matches!(context.get_state(), State::Failed | State::Terminated) || Instant::now() >= deadline {
            return None;
        }
        if !iterate(&mut mainloop) {
            return None;
        }
    }
    Some((mainloop, context))
}

/// One non-blocking mainloop pass; blocking would let a stalled server outlast the deadline.
fn iterate(mainloop: &mut Mainloop) -> bool {
    let ok = matches!(mainloop.iterate(false), IterateResult::Success(_));
    std::thread::sleep(Duration::from_millis(1));
    ok
}

fn pump(mainloop: &mut Mainloop, done: &AtomicBool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(1);
    while !done.load(Ordering::Relaxed) {
        if Instant::now() >= deadline || !iterate(mainloop) {
            return false;
        }
    }
    true
}

/// The server's sinks and sources as `(pulse name, picker label)`. `None` when no server
/// answers (the caller falls back to cpal).
pub(super) fn enumerate() -> Option<(Devices, Devices)> {
    let (mut mainloop, context) = connect()?;
    let sinks = Arc::new(Mutex::new(Vec::new()));
    let sources = Arc::new(Mutex::new(Vec::new()));
    let done = Arc::new(AtomicBool::new(false));

    {
        let sinks = sinks.clone();
        let flag = done.clone();
        let _op = context.introspect().get_sink_info_list(move |r| match r {
            ListResult::Item(i) => {
                if let Some(name) = i.name.as_deref() {
                    sinks.lock().unwrap().push((name.to_string(), i.description.as_deref().unwrap_or(name).to_string()));
                }
            }
            ListResult::End | ListResult::Error => flag.store(true, Ordering::Relaxed),
        });
        if !pump(&mut mainloop, &done) {
            return None;
        }
    }
    done.store(false, Ordering::Relaxed);
    {
        let sources = sources.clone();
        let flag = done.clone();
        let _op = context.introspect().get_source_info_list(move |r| match r {
            ListResult::Item(i) => {
                if let Some(name) = i.name.as_deref() {
                    sources.lock().unwrap().push((name.to_string(), i.description.as_deref().unwrap_or(name).to_string()));
                }
            }
            ListResult::End | ListResult::Error => flag.store(true, Ordering::Relaxed),
        });
        if !pump(&mut mainloop, &done) {
            return None;
        }
    }
    let take = |m: &Arc<Mutex<Devices>>| labels(m.lock().unwrap().clone());
    Some((take(&sinks), take(&sources)))
}

fn resolve(dir: Direction, wanted: Option<&str>) -> Option<Option<String>> {
    let (sinks, sources) = enumerate()?;
    let list = if dir == Direction::Playback { sinks } else { sources };
    match wanted {
        Some(name) if !list.iter().any(|(n, l)| n == name || l == name) => {
            // Unplugged since it was picked: play on the server default rather than failing.
            eprintln!("reflux-voice: audio device {name:?} is gone; using the default");
            Some(None)
        }
        Some(name) => Some(pick(&list, Some(name)).or(Some(name.to_string()))),
        None => Some(None),
    }
}

/// Plays the mixer on `device` at 48 kHz stereo through the server. `None` when no server
/// answers or the stream cannot be opened (the caller falls back to cpal).
pub(super) fn open_output(device: Option<&str>, mixer: Arc<Mutex<Mixer>>, apm: Apm) -> Option<IoThread> {
    let target = resolve(Direction::Playback, device)?;
    let attr = BufferAttr {
        maxlength: u32::MAX,
        tlength: PLAY_TLENGTH_MS * bytes_per_ms(CHANNELS),
        prebuf: PLAY_PREBUF_MS * bytes_per_ms(CHANNELS),
        minreq: PLAY_MINREQ_MS * bytes_per_ms(CHANNELS),
        fragsize: u32::MAX,
    };
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = std::sync::mpsc::channel();
    let st = stop.clone();
    std::thread::spawn(move || {
        let Some(simple) = Simple::new(None, "Reflux", Direction::Playback, target.as_deref(), "Voice", &spec(CHANNELS), None, Some(&attr)).ok() else {
            let _ = tx.send(false);
            return;
        };
        let _ = tx.send(true);
        let mut mix = vec![0.0f32; FRAME];
        let mut pcm = vec![0u8; FRAME * CHANNELS as usize * 4];
        while !st.load(Ordering::Relaxed) {
            mixer.lock().unwrap().mix(&mut mix);
            for (out, s) in pcm.as_chunks_mut::<8>().0.iter_mut().zip(&mix) {
                // Mono in, the same sample on both channels.
                out[..4].copy_from_slice(&s.to_le_bytes());
                out[4..].copy_from_slice(&s.to_le_bytes());
            }
            if simple.write(&pcm).is_err() {
                break;
            }
            // The echo canceller's far end: what we just queued, in 10 ms frames.
            if let Some(apm) = apm.lock().unwrap().as_mut() {
                let mut i16pcm: Vec<i16> = mix.iter().map(|&s| (s.clamp(-1.0, 1.0) * 32767.0) as i16).collect();
                let _ = apm.process_reverse_stream(&mut i16pcm, RATE as i32, 1);
            }
        }
    });
    rx.recv().ok().filter(|ok| *ok).map(|_| IoThread { stop })
}

/// Records `device` at 48 kHz mono through the server into `chain`.
pub(super) fn open_input(device: Option<&str>, chain: MicChain) -> Option<IoThread> {
    open_capture(device, Arc::new(Mutex::new(move |samples: &[f32]| chain.feed(samples.iter().copied()))))
}

/// Records `device` at 48 kHz mono through the server, handing each block to `sink`: the call's
/// input, and (through [`super::capture`]) the mic test meter and voice messages.
pub(super) fn open_capture(device: Option<&str>, sink: super::CaptureSink) -> Option<IoThread> {
    let target = resolve(Direction::Record, device)?;
    let attr = BufferAttr { maxlength: u32::MAX, tlength: u32::MAX, prebuf: u32::MAX, minreq: u32::MAX, fragsize: REC_FRAGSIZE_MS * bytes_per_ms(1) };
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = std::sync::mpsc::channel();
    let st = stop.clone();
    std::thread::spawn(move || {
        let Some(simple) = Simple::new(None, "Reflux", Direction::Record, target.as_deref(), "Microphone", &spec(1), None, Some(&attr)).ok() else {
            let _ = tx.send(false);
            return;
        };
        let _ = tx.send(true);
        let mut buf = vec![0u8; FRAME * 4];
        while !st.load(Ordering::Relaxed) {
            if simple.read(&mut buf).is_err() {
                break;
            }
            let samples: Vec<f32> = buf.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect();
            sink.lock().unwrap()(&samples);
        }
    });
    rx.recv().ok().filter(|ok| *ok).map(|_| IoThread { stop })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(name: &str, label: &str) -> (String, String) {
        (name.to_string(), label.to_string())
    }

    #[test]
    fn labels_hide_fluxer_loopbacks_and_sort() {
        let list = labels(vec![
            dev("sidealsa-line2", "SideALSA Line 2"),
            dev("fluxer-direct-capture-1", "Fluxer Direct Capture Audio"),
            dev("sidealsa-line1", "SideALSA Line 1"),
        ]);
        assert_eq!(list, vec![dev("sidealsa-line1", "SideALSA Line 1"), dev("sidealsa-line2", "SideALSA Line 2")]);
    }

    #[test]
    fn duplicate_labels_get_the_stable_name() {
        let list = labels(vec![dev("usb-a", "Headset"), dev("usb-b", "Headset")]);
        assert_eq!(list[0], dev("usb-a", "Headset"));
        assert_eq!(list[1], dev("usb-b", "Headset (usb-b)"));
    }

    #[test]
    fn picks_the_selected_label_or_name_and_defaults() {
        let list = labels(vec![dev("sidealsa-line4", "SideALSA Line 4"), dev("usb-a", "Headset"), dev("usb-a2", "Headset")]);
        assert_eq!(pick(&list, Some("SideALSA Line 4")).as_deref(), Some("sidealsa-line4"));
        assert_eq!(pick(&list, Some("sidealsa-line4")).as_deref(), Some("sidealsa-line4"));
        assert_eq!(pick(&list, Some("Headset (usb-a2)")).as_deref(), Some("usb-a2"));
        assert_eq!(pick(&list, Some("Headset")).as_deref(), Some("usb-a"));
        assert_eq!(pick(&list, None), None);
        assert_eq!(pick(&list, Some("Gone")), None, "the caller falls back to the server default");
    }
}
