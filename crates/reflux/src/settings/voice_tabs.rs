//! The Voice & video tab (`VoiceVideoTab.tsx` = `UserVoiceTab` + `UserVideoTab`): devices,
//! volume, voice processing (profiles, push-to-talk, activity threshold), mic test, voice
//! channel display, camera and screen share quality.

use reflux_model::Snowflake;
use rune::{El, col, el, row};
use crate::ui::mark::marked as text;

use super::tabs::section;
use crate::shell::voice::{Modal, Op, Settings};
use crate::shell::{Intent, ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonVariant, RadioGroup, Select, Slider, Switch};
use crate::i18n::t;

/// Changes a setting, then saves and applies it.
fn set<S: 'static>(acc: ShellAccess<S>, f: impl Fn(&mut Settings) + 'static) -> impl Fn(&mut S) + 'static {
    move |s: &mut S| {
        let sh = acc(s);
        f(&mut sh.voice.settings);
        sh.intents.push(Intent::Voice(Op::Settings));
    }
}

fn desc(s: &str, p: &Palette) -> El {
    text(s.to_string()).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted).wrap()
}

/// `SettingsTabSection`: a 15/600 heading over its content, 12 apart; the content 8 apart.
fn sub(title: &str, content: El, p: &Palette) -> El {
    col()
        .key(format!("sub-{title}"))
        .w_full()
        .gap(12.0)
        .child(text(t(title).to_string()).font(font(15.0, 600.0).lh(19.5)).color(p.text_primary))
        .child(col().w_full().gap(8.0).child(content))
}

/// `CompactComboboxRow`'s `controlWidth`s: the select's (min, max) width; it sizes to the shown
/// label's `ch`s + 4rem in between.
const WIDE: (f32, f32) = (288.0, 448.0);
const MEDIUM: (f32, f32) = (176.0, 288.0);
const SMALL: (f32, f32) = (128.0, 192.0);

fn combo_w(shown: &str, (min, max): (f32, f32)) -> f32 {
    (shown.chars().count() as f32 * 8.0 + 64.0).clamp(min, max)
}

/// `CompactComboboxRow`: the label (and description) on the left, a compact select on the right.
fn combo_row(label: &str, desc: Option<&str>, select: El, p: &Palette) -> El {
    let mut words = col().grow(1.0).min_w(0.0).gap(4.0).child(text(t(label).to_string()).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary));
    if let Some(d) = desc {
        words = words.child(text(d.to_string()).font(font(13.0, 400.0).lh(20.0)).color(p.text_primary_muted).wrap().pr(16.0));
    }
    row().w_full().min_h(44.0).py(4.0).gap(16.0).center_y().child(words).child(select)
}

/// A select in a [`combo_row`], sized like Fluxer's from the shown option.
#[allow(clippy::too_many_arguments)]
fn combo<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>, id: &'static str, opts: &[(&str, &str)], value: &str, width: (f32, f32), f: impl Fn(&mut S, &str) + 'static) -> El {
    let pops = crate::ui::popup_access(move |s: &mut S| &mut acc(s).popups);
    let shown = opts.iter().find(|(v, _)| *v == value).map_or("", |(_, l)| t(l));
    Select::new(id, opts, Some(value)).compact().width(combo_w(shown, width)).on_choose(f).el(&st.popups, pops, p, layer)
}

/// `SliderResetIconButton`: shown while the value isn't its default.
fn reset_icon<S: 'static>(key: String, can: bool, on: impl Fn(&mut S) + 'static, p: &Palette) -> Option<El> {
    can.then(|| {
        row()
            .key(key)
            .size(24.0, 24.0)
            .center()
            .radius(4.0)
            .hover(p.background_modifier_hover)
            .hover_group()
            .cursor(rune::Cursor::Pointer)
            .role("button")
            .label(t("Reset to default"))
            .child(rune::icon(&crate::icons::ARROW_COUNTER_CLOCKWISE_BOLD, 14.0).color(p.text_primary_muted).group_color(p.text_primary))
            .on_press(on)
    })
}

/// `.sliderLabelRow`: a 16/600 label and the reset button, 12 over the slider.
fn slider_label(name: &str, reset: Option<El>, p: &Palette) -> El {
    row().w_full().min_h(24.0).mb(12.0).gap(8.0).center_y().child(text(t(name).to_string()).font(font(16.0, 600.0).lh(20.8)).color(p.text_primary).grow(1.0).min_w(0.0)).children(reset)
}

/// `.pttSettingDescription` / `.deviceNoticeDescription`: 13px tertiary text.
fn note(s: &str, p: &Palette) -> El {
    text(s.to_string()).font(font(13.0, 400.0).lh(18.2)).color(p.text_tertiary).wrap()
}

/// `.profileSubSection` / `.pttSettings`: a rule, then the rows 16 apart.
fn ruled(top: f32, rows: El, p: &Palette) -> El {
    col().w_full().mt(top).pt(12.0).gap(16.0).child(el().pin(Some(0.0), Some(0.0), Some(0.0), None).h(1.0).fill(p.background_modifier_accent)).child(rows)
}

/// `.deviceNotice`: no device found.
fn device_notice(title: &str, body: &str, p: &Palette) -> El {
    col()
        .key("device-notice")
        .w_full()
        .padding(12.0)
        .gap(4.0)
        .radius(8.0)
        .fill(p.panel_control_bg)
        .border(1.0, p.panel_control_border)
        .child(text(t(title).to_string()).font(font(14.0, 600.0).lh(20.0)).color(p.text_primary))
        .child(note(t(body), p))
}

