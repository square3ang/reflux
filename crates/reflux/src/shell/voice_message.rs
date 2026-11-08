//! Voice messages: `VoiceMessagePlayer` for attachments carrying a waveform, and the
//! `VoiceMessageComposerModal` (record, review, send). Recording is `voice_input::Mic`; the file
//! is a mono WAV like Fluxer's (`prepareVoiceMessageWav`).

use reflux_model::Attachment;
use rune::{El, col, el, icon, row, text};

use super::voice::Op;
use super::{Intent, ShellAccess, ShellState};
use crate::icons;
use crate::theme::{Palette, dim, font};
use crate::ui::{Button, ButtonVariant, Modal, mix};
use crate::i18n::t;

/// `VOICE_PLAYBACK_RATE_STEPS`.
const RATES: [f32; 4] = [0.75, 1.0, 1.5, 2.0];

/// The step after `rate` (`MediaPlaybackRate` click), wrapping.
fn next_rate(rate: f32) -> f32 {
    RATES.iter().copied().find(|r| *r > rate + 0.01).unwrap_or(RATES[0])
}

/// `WAVEFORM_BAR_COUNT`.
const BARS: usize = 32;

/// `resampleWaveform`: pad with zeros, or average runs down to `n`.
pub fn resample(samples: &[f32], n: usize) -> Vec<f32> {
    if samples.len() <= n {
        let mut v = samples.to_vec();
        v.resize(n, 0.0);
        return v;
    }
    let ratio = samples.len() as f32 / n as f32;
    let mut out = Vec::with_capacity(n);
    let mut cursor = 0;
    while out.len() < n {
        let boundary = (((out.len() + 1) as f32 * ratio).round() as usize).min(samples.len());
        let run = &samples[cursor.min(boundary)..boundary];
        out.push(if run.is_empty() { 0.0 } else { run.iter().sum::<f32>() / run.len() as f32 });
        cursor = boundary;
    }
    out
}

/// The bars: each `(22·v + 2) / 24` of 20px tall, played ones highlighted.
fn bars(values: &[f32], played: usize, active: bool, p: &Palette) -> El {
    let (past, rest) = if active { (p.text_on_brand_primary, mix(p.text_on_brand_primary, rune::Color::TRANSPARENT, 30.0)) } else { (p.brand_primary_light, p.text_tertiary) };
    row().grow(1.0).min_w(0.0).h(20.0).gap(1.0).center_y().children(values.iter().enumerate().map(|(i, v)| {
        let h = 20.0 * (22.0 * v.clamp(0.0, 1.0) + 2.0) / 24.0;
        el().grow(1.0).min_w(1.5).h(h).radius(dim::RADIUS_SM).fill(if i < played { past } else { rest })
    }))
}

/// `VoiceMessagePlayer`: play button, waveform (click to seek), time.
pub fn player<S: 'static>(st: &ShellState, a: &Attachment, p: &Palette, acc: ShellAccess<S>) -> El {
    let key = format!("voice-{}", a.id);
    let url = a.url.clone().unwrap_or_default();
    let gst = super::clip::get(st, &key);
    let playing = gst.is_some_and(|c| c.player.playing);
    let dur = gst.and_then(|c| c.player.duration()).or(a.duration).unwrap_or(0.0);
    let now = gst.and_then(|c| c.player.position()).unwrap_or(0.0);
    let values = resample(&crate::voice_input::decode_waveform(a.waveform.as_deref().unwrap_or_default()), BARS);
    let played = if dur > 0.0 { ((now / dur) * BARS as f64).round() as usize } else { 0 };
    let has = gst.is_some();
    let rate = st.prefs.voice_message_rate;
    let (k1, k2, k3, u1) = (key.clone(), key.clone(), key.clone(), url.clone());
    let button = row()
        .size(30.0, 30.0)
        .center()
        .radius(dim::RADIUS_FULL)
        .fill(p.brand_primary)
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(if playing { t("Pause") } else { t("Play") })
        .child(icon(if playing { &icons::PAUSE_FILL } else { &icons::PLAY_FILL }, 14.0).color(p.text_on_brand_primary))
        .on_press(move |s: &mut S| {
            if has {
                return super::clip::toggle(acc, &k1, s);
            }
            super::clip::start(acc(s), k1.clone(), u1.clone(), true);
            if rate != 1.0 {
                super::clip::set_rate(acc, &k1, s, rate as f64);
            }
        });
    let speed = text(format!("{rate}x"))
        .font(font(11.0, 600.0).lh(14.0))
        .color(p.text_secondary)
        .px(4.0)
        .py(2.0)
        .radius(dim::RADIUS_SM)
        .hover(p.background_modifier_hover)
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(t("Playback speed: {formattedRate}").replace("{formattedRate}", &format!("{rate}x")))
        .on_press(move |s: &mut S| {
            let sh = acc(s);
            let r = next_rate(sh.prefs.voice_message_rate);
            sh.prefs.voice_message_rate = r;
            let prefs = Box::new(sh.prefs.clone());
            sh.intents.push(Intent::Prefs(prefs));
            super::clip::set_rate(acc, &k3, s, r as f64);
        });
    let wave = bars(&values, played.min(BARS), false, p).cursor(rune::Cursor::Pointer).role("slider").label(t("Voice message")).on_drag(move |s: &mut S, d, _| {
        if d.w > 0.0 && dur > 0.0 {
            super::clip::seek(acc, &k2, s, (d.x / d.w).clamp(0.0, 1.0) as f64 * dur);
        }
    });
    let shown = if playing || now > 0.0 { now } else { dur };
    row()
        .min_w(200.0)
        .max_w(300.0)
        .py(dim::SPACING_1)
        .px(dim::SPACING_2)
        .gap(dim::SPACING_2)
        .center_y()
        .radius(dim::RADIUS_2XL)
        .fill(p.background_secondary)
        .border(1.0, p.background_modifier_accent)
        .role("group")
        .label(t("Voice message"))
        .child(button)
        .child(wave)
        .child(text(super::attachments::clock(shown)).font(font(11.0, 500.0).lh(14.0)).color(p.text_secondary))
        .child(speed)
        .child(volume_button(st, &key, p, acc))
}

