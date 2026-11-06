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
    let (k1, k2, u1) = (key.clone(), key.clone(), url.clone());
    let button = row()
        .size(30.0, 30.0)
        .center()
        .radius(dim::RADIUS_FULL)
        .fill(p.brand_primary)
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(if playing { "Pause" } else { "Play" })
        .child(icon(if playing { &icons::PAUSE_FILL } else { &icons::PLAY_FILL }, 14.0).color(p.text_on_brand_primary))
        .on_press(move |s: &mut S| if has { super::clip::toggle(acc, &k1, s) } else { super::clip::start(acc(s), k1.clone(), u1.clone(), true) });
    let wave = bars(&values, played.min(BARS), false, p).cursor(rune::Cursor::Pointer).role("slider").label("Voice message").on_drag(move |s: &mut S, d, _| {
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
        .label("Voice message")
        .child(button)
        .child(wave)
        .child(text(super::attachments::clock(shown)).font(font(11.0, 500.0).lh(14.0)).color(p.text_secondary))
}

/// `VoiceMessageComposerModal`.
pub fn modal<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let vm = st.voice.voice_message.as_ref()?;
    let op = move |s: &mut S, o: Op| acc(s).intents.push(Intent::Voice(o));
    let close = move |s: &mut S| op(s, Op::RecordCancel);
    let wave = if vm.recording { bars(&resample(&vm.bars, 24), 24, true, p) } else { bars(&resample(&vm.bars, BARS), 0, false, p) };
    let status = if vm.recording { "Recording…" } else { "Recorded" };
    let mut body = col()
        .w_full()
        .gap(12.0)
        .child(
            row()
                .w_full()
                .gap(12.0)
                .center_y()
                .padding(12.0)
                .radius(dim::RADIUS_2XL)
                .fill(if vm.recording { p.brand_primary } else { p.background_secondary })
                .child(icon(&icons::MICROPHONE, 18.0).color(if vm.recording { p.text_on_brand_primary } else { p.text_primary_muted }))
                .child(wave)
                .child(text(super::attachments::clock(vm.secs as f64)).font(font(12.0, 500.0).lh(16.0)).color(if vm.recording { p.text_on_brand_primary } else { p.text_secondary })),
        )
        .child(text(status).font(font(12.0, 400.0).lh(16.0)).color(p.text_tertiary));
    if let Some(e) = &vm.error {
        body = body.child(text(e.clone()).font(font(13.0, 400.0).lh(18.0)).color(p.status_danger).wrap());
    }
    let footer = if vm.recording {
        row()
            .gap(8.0)
            .child(Button::new("Cancel").variant(ButtonVariant::Secondary).key("vm-cancel").on_press(close).el(p))
            .child(Button::new("Stop recording").key("vm-stop").on_press(move |s: &mut S| op(s, Op::RecordStop)).el(p))
    } else {
        let ch = vm.channel;
        row()
            .gap(8.0)
            .child(Button::new("Discard").variant(ButtonVariant::Secondary).key("vm-discard").on_press(close).el(p))
            .child(Button::new("Record again").variant(ButtonVariant::Secondary).key("vm-again").on_press(move |s: &mut S| op(s, Op::RecordStart(ch))).el(p))
            .child(Button::new("Send").key("vm-send").on_press(move |s: &mut S| op(s, Op::RecordSend)).el(p))
    };
    Some(Modal::new("Voice message", close).body(body).footer(footer).el(p))
}

#[cfg(test)]
mod tests {
    #[test]
    fn resample_pads_and_averages() {
        assert_eq!(super::resample(&[1.0], 3), [1.0, 0.0, 0.0]);
        assert_eq!(super::resample(&[1.0, 0.0, 0.5, 0.5], 2), [0.5, 0.5]);
        assert_eq!(super::resample(&[0.2; 100], 32).len(), 32);
    }
}