/// `WarningAlert`: a bar, the warning icon and the text (and a link under it).
fn warning(text_: String, link: Option<El>, p: &Palette) -> El {
    let c = p.alert_warning_color;
    row()
        .w_full()
        .gap(8.0)
        .items_start()
        .py(6.0)
        .pl(16.0)
        .pr(12.0)
        .role("alert")
        .child(el().pin(Some(0.0), Some(0.0), None, Some(0.0)).w(4.0).radius(4.0).fill(c))
        .child(rune::icon(&crate::icons::WARNING_FILL, 16.0).color(c).mt(2.0))
        .child(col().grow(1.0).min_w(0.0).gap(4.0).child(text(text_).font(font(13.0, 400.0).lh(18.2)).color(p.text_primary).wrap()).children(link))
}

/// `detectExternalAudioProcessor`: audio software that already processes the mic.
fn external_processor(device: &str) -> Option<&'static str> {
    let d = device.to_lowercase().split_whitespace().collect::<String>();
    let has = |a: &[&str]| a.iter().any(|x| d.contains(x));
    [
        ("NVIDIA Broadcast", &["nvidiabroadcast", "rtxvoice"][..]),
        ("Krisp", &["krisp"]),
        ("Elgato Wave Link", &["wavelink", "elgatowave"]),
        ("VoiceMeeter", &["voicemeeter", "vb-audio"]),
        ("SteelSeries Sonar", &["steelseriessonar", "sonar-mic", "sonar-stream", "sonar-chat"]),
        ("Razer Synapse", &["razervirtual", "synapsevirtual"]),
    ]
    .into_iter()
    .find(|(_, pats)| has(pats))
    .map(|(name, _)| name)
}

/// The Voice & video tab body (`VoiceVideoTab`): an Audio section (`UserVoiceTab`) and a Video
/// section (`UserVideoTab`).
pub fn tab<S: 'static>(store: &reflux_store::Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    col().w_full().child(section(t("Audio"), audio(store, st, p, acc, layer), true, p)).child(section(t("Video"), video(st, p, acc, layer), false, p))
}