/// `getVolumeIcon`.
fn volume_icon(volume: f32, muted: bool) -> &'static rune::Icon {
    match volume {
        _ if muted || volume <= 0.0 => &icons::SPEAKER_X_FILL,
        v if v < 0.33 => &icons::SPEAKER_NONE_FILL,
        v if v < 0.67 => &icons::SPEAKER_LOW_FILL,
        _ => &icons::SPEAKER_HIGH_FILL,
    }
}

/// Opens (`Some`) or schedules closing (`None`: 200 ms on) the volume popout of `key`.
fn hover_volume<S: 'static>(acc: ShellAccess<S>, key: &str, s: &mut S, inside: bool, tx: &mut rune::Tx) {
    let pop = &mut acc(s).media_ui.volume_pop;
    if inside {
        *pop = Some((key.to_string(), None));
    } else if let Some((k, at)) = pop
        && k == key
    {
        *at = Some(std::time::Instant::now() + std::time::Duration::from_millis(200));
        tx.wake_after(std::time::Duration::from_millis(210));
    }
}

/// `MediaVerticalVolumeControl`'s mute button (16px icon); hovering it opens the slider popout.
fn volume_button<S: 'static>(st: &ShellState, key: &str, p: &Palette, acc: ShellAccess<S>) -> El {
    let (v, m) = (st.prefs.media_volume, st.prefs.media_muted);
    let (k1, k2) = (key.to_string(), key.to_string());
    row()
        .key(format!("{key}-vol"))
        .size(28.0, 28.0)
        .center()
        .radius(dim::RADIUS_SM)
        .hover(p.background_modifier_hover)
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(if m { t("Unmute") } else { t("Mute") })
        .child(icon(volume_icon(v, m), 16.0).color(p.text_secondary))
        .on_press(move |s: &mut S| {
            let (v, m) = (acc(s).prefs.media_volume, acc(s).prefs.media_muted);
            super::clip::set_volume(acc, &k1, s, v, !m);
        })
        .on_hover_change(move |s: &mut S, inside, tx| hover_volume(acc, &k2, s, inside, tx))
}

/// The open volume popout: a vertical 0–100 slider 8px above the button.
pub fn volume_layer<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    let Some((key, _)) = &st.media_ui.volume_pop else { return };
    let value = if st.prefs.media_muted { 0.0 } else { st.prefs.media_volume };
    let (k1, k2) = (key.clone(), key.clone());
    let rail = col()
        .w(4.0)
        .h_full()
        .radius(2.0)
        .fill(mix(p.interactive_muted, p.background_tertiary, 82.0))
        .child(el().w_full().grow(1.0 - value))
        .child(el().w_full().grow(value).radius(2.0).fill(p.brand_primary_foreground));
    let thumb = el().size(12.0, 12.0).radius(6.0).fill(p.background_primary).border(2.0, mix(p.background_primary, p.brand_primary, 80.0));
    // The thumb rides the free space split (1 - value) : value, as the horizontal slider does.
    let thumbs = col().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).items_center().child(el().grow(1.0 - value)).child(thumb).child(el().grow(value.max(0.0001)));
    let slider = el()
        .size(20.0, 80.0)
        .role("slider")
        .label(t("Volume"))
        .cursor(rune::Cursor::Pointer)
        .child(row().w_full().h_full().justify_center().child(rail))
        .child(thumbs)
        .on_drag(move |s: &mut S, d, _| {
            if d.h > 0.0 {
                let v = 1.0 - (d.y / d.h).clamp(0.0, 1.0);
                if let Some((_, at)) = &mut acc(s).media_ui.volume_pop {
                    *at = None;
                }
                super::clip::set_volume(acc, &k1, s, v, v <= 0.0);
            }
        });
    let card = col()
        .w(28.0)
        .py(8.0)
        .items_center()
        .radius(8.0)
        .fill(p.form_surface_background)
        .border(1.0, p.background_modifier_accent)
        .box_shadow(crate::ui::overlay::popout_shadow(p, 0.24, 0.14))
        .child(slider)
        .on_hover_change(move |s: &mut S, inside, tx| hover_volume(acc, &k2, s, inside, tx));
    layer.push(crate::ui::overlay::anchor(card, &format!("{key}-vol"), crate::ui::overlay::Placement::Top, 8.0));
}

/// Closes the volume popout once its 200 ms grace has passed.
pub fn poll_volume(st: &mut ShellState) -> bool {
    let due = st.media_ui.volume_pop.as_ref().and_then(|(_, at)| *at).is_some_and(|at| std::time::Instant::now() >= at);
    if due {
        st.media_ui.volume_pop = None;
    }
    due
}

/// `formatElapsedMs`: `m:ss.t`.
fn elapsed(secs: f32) -> String {
    let ms = (secs.max(0.0) * 1000.0) as u64;
    format!("{}:{:02}.{}", ms / 60_000, ms / 1000 % 60, ms % 1000 / 100)
}

/// The review waveform (`AudioWaveform`): peaks, the selection in the brand colour, a handle
/// at each end; dragging moves the nearest handle.
fn trim_wave<S: 'static>(vm: &super::voice::VoiceMessage, p: &Palette, acc: ShellAccess<S>) -> El {
    const N: usize = 80;
    // Stretched (not zero-padded) across the width: a 4 s clip has only 40 points.
    let len = vm.bars.len().max(1);
    let values: Vec<f32> = if vm.bars.len() >= N { resample(&vm.bars, N) } else { (0..N).map(|i| vm.bars.get(i * len / N).copied().unwrap_or(0.0)).collect() };
    let total = vm.secs.max(0.01);
    let handle = || el().w(3.0).h(56.0).radius(2.0).fill(p.brand_primary);
    let mut kids = Vec::with_capacity(N + 3);
    let (mut a, mut b) = (false, false);
    let mut head = vm.playhead();
    for (i, v) in values.iter().enumerate() {
        let at = (i as f32 + 0.5) / N as f32 * total;
        if head.is_some_and(|h| at >= h) {
            head = None;
            kids.push(el().w(2.0).h(56.0).fill(p.text_primary).key("head"));
        }
        if !a && at >= vm.start {
            a = true;
            kids.push(handle().key("start"));
        }
        if !b && at > vm.end {
            b = true;
            kids.push(handle().key("end"));
        }
        let on = at >= vm.start && at <= vm.end;
        kids.push(el().key(format!("b{i}")).grow(1.0).min_w(1.0).h((48.0 * v.clamp(0.0, 1.0)).max(2.0)).radius(1.0).fill(if on { p.brand_primary } else { p.text_tertiary }));
    }
    if !b {
        kids.push(handle().key("end"));
    }
    row()
        .w_full()
        .h(64.0)
        .gap(1.0)
        .center_y()
        .padding(4.0)
        .radius(dim::RADIUS_LG)
        .fill(p.background_tertiary)
        .cursor(rune::Cursor::Pointer)
        .role("slider")
        .label(t("Voice message"))
        .children(kids)
        .on_drag(move |s: &mut S, d, _| {
            if let Some(vm) = &mut acc(s).voice.voice_message
                && d.w > 0.0
            {
                crate::desktop::sound::stop_preview();
                vm.playing = None;
                vm.drag((d.x / d.w).clamp(0.0, 1.0) * vm.secs);
                vm.error = None;
            }
        })
}