fn audio<S: 'static>(store: &reflux_store::Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let v = &st.voice;
    let set_ = &v.settings;

    // ---- Input and output
    let devices = match &v.devices {
        None => desc("Looking for devices…", p).key("loading"),
        Some((inputs, outputs)) => {
            let options = |names: &[String]| -> Vec<(String, String)> { std::iter::once((String::new(), "Default".to_string())).chain(names.iter().map(|n| (n.clone(), n.clone()))).collect() };
            let (ins, outs) = (options(inputs), options(outputs));
            let mut pick = |id: &'static str, name: &str, opts: &[(String, String)], value: &Option<String>, output: bool| {
                let refs: Vec<(&str, &str)> = opts.iter().map(|(v, l)| (v.as_str(), l.as_str())).collect();
                // A device that disappeared (or a pre-Pulse cpal name) reads as Default, which
                // is what the engine falls back to.
                let selected = value.as_deref().filter(|v| opts.iter().any(|(id, _)| id == v)).unwrap_or("");
                let select = combo(st, p, acc, layer, id, &refs, selected, WIDE, move |s: &mut S, v| {
                    let sh = acc(s);
                    let v = (!v.is_empty()).then(|| v.to_string());
                    if output { sh.voice.output = v } else { sh.voice.input = v }
                    sh.intents.push(Intent::VoiceDevices);
                });
                combo_row(name, None, select, p)
            };
            let a = pick("voice-input", "Input device", &ins, &v.input, false);
            let b = pick("voice-output", "Output device", &outs, &v.output, true);
            let notice = inputs.is_empty().then(|| device_notice("No microphone found", "Connect a microphone and try again.", p));
            col().key("devices").w_full().gap(8.0).children(notice).child(a).child(b)
        }
    };

    // ---- Volume: 0–200 % with marks every 50 and a reset back to 100 %.
    let vol = |key: &'static str, name: &str, value: f32, input: bool| {
        let put = move |st: &mut Settings, x: f32| if input { st.input_volume = x } else { st.output_volume = x };
        let max = crate::shell::voice::VOICE_VOLUME_MAX_PERCENT;
        let marks = [0.0, max / 4.0, max / 2.0, max * 3.0 / 4.0, max].map(|m: f32| (m, format!("{m}%"))).to_vec();
        col()
            .key(format!("{key}-row"))
            .w_full()
            .child(slider_label(name, reset_icon(format!("{key}-reset"), value != 100.0, set(acc, move |st: &mut Settings| put(st, 100.0)), p), p))
            .child(
                Slider::new(key, value, 0.0, max)
                    .step(1.0)
                    .label(t(name))
                    .marks(marks, 100.0)
                    .tip(format!("{}%", value.round() as i64))
                    .on_change(move |s: &mut S, x| set(acc, move |st: &mut Settings| put(st, x))(s))
                    .el(p),
            )
    };
    let volume = col().w_full().gap(8.0).child(vol("voice-in-vol", "Input volume", set_.input_volume, true)).child(vol("voice-out-vol", "Output volume", set_.output_volume, false));

    // ---- Voice processing: the profile card, and what each profile adds under a rule.
    let mode = set_.processing.as_str();
    let profiles = [
        ("voice", "Focused voice", Some(t("Recommended. Cleans up your mic for clear speech."))),
        ("studio", "Direct input", Some(t("Sends your audio untouched. Best if you're using external audio software."))),
        ("custom", "Custom", Some(t("Adjust noise suppression, echo cancellation, and gain yourself."))),
    ];
    let radio = RadioGroup::new(&profiles, Some(mode))
        .on_change(move |s: &mut S, val| {
            let val = val.to_string();
            set(acc, move |st: &mut Settings| st.processing = val.clone())(s)
        })
        .el(p);
    let agc = || {
        Switch::new(set_.auto_gain_control)
            .label(t("Automatic gain control"))
            .description(t("Evens out your mic volume so you are not too quiet."))
            .on_toggle(set(acc, |st: &mut Settings| st.auto_gain_control ^= true))
            .el(p)
            .key("agc")
    };
    let stereo = || {
        Switch::new(set_.stereo)
            .label(t("Stereo microphone"))
            .description(t("Sends both channels of a stereo microphone. Needs all processing off and a channel at 64 kbps or higher."))
            .on_toggle(set(acc, |st: &mut Settings| st.stereo ^= true))
            .el(p)
            .key("stereo")
    };
    let mut ptt = || ptt_controls(st, p, acc, layer);
    let fallback = super::super::shell::voice::noise_fallback(v);
    let mut card = col().w_full().padding(16.0).radius(8.0).fill(p.background_secondary).border(1.0, p.background_modifier_accent).child(radio);
    match mode {
        "studio" => card = card.child(ruled(12.0, stereo(), p).key("studio")),
        "custom" => {
            let mut rows = col().w_full().gap(16.0).child(ptt()).child(Switch::new(set_.vad_auto).label(t("Auto-set activity threshold")).on_toggle(set(acc, |st: &mut Settings| st.vad_auto ^= true)).el(p).key("vad-auto"));
            if !set_.vad_auto {
                let pct = set_.vad_percent().round();
                rows = rows.child(
                    col()
                        .key("vad")
                        .w_full()
                        .px(4.0)
                        .child(slider_label("Activity threshold", reset_icon("vad-reset".into(), pct != 50.0, set(acc, |st: &mut Settings| st.vad_threshold = 50.0), p), p))
                        .child(Slider::new("vad-threshold", pct, 0.0, 100.0).step(1.0).label(t("Activity threshold")).tip(format!("{pct}%")).on_change(move |s: &mut S, x| set(acc, move |st: &mut Settings| st.vad_threshold = x)(s)).el(p)),
                );
            }
            let opts: Vec<(&str, &str)> = crate::shell::voice::NOISE_BACKENDS.iter().map(|(id, l)| (*id, if *id == "none" { t(l) } else { *l })).collect();
            let noise = combo(st, p, acc, layer, "noise-backend", &opts, &set_.noise_backend, MEDIUM, move |s: &mut S, val| {
                let val = val.to_string();
                set(acc, move |st: &mut Settings| st.noise_backend = val.clone())(s)
            });
            rows = rows
                .child(combo_row("Noise suppression", fallback.as_deref(), noise, p).key("noise"))
                .child(stereo())
                .child(Switch::new(set_.echo_cancellation).label(t("Echo cancellation")).on_toggle(set(acc, |st: &mut Settings| st.echo_cancellation ^= true)).el(p).key("echo"))
                .child(agc());
            card = card.child(ruled(12.0, rows, p).key("custom"));
        }
        _ => {
            if let Some(f) = &fallback {
                card = card.child(note(f, p).mt(12.0).key("fallback"));
            }
            card = card.child(ruled(12.0, col().w_full().gap(16.0).child(ptt()).child(agc()), p).key("voice"));
        }
    }
    let mut processing = col().w_full().gap(16.0);
    let device = v.input.as_deref().or_else(|| v.devices.as_ref().and_then(|(ins, _)| ins.first().map(String::as_str))).unwrap_or("");
    if mode != "studio"
        && let Some(name) = external_processor(device)
    {
        let switch = text(t("Switch to direct input").to_string())
            .font(font(13.0, 600.0).lh(18.2))
            .color(p.text_link)
            .cursor(rune::Cursor::Pointer)
            .role("link")
            .key("to-studio")
            .on_press(set(acc, |st: &mut Settings| st.processing = "studio".into()));
        let msg = crate::i18n::tf("Looks like you're using {0}. Stacking {PRODUCT_NAME}'s processing on top can cause pumping or distortion. Direct input passes your audio through untouched.", &[("0", name), ("PRODUCT_NAME", "Fluxer")]);
        processing = processing.child(warning(msg, Some(switch), p).key("external"));
    }
    processing = processing.child(card);
    if mode == "studio" && set_.push_to_talk && set_.ptt_key.is_some() {
        processing = processing.child(warning(t("Push-to-talk is ignored in direct input. Switch to focused voice or custom to use it.").into(), None, p).key("ptt-ignored"));
    }

    // ---- Mic test: `MicTestSection`'s bar, a level meter then a square play / stop button
    // that plays the processed mic back 0.9 s late (`useMicTest`).
    let test = Button::new(String::new())
        .icon(if v.mic_test { &crate::icons::STOP_FILL } else { &crate::icons::PLAY_FILL })
        .square()
        .variant(if v.mic_test { ButtonVariant::Secondary } else { ButtonVariant::Primary })
        .key("mic-test")
        .on_press({
            let on = !v.mic_test;
            move |s: &mut S| acc(s).intents.push(Intent::Voice(Op::MicTest(on)))
        })
        .el(p)
        .size(40.0, 40.0) // `.actionButton`: 2.5rem
        .label(if v.mic_test { t("Stop mic test") } else { t("Start mic test") });
    let bar = row()
        .w_full()
        .gap(10.0)
        .padding(12.0)
        .center_y()
        .radius(8.0)
        .border(1.0, p.background_modifier_accent)
        .fill(p.background_secondary)
        .child(row().grow(1.0).min_w(0.0).center_y().child(level_meter(if v.mic_test { v.level } else { -100.0 }, p)))
        .child(test);

    let channels = Switch::new(set_.show_connection_ids)
        .label(t("Show connection IDs"))
        .description(t("Show the ID of each voice connection next to people in calls and in voice channels. With it off, each device shows its type."))
        .on_toggle(set(acc, |st: &mut Settings| st.show_connection_ids ^= true))
        .el(p);

    col()
        .w_full()
        .gap(20.0)
        .child(sub("Input and output", devices, p))
        .child(sub("Volume", volume, p))
        .child(sub("Voice processing", processing, p))
        .child(sub("Mic test", bar, p))
        .child(sub("Entrance sound", entrance(store, st, p, acc, layer), p))
        .child(sub("Voice channels", channels, p))
}

/// The push-to-talk switch; on, a rule then the note, the keybind and the release delay.
fn ptt_controls<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let v = &st.voice;
    let set_ = &v.settings;
    let switch = Switch::new(set_.push_to_talk).label(t("Push-to-talk")).on_toggle(set(acc, |st: &mut Settings| st.push_to_talk ^= true)).el(p).key("ptt");
    if !set_.push_to_talk || set_.processing == "studio" {
        return switch;
    }
    // `KeybindRecorder` (`labelPlacement="inline"`): a pencil and the key.
    // shortcut: pressing records in place ("Press a key…") instead of opening Fluxer's recorder
    // popout; the push-to-talk key is Reflux's in-window key, not a synced custom keybind.
    let (key, set_key) = match (&set_.ptt_key, v.recording_key) {
        (_, true) => (t("Press a key…").to_string(), true),
        (Some(k), _) => (k.clone(), true),
        (None, _) => (t("No keybind set").to_string(), false),
    };
    let recorder = row()
        .key("ptt-record")
        .min_h(34.0)
        .max_w(288.0)
        .pr(12.0)
        .center_y()
        .radius(6.0)
        .fill(p.form_surface_background)
        .border(1.0, if v.recording_key { p.background_modifier_accent_focus } else { p.background_modifier_accent })
        // `.recorder:hover`: the border turns accent-focus.
        .hover_group()
        .child(el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).radius(6.0).border(1.0, p.background_modifier_accent_focus).group_opacity(0.0, 1.0).inert())
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(t("Edit keybind"))
        .child(row().w(28.0).center().child(rune::icon(&crate::icons::PENCIL_SIMPLE_BOLD, 12.0).color(p.text_primary_muted)))
        .child(text(key).font(font(13.0, if set_key { 600.0 } else { 400.0 }).lh(18.4)).color(if set_key { p.text_primary } else { p.text_primary_muted }).truncate())
        .on_press(move |s: &mut S| acc(s).voice.recording_key ^= true);
    let keybind = row().w_full().gap(12.0).center_y().child(text(t("Keybind").to_string()).font(font(13.0, 400.0).lh(20.0)).color(p.text_primary).grow(1.0)).child(recorder);
    // `RELEASE_DELAY_OPTIONS`, shown as the nearest one.
    const DELAYS: [u64; 7] = [20, 100, 250, 500, 1000, 1500, 2000];
    let near = DELAYS.into_iter().min_by_key(|d| d.abs_diff(set_.ptt_release_ms)).unwrap_or(20);
    let labels: Vec<(String, String)> = DELAYS.iter().map(|d| (d.to_string(), format!("{}ms", if *d >= 1000 { format!("{},{:03}", d / 1000, d % 1000) } else { d.to_string() }))).collect();
    let opts: Vec<(&str, &str)> = labels.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let delay = combo(st, p, acc, layer, "ptt-release", &opts, &near.to_string(), SMALL, move |s: &mut S, val| {
        let ms = val.parse().unwrap_or(20);
        set(acc, move |st: &mut Settings| st.ptt_release_ms = ms)(s)
    });
    let rows = col()
        .w_full()
        .gap(16.0)
        .child(note(t("While push-to-talk is on, your keybind controls your microphone, so the mute button is disabled."), p))
        // shortcut: Fluxer's system-wide keybind hint and setup alert live in Reflux's Desktop tab
        // ("Configure global shortcuts…"); this line points there.
        .child(col().w_full().gap(8.0).child(keybind).child(note(t("This key works while Reflux has focus. The global Push-to-talk shortcut (set in your desktop's settings) works anywhere."), p)))
        .child(combo_row("Release delay", Some(t("Keep transmitting briefly after release.")), delay, p));
    col().key("ptt-controls").w_full().gap(16.0).child(switch).child(ruled(0.0, rows, p).key("ptt-settings"))
}