/// `VoiceMessageComposerModal`: record (live bars, `Recording` and the elapsed time), then
/// review: trim with the handles, play the selection, re-record or send.
pub fn modal<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let vm = st.voice.voice_message.as_ref()?;
    let op = move |s: &mut S, o: Op| acc(s).intents.push(Intent::Voice(o));
    let close = move |s: &mut S| op(s, Op::RecordCancel);
    let small = |l: &str, i: &'static rune::Icon, key: &str| Button::new(l.to_string()).variant(ButtonVariant::Secondary).size(crate::ui::ButtonSize::Small).icon(i).key(key.to_string());
    let meta = |a: String, b: String, c: rune::Color| {
        row().w_full().center_y().child(text(a).font(font(12.0, 500.0).lh(16.0)).color(c).grow(1.0)).child(text(b).font(font(12.0, 500.0).lh(16.0)).color(p.text_secondary))
    };
    let reviewing = !vm.recording && vm.secs > 0.0;
    let hint = if reviewing { t("Drag the handles to trim, then press Send.") } else { t("Speak now. Press Stop when you are done, and you can trim the recording afterwards.") };
    let mut body = col().w_full().gap(12.0).child(text(hint).font(font(14.0, 400.0).lh(20.0)).color(p.text_secondary).wrap());
    if !reviewing {
        body = body.child(
            row()
                .key("live")
                .w_full()
                .gap(12.0)
                .center_y()
                .padding(12.0)
                .radius(dim::RADIUS_2XL)
                .fill(p.background_secondary)
                .child(icon(&icons::MICROPHONE_FILL, 18.0).color(p.text_primary_muted))
                .child(bars(&resample(&vm.bars, 24), 24, false, p)),
        );
        if vm.recording {
            body = body.child(meta(t("Recording").into(), elapsed(vm.secs), p.status_danger).key("recording"));
        }
    } else {
        let ch = vm.channel;
        let playing = vm.playhead().is_some();
        let (play, glyph) = if playing { (t("Pause"), &icons::PAUSE_FILL) } else { (t("Play"), &icons::PLAY_FILL) };
        body = body
            .child(trim_wave(vm, p, acc).key("wave"))
            .child(meta(t("Selection: {seconds}").replace("{seconds}", &format!("{:.2}s", vm.end - vm.start)), format!("{:.2}s → {:.2}s", vm.start, vm.end), p.text_secondary).key("selection"))
            .child(
                row()
                    .key("review")
                    .gap(8.0)
                    .child(small(play, glyph, "vm-play").on_press(move |s: &mut S| op(s, Op::RecordPlay)).el(p))
                    .child(small(t("Re-record"), &icons::ARROW_COUNTER_CLOCKWISE_BOLD, "vm-again").on_press(move |s: &mut S| op(s, Op::RecordStart(ch))).el(p)),
            );
    }
    if let Some(e) = &vm.error {
        body = body.child(text(e.clone()).font(font(13.0, 400.0).lh(18.0)).color(p.status_danger).wrap().key("error"));
    }
    let mut footer = row().gap(8.0).child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).key("vm-cancel").on_press(close).el(p));
    if vm.recording {
        footer = footer.child(Button::new(t("Stop")).icon(&icons::STOP_FILL).key("vm-stop").on_press(move |s: &mut S| op(s, Op::RecordStop)).el(p));
    } else if reviewing {
        let short = vm.end - vm.start < super::voice::VoiceMessage::MIN_SECS;
        footer = footer.child(Button::new(t("Send")).icon(&icons::PAPER_PLANE_RIGHT_FILL).key("vm-send").disabled(short).on_press(move |s: &mut S| op(s, Op::RecordSend)).el(p));
    } else {
        let ch = vm.channel;
        footer = footer.child(Button::new(t("Start recording")).icon(&icons::MICROPHONE_FILL).key("vm-retry").on_press(move |s: &mut S| op(s, Op::RecordStart(ch))).el(p));
    }
    Some(Modal::new(t("Voice message"), close).body(body).footer(footer).el(p))
}

#[cfg(test)]
mod tests {
    #[test]
    fn resample_pads_and_averages() {
        assert_eq!(super::resample(&[1.0], 3), [1.0, 0.0, 0.0]);
        assert_eq!(super::resample(&[1.0, 0.0, 0.5, 0.5], 2), [0.5, 0.5]);
        assert_eq!(super::resample(&[0.2; 100], 32).len(), 32);
    }

    #[test]
    fn trim_handles_and_range() {
        let mut vm = crate::shell::voice::VoiceMessage { secs: 4.0, end: 4.0, ..Default::default() };
        vm.drag(1.0);
        assert_eq!((vm.start, vm.end), (1.0, 4.0));
        vm.drag(1.2);
        assert_eq!(vm.start, 1.2);
        vm.drag(3.0);
        assert_eq!(vm.end, 3.0);
        vm.drag(2.9); // nearest is the end; keeps 0.5 s
        assert_eq!(vm.end, 2.9);
        (vm.start, vm.end) = (1.0, 1.5);
        vm.drag(1.4);
        assert_eq!(vm.end, 1.5);
        assert_eq!(vm.range(4 * 48_000, 48_000), 48_000..72_000);
        assert_eq!(vm.range(10, 48_000), 10..10);
        assert_eq!(super::elapsed(65.43), "1:05.4");
        assert_eq!([0.75, 1.0, 1.5, 2.0].map(super::next_rate), [1.0, 1.5, 2.0, 0.75]);
        assert_eq!(format!("{}x", 1.0f32), "1x");
    }
}