/// `UserVideoTab`: the camera group, the screen share group and the advanced settings link.
fn video<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let set_ = &st.voice.settings;
    // `.controlGroup`: rows 6 apart over a rule.
    let group = |rows: El| col().w_full().gap(6.0).pb(12.0).child(rows).child(el().pin(Some(0.0), None, Some(0.0), Some(0.0)).h(1.0).fill(p.background_modifier_accent));

    let cams = st.voice.cameras.clone().unwrap_or_default();
    let cam_opts: Vec<(&str, &str)> = std::iter::once(("", "Default")).chain(cams.iter().map(|n| (n.as_str(), n.as_str()))).collect();
    let cam_ok = crate::camera::available();
    let camera = combo(st, p, acc, layer, "settings-camera", &cam_opts, set_.camera.as_deref().unwrap_or(""), WIDE, move |s: &mut S, val| {
        let val = (!val.is_empty()).then(|| val.to_string());
        set(acc, move |st: &mut Settings| st.camera = val.clone())(s)
    });
    let preview = Button::new(t("Start camera test and configure effects"))
        .key("camera-test")
        .disabled(!cam_ok)
        .on_press(move |s: &mut S| {
            let sh = acc(s);
            sh.voice.modal = Some(Modal::CameraPreview);
            sh.intents.push(Intent::Voice(Op::LoadCameras));
            sh.intents.push(Intent::Voice(Op::Preview(true)));
        })
        .el(p);
    let quality: Vec<(&str, &str)> =
        if st.limits.higher_video_quality { vec![("low", "480p"), ("medium", "720p"), ("high", "1080p")] } else { vec![("low", "480p"), ("medium", "720p")] };
    let cam_q = if quality.iter().any(|(v, _)| *v == set_.camera_quality) { set_.camera_quality.as_str() } else { "medium" };
    let cam_quality = combo(st, p, acc, layer, "camera-quality", &quality, cam_q, SMALL, move |s: &mut S, val| {
        let val = val.to_string();
        set(acc, move |st: &mut Settings| st.camera_quality = val.clone())(s)
    });
    let notice = (!cam_ok || st.voice.cameras.as_ref().is_some_and(Vec::is_empty))
        .then(|| device_notice("No camera found", if cam_ok { "Connect a camera and try again." } else { "Camera needs Reflux built with GStreamer." }, p));
    // `.cameraControlGroup`: the mirror switch and the test button on one row.
    let controls = row()
        .w_full()
        .min_h(44.0)
        .py(4.0)
        .gap(16.0)
        .center_y()
        .child(row().grow(1.0).min_w(0.0).child(Switch::new(set_.mirror_camera).label(t("Mirror camera")).on_toggle(set(acc, |st: &mut Settings| st.mirror_camera ^= true)).el(p)))
        .child(preview);
    let cam_group = group(col().w_full().gap(6.0).child(combo_row("Camera", None, camera, p)).child(controls).child(combo_row("Camera quality", None, cam_quality, p)));

    // ---- Screen share
    let higher = st.limits.higher_video_quality;
    let quality = set_.screen_quality(&st.limits, higher);
    let res = crate::shell::voice::screen_resolutions(higher);
    let res: Vec<(&str, &str)> = res.iter().map(|(v, l)| (v.as_str(), l.as_str())).collect();
    let fps = crate::shell::voice::screen_frame_rates(higher);
    let fps: Vec<(&str, &str)> = fps.iter().map(|(v, l)| (v.as_str(), l.as_str())).collect();
    let res_v = quality.max_height.map_or("source".to_string(), |h| h.to_string());
    let fps_v = quality.fps.to_string();
    let r = combo(st, p, acc, layer, "settings-share-res", &res, &res_v, SMALL, move |s: &mut S, val| {
        let h = val.parse().ok();
        set(acc, move |st: &mut Settings| (st.screen_height, st.streaming_mode) = (h, "custom".into()))(s)
    });
    let f = combo(st, p, acc, layer, "settings-share-fps", &fps, &fps_v, SMALL, move |s: &mut S, val| {
        let n = val.parse().unwrap_or(30);
        set(acc, move |st: &mut Settings| (st.screen_fps, st.streaming_mode) = (n, "custom".into()))(s)
    });
    // `.frameRateNote`: 13px muted, 4 under the select's row.
    let fps_note = |s: String, icon: bool| {
        row()
            .w_full()
            .gap(4.0)
            .mt(-4.0)
            .mb(8.0)
            .center_y()
            .children(icon.then(|| rune::icon(&crate::icons::ATOM_FILL, 14.0).color(p.text_tertiary)))
            .child(text(s).font(font(13.0, 400.0).lh(18.0)).color(p.text_primary_muted).wrap().grow(1.0).min_w(0.0))
    };
    let mut share = col().w_full().gap(6.0).child(combo_row("Screen share quality", None, r, p));
    if !higher && st.premium {
        // `.premiumCard`.
        let upsell = col()
            .key("premium")
            .w_full()
            .padding(14.0)
            .radius(8.0)
            .fill(p.background_secondary)
            .border(1.0, p.background_modifier_accent)
            .child(
                row()
                    .gap(8.0)
                    .mb(8.0)
                    .center_y()
                    .child(rune::icon(&crate::icons::ATOM_FILL, 18.0).color(p.text_primary))
                    .child(text(crate::i18n::tf("Unlock HD screen share with {premiumProductName}", &[("premiumProductName", "Plutonium")])).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary)),
            )
            .child(text(t("Share your screen in high definition (1080p), quad HD (1440p), or native source resolution, at up to 60 FPS.").to_string()).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted).wrap().mb(12.0))
            .child(
                row().child(
                    Button::new(crate::i18n::tf("Get {premiumProductName}", &[("premiumProductName", "Plutonium")]))
                        .variant(ButtonVariant::Secondary)
                        .size(crate::ui::ButtonSize::Small)
                        .key("premium-open")
                        .on_press(move |s: &mut S| crate::settings::open_tab(acc(s), "plutonium"))
                        .el(p),
                ),
            );
        share = share.child(upsell);
    }
    share = share.child(combo_row("Frame rate", None, f, p));
    if !higher && st.premium {
        share = share.child(fps_note(crate::i18n::tf("Frame rates above 30 FPS require {premiumProductName}", &[("premiumProductName", "Plutonium")]), true).key("fps-note"));
    } else if !higher {
        share = share.child(fps_note(t("This instance currently allows screen sharing up to 720p at 30 FPS.").into(), false).key("limit-note"));
    }
    if let Some(preset) = set_.screen_preset() {
        let note = crate::i18n::tf("Set by the {preset} preset. Picking a value here switches to Custom.", &[("preset", t(preset.0))]);
        share = share.child(fps_note(note, false).key("preset"));
    }
    // The bitrate select is part of the higher-quality tier.
    if higher {
        let rates = crate::shell::voice::screen_bitrates(st.limits.screen_share_bitrate, set_.screen_bitrate);
        let rates: Vec<(&str, &str)> = rates.iter().map(|(v, l)| (v.as_str(), l.as_str())).collect();
        let rate_v = set_.screen_bitrate.map_or("auto".to_string(), |b| b.to_string());
        let b = combo(st, p, acc, layer, "settings-share-bitrate", &rates, &rate_v, SMALL, move |s: &mut S, val| {
            let b = val.parse().ok();
            set(acc, move |st: &mut Settings| st.screen_bitrate = b)(s)
        });
        let d = t("With Automatic, the bitrate follows your resolution and frame rate. Higher values keep fast motion sharper and use more upload bandwidth.");
        share = share.child(combo_row("Bitrate", Some(d), b, p).key("bitrate"));
    }

    // `.actionRow`: the encoder path lives in Advanced (`ScreenShareEncoderControls`).
    let advanced = row()
        .key("advanced")
        .w_full()
        .min_h(44.0)
        .gap(16.0)
        .center_y()
        .child(text(t("Advanced video settings").to_string()).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary).grow(1.0).min_w(0.0))
        .child(
            Button::new(t("Open advanced media settings"))
                .variant(ButtonVariant::Secondary)
                .icon(&crate::icons::GEAR_BOLD)
                .key("open-advanced-media")
                .on_press_tx(move |s: &mut S, tx: &mut rune::Tx| {
                    crate::settings::open_tab(acc(s), "advanced_settings");
                    // `scrollToSection('advanced-settings-media')`: its top lands 24 below the content's.
                    tx.scroll_key_to(format!("sec-{}", t("Media")), 24.0);
                })
                .el(p),
        );

    col().w_full().gap(12.0).children(notice).child(cam_group).child(group(share)).child(advanced)
}

/// `EntranceSoundSection`: which scope a sound applies to (everywhere, all communities, DMs, the
/// open community, or another community's override), the sound for it (or the one it
/// inherits), the library with preview / delete, and upload through the trimmer.
fn entrance<S: 'static>(store: &reflux_store::Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let Some(lib) = &st.voice.entrance else {
        return col().key("loading").w_full().gap(8.0).child(desc(t("Loading…"), p)).child(
            Button::new(t("Load")).variant(ButtonVariant::Secondary).key("entrance-load").on_press(move |s: &mut S| acc(s).intents.push(Intent::Voice(Op::LoadEntrance))).el(p),
        );
    };
    let here = st.guild;
    let scope = st.voice.entrance_scope.clone().unwrap_or_else(|| here.map_or("global".into(), |g| format!("guild:{g}")));
    let guild_name = |g: Snowflake| store.guilds.get(&g).and_then(|x| x.info.as_ref()).map_or_else(|| t("Unknown community").to_string(), |i| i.name.clone());
    let label = |s: &str| match s {
        "global" => t("All communities and DMs").to_string(),
        "guilds" => t("All communities").to_string(),
        "dms" => t("Direct messages").to_string(),
        _ => s.strip_prefix("guild:").and_then(|g| g.parse().ok()).map_or_else(|| t("Unknown community").to_string(), |g| guild_name(Snowflake(g))),
    };
    let hint = match scope.as_str() {
        "global" => t("Your fallback entrance sound for all communities and direct messages. More specific settings override it.").to_string(),
        "guilds" => t("Used in communities that do not have their own sound set.").to_string(),
        "dms" => t("Used in direct-message calls. If you do not set one, your global sound is used instead.").to_string(),
        _ => crate::i18n::tf("Used in this community. If you do not set one, {productName} falls back to your community default, then your global default.", &[("productName", "Fluxer")]),
    };
    let tile = |value: String, glyph: Option<&'static rune::Icon>, guild: Option<Snowflake>, title: String, about: Option<String>| {
        let on = value == scope;
        let art = match (glyph, guild) {
            (_, Some(g)) => crate::shell::guild_face(store, st, g, if about.is_some() { 32.0 } else { 24.0 }, false, p),
            (Some(i), _) => row().size(32.0, 32.0).center().radius(8.0).fill(if on { p.brand_primary } else { p.background_modifier_hover }).child(rune::icon(i, 18.0).color(if on { rune::Color::WHITE } else { p.text_primary_muted })),
            _ => row(),
        };
        let v = value.clone();
        row()
            .min_w(0.0)
            .padding(10.0)
            .gap(10.0)
            .center_y()
            .radius(8.0)
            .fill(if on { p.background_modifier_selected } else { p.background_secondary })
            .border(1.0, if on { p.brand_primary } else { p.background_modifier_accent })
            .hover(p.background_modifier_hover)
            .cursor(rune::Cursor::Pointer)
            .role("radio")
            .label(title.clone())
            .key(format!("entrance-scope-{value}"))
            .child(art)
            .child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .gap(2.0)
                    .child(text(title).font(font(14.0, 600.0).lh(18.0)).color(p.text_primary).truncate())
                    .children(about.map(|a| text(a).font(font(12.0, 400.0).lh(16.0)).color(p.text_primary_muted).wrap())),
            )
            .on_press(move |s: &mut S| acc(s).voice.entrance_scope = Some(v.clone()))
    };
    let mut tiles = vec![
        tile("global".into(), Some(&crate::icons::GLOBE_HEMISPHERE_WEST_FILL), None, label("global"), Some(t("Used by default unless a community or DM sound overrides it.").into())),
        tile("guilds".into(), Some(&crate::icons::USERS_THREE_FILL), None, label("guilds"), Some(t("Used in communities that do not have their own sound.").into())),
        tile("dms".into(), Some(&crate::icons::CHAT_CIRCLE_TEXT_FILL), None, label("dms"), Some(t("Used for all direct-message calls.").into())),
    ];
    if let Some(g) = here {
        tiles.push(tile(format!("guild:{g}"), None, Some(g), guild_name(g), Some(t("Used only in this community.").into())));
    }
    let grid = el().w_full().equal_columns(2).gap(8.0).children(tiles);
    // `orderedGuilds` minus the open one, by name.
    let mut others: Vec<(Snowflake, String)> = store.guilds.keys().filter(|g| Some(**g) != here && store.guilds[*g].info.is_some()).map(|g| (*g, guild_name(*g))).collect();
    others.sort_by(|a, b| a.1.cmp(&b.1));
    let mut c = col()
        .key("library")
        .w_full()
        .gap(12.0)
        .child(desc(t("Plays for everyone else in the voice channel when you join."), p))
        .child(col().w_full().gap(4.0).child(text(t("Applies to").to_uppercase()).font(font(12.0, 600.0).lh(16.0)).color(p.text_primary_muted)).child(desc(&hint, p)))
        .child(grid);
    if !others.is_empty() {
        let list = el().w_full().equal_columns(2).gap(6.0).children(others.into_iter().map(|(g, n)| tile(format!("guild:{g}"), None, Some(g), n, None)));
        c = c.child(text(t("Community overrides").to_uppercase()).key("others-title").font(font(12.0, 600.0).lh(16.0)).color(p.text_primary_muted)).child(list.key("others"));
    }
    let pops = crate::ui::popup_access(move |s: &mut S| &mut acc(s).popups);
    let ids: Vec<String> = lib.sounds.iter().map(|x| x.id.to_string()).collect();
    let opts: Vec<(&str, &str)> = std::iter::once(("", t("None"))).chain(lib.sounds.iter().zip(&ids).map(|(x, id)| (id.as_str(), x.name.as_str()))).collect();
    let cur = lib.selections.get(&scope).map(|x| x.to_string()).unwrap_or_default();
    let sc = scope.clone();
    let pick = Select::new("entrance-scope-sound", &opts, Some(&cur))
        .label(t("Sound for this scope"))
        .width(340.0)
        .on_choose(move |s: &mut S, v| acc(s).intents.push(Intent::Voice(Op::SelectEntrance(sc.clone(), v.parse().ok().map(reflux_model::Snowflake)))))
        .el(&st.popups, pops, p, layer);
    c = c.child(pick.key("pick"));
    // The sound this scope plays: set here, or inherited.
    if let Some((id, from)) = lib.resolve_scope(&scope)
        && let Some(x) = lib.sounds.iter().find(|x| x.id == id)
    {
        let badge = if from == scope { t("Set here").to_string() } else { crate::i18n::tf("Inherited from {inheritedFromLabel}", &[("inheritedFromLabel", &label(&from))]) };
        c = c.child(
            row()
                .key("resolved")
                .w_full()
                .padding(10.0)
                .gap(10.0)
                .center_y()
                .radius(8.0)
                .fill(p.background_secondary)
                .child(
                    row()
                        .size(36.0, 36.0)
                        .center()
                        .radius(18.0)
                        .fill(p.background_modifier_hover)
                        .hover(p.background_modifier_selected)
                        .cursor(rune::Cursor::Pointer)
                        .role("button")
                        .label(t("Preview sound"))
                        .key("entrance-preview-resolved")
                        .child(rune::icon(&crate::icons::SPEAKER_HIGH_FILL, 20.0).color(p.text_primary))
                        .on_press(move |s: &mut S| acc(s).intents.push(Intent::Voice(Op::PreviewEntrance(id)))),
                )
                .child(col().grow(1.0).min_w(0.0).child(text(x.name.clone()).font(font(14.0, 600.0).lh(18.0)).color(p.text_primary).truncate()).child(text(format!("{:.1} s", x.duration_ms as f32 / 1000.0)).font(font(12.0, 400.0).lh(16.0)).color(p.text_primary_muted)))
                .child(text(badge).font(font(12.0, 600.0).lh(16.0)).color(p.text_primary_muted)),
        );
    }
    for x in &lib.sounds {
        let id = x.id;
        c = c.child(
            row()
                .key(format!("snd-{id}"))
                .w_full()
                .gap(8.0)
                .center_y()
                .child(text(format!("{} · {:.1} s", x.name, x.duration_ms as f32 / 1000.0)).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary).grow(1.0).truncate())
                .child(Button::new(t("Play")).variant(ButtonVariant::Secondary).key(format!("entrance-play-{id}")).on_press(move |s: &mut S| acc(s).intents.push(Intent::Voice(Op::PreviewEntrance(id)))).el(p))
                .child(Button::new(t("Delete")).variant(ButtonVariant::Danger).key(format!("entrance-del-{id}")).on_press(move |s: &mut S| acc(s).intents.push(Intent::Voice(Op::DeleteEntrance(id)))).el(p)),
        );
    }
    // `ENTRANCE_SOUND_MAX_PER_USER`.
    let full = lib.sounds.len() >= 8;
    let upload = Button::new(t("Upload")).key("entrance-upload").disabled(full).on_press_tx(move |_: &mut S, tx: &mut rune::Tx| {
        let options = rune::FileDialogOptions { title: t("Upload").into(), ..Default::default() }.filter(t("Audio"), &["mp3", "ogg", "wav", "m4a"]);
        tx.open_file(options, move |s: &mut S, r, _| {
            let Ok(Some(path)) = r else { return };
            let sh = acc(s);
            let name = path.file_stem().and_then(|n| n.to_str()).unwrap_or("Entrance sound").chars().take(32).collect();
            match std::fs::read(&path).ok().and_then(crate::voice_input::decode) {
                Some(pcm) => {
                    let mut trim = crate::shell::voice::Trim { name, samples: std::sync::Arc::new(pcm), ..Default::default() };
                    trim.set(false, crate::shell::voice::Trim::MAX_SECS);
                    sh.voice.trim = Some(trim);
                    sh.voice.modal = Some(Modal::Trim);
                }
                None => sh.notice = Some((t("Entrance sound").into(), t("Could not decode this audio file.").into())),
            }
        });
    });
    c = c.child(row().key("upload").child(upload.el(p)));
    if full {
        c = c.child(desc(t("Library full (8 sounds max). Delete one before adding another."), p).key("full"));
    }
    c
}

/// `AudioLevelMeter`: 20 bars rising from 28 % to 100 % high; lit ones run green to red.
/// `level` is dBFS, shown from -60 to 0 like `useMicTest`.
fn level_meter(level: f32, p: &Palette) -> El {
    let lit = ((level + 60.0) / 60.0).clamp(0.0, 1.0);
    let off = p.background_modifier_accent;
    let edge = rune::Color { a: p.text_primary.a * 0.1, ..p.text_primary };
    row().w_full().h(40.0).gap(1.28).items_end().role("meter").label(t("Mic level")).children((0..20).map(|i| {
        let f = (i + 1) as f32 / 20.0;
        let h = 40.0 * (28.0 + 72.0 * f).round().max(22.0) / 100.0;
        // `clip-path: polygon(20% 0, 100% 0, 80% 100%, 0 100%)` between heights `a` and `b`, in a
        // box 1 wide (stretched to the bar) and `h` points tall.
        let band = |a: f32, b: f32| vec![[0.2 - 0.2 * a / h, a], [1.0 - 0.2 * a / h, a], [1.0 - 0.2 * b / h, b], [0.2 - 0.2 * b / h, b]];
        let (top, bottom, ring, bands) = if lit >= f {
            let s = i as f32 / 19.0;
            let hue = 124.0 - 124.0 * s;
            let top = crate::theme::custom::hsl(hue, 94.0, 62.0 - 8.0 * s, 1.0);
            (top, crate::theme::custom::hsl((hue - 8.0).max(0.0), 90.0, 42.0 - 10.0 * s, 1.0), top.alpha(0.5), 16)
        } else {
            (off, off, edge, 1)
        };
        // Shape fills are flat: the gradient is stacked bands, each running half a point into the
        // next so their antialiased edges leave no seam.
        let mut shape = rune::Shape::new();
        for k in 0..bands {
            let (a, b) = (h * k as f32 / bands as f32, h * (k + 1) as f32 / bands as f32);
            shape = shape.fill(band(a, (b + 0.5).min(h)), top.mix(bottom, (k as f32 + 0.5) / bands as f32));
        }
        // The inset 1px ring survives the clip along the top and bottom edges, plus 1px-wide
        // slivers at the bottom-left and top-right tips (left out: the bar's width isn't known
        // here); the clip also hides the outer glow.
        shape = shape.fill(band(0.0, 1.0), ring).fill(band(h - 1.0, h), ring);
        el().grow(1.0).min_w(1.6).h(h).shape(shape.view_box(1.0, h))
    }))
}
