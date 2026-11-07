//! The Voice & video tab (`VoiceVideoTab.tsx` = `UserVoiceTab` + `UserVideoTab`): devices,
//! volume, voice processing (profiles, push-to-talk, activity threshold), mic test, voice
//! channel display, camera and screen share quality, encoder preference.

use rune::{El, col, el, row, text};

use super::tabs::section;
use crate::shell::voice::{Modal, Op, Settings};
use crate::shell::{Intent, ShellAccess, ShellState};
use crate::theme::{Palette, dim, font};
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

fn label(s: &str, p: &Palette) -> El {
    text(s.to_string()).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary)
}

/// The Voice & video tab body.
pub fn tab<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let v = &st.voice;
    let set_ = &v.settings;
    let pops = crate::ui::popup_access(move |s: &mut S| &mut acc(s).popups);

    // ---- Devices
    let devices = match &v.devices {
        None => desc("Looking for devices…", p),
        Some((inputs, outputs)) => {
            let options = |names: &[String]| -> Vec<(String, String)> { std::iter::once((String::new(), "Default".to_string())).chain(names.iter().map(|n| (n.clone(), n.clone()))).collect() };
            let (ins, outs) = (options(inputs), options(outputs));
            let mut pick = |id: &str, name: &str, opts: &[(String, String)], value: &Option<String>, output: bool| {
                let refs: Vec<(&str, &str)> = opts.iter().map(|(v, l)| (v.as_str(), l.as_str())).collect();
                Select::new(id, &refs, Some(value.as_deref().unwrap_or("")))
                    .label(name)
                    .width(340.0)
                    .on_choose(move |s: &mut S, v| {
                        let sh = acc(s);
                        let v = (!v.is_empty()).then(|| v.to_string());
                        if output { sh.voice.output = v } else { sh.voice.input = v }
                        sh.intents.push(Intent::VoiceDevices);
                    })
                    .el(&st.popups, pops, p, layer)
            };
            let a = pick("voice-input", t("Input device"), &ins, &v.input, false);
            let b = pick("voice-output", t("Output device"), &outs, &v.output, true);
            col().w_full().gap(16.0).child(a).child(b)
        }
    };
    let mut out = col().w_full().child(section(t("Devices"), devices, true, p));

    // ---- Volume (applied by our own mic path and mixer, `reflux_voice::audio`)
    let vol = |key: &'static str, name: &str, value: f32, input: bool| {
        col().w_full().gap(8.0).child(label(name, p)).child(
            Slider::new(key, value, 0.0, 200.0)
                .step(1.0)
                .on_change(move |s: &mut S, x| {
                    set(acc, move |st: &mut Settings| if input { st.input_volume = x } else { st.output_volume = x })(s);
                })
                .el(p),
        )
    };
    out = out.child(section(t("Volume"), col().w_full().gap(20.0).child(vol("voice-in-vol", t("Input volume"), set_.input_volume, true)).child(vol("voice-out-vol", t("Output volume"), set_.output_volume, false)), false, p));

    // ---- Voice processing
    let profiles = [
        ("voice", "Focused voice", Some(t("Recommended. Cleans up your mic for clear speech."))),
        ("studio", "Direct input", Some(t("Sends your audio untouched. Best if you're using external audio software."))),
        ("custom", "Custom", Some(t("Adjust noise suppression, echo cancellation, and gain yourself."))),
    ];
    let mut processing = col().w_full().gap(20.0).child(
        RadioGroup::new(&profiles, Some(set_.processing.as_str()))
            .on_change(move |s: &mut S, val| {
                let val = val.to_string();
                set(acc, move |st: &mut Settings| st.processing = val.clone())(s)
            })
            .el(p),
    );
    if set_.processing != "studio" {
        processing = processing.child(Switch::new(set_.push_to_talk).label(t("Push-to-talk")).on_toggle(set(acc, |st: &mut Settings| st.push_to_talk ^= true)).el(p));
        if set_.push_to_talk {
            let key = if v.recording_key { "Press a key…".to_string() } else { set_.ptt_key.clone().unwrap_or_else(|| t("No keybind set").into()) };
            let record = Button::new(if v.recording_key { t("Cancel") } else { t("Edit keybind") })
                .variant(ButtonVariant::Secondary)
                .key("ptt-record")
                .on_press(move |s: &mut S| acc(s).voice.recording_key ^= true)
                .el(p);
            processing = processing
                .child(
                    col().w_full().gap(8.0).child(label(t("Keybind"), p)).child(
                        row()
                            .w_full()
                            .gap(12.0)
                            .center_y()
                            .child(row().grow(1.0).h(40.0).px(12.0).center_y().radius(dim::RADIUS_LG).fill(p.background_tertiary).child(text(key).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary)))
                            .child(record),
                    ),
                )
                .child(desc("This key works while Reflux has focus. The global Push-to-talk shortcut (set in your desktop's settings) works anywhere.", p))
                .child(
                    col()
                        .w_full()
                        .gap(8.0)
                        .child(label(t("Release delay"), p))
                        .child(desc(t("Keep transmitting briefly after release."), p))
                        .child(
                            Slider::new("ptt-release", set_.ptt_release_ms as f32, 0.0, 2000.0)
                                .step(20.0)
                                .on_change(move |s: &mut S, x| set(acc, move |st: &mut Settings| st.ptt_release_ms = x as u64)(s))
                                .el(p),
                        )
                        .child(text(format!("{} ms", set_.ptt_release_ms)).font(font(12.0, 500.0).lh(16.0)).color(p.text_tertiary)),
                );
        }
        processing = processing
            .child(
                Switch::new(set_.auto_gain_control)
                    .label(t("Automatic gain control"))
                    .description(t("Evens out your mic volume so you are not too quiet."))
                    .on_toggle(set(acc, |st: &mut Settings| st.auto_gain_control ^= true))
                    .el(p),
            )
            .child(Switch::new(set_.stereo).label(t("Stereo microphone")).on_toggle(set(acc, |st: &mut Settings| st.stereo ^= true)).el(p));
    }
    if !set_.ptt() && set_.processing != "studio" {
        processing = processing.child(Switch::new(set_.vad_auto).label(t("Auto-set activity threshold")).on_toggle(set(acc, |st: &mut Settings| st.vad_auto ^= true)).el(p));
        if !set_.vad_auto {
            processing = processing.child(
                col()
                    .w_full()
                    .gap(8.0)
                    .child(label(t("Activity threshold"), p))
                    .child(meter(v.mic_test, v.level, Some(set_.vad_threshold), p))
                    .child(Slider::new("vad-threshold", set_.vad_threshold, -100.0, 0.0).step(1.0).on_change(move |s: &mut S, x| set(acc, move |st: &mut Settings| st.vad_threshold = x)(s)).el(p)),
            );
        }
    }
    if set_.processing == "custom" {
        processing = processing
            .child(Switch::new(set_.noise_suppression).label(t("Noise suppression")).on_toggle(set(acc, |st: &mut Settings| st.noise_suppression ^= true)).el(p))
            .child(Switch::new(set_.echo_cancellation).label(t("Echo cancellation")).on_toggle(set(acc, |st: &mut Settings| st.echo_cancellation ^= true)).el(p));
    }
    out = out.child(section(t("Voice processing"), processing, false, p));

    // ---- Mic test
    let test = Button::new(if v.mic_test { t("Stop mic test") } else { t("Start mic test") })
        .key("mic-test")
        .on_press({
            let on = !v.mic_test;
            move |s: &mut S| acc(s).intents.push(Intent::Voice(Op::MicTest(on)))
        })
        .el(p);
    let threshold = (!set_.vad_auto && !set_.ptt()).then_some(set_.vad_threshold);
    out = out.child(section(
        "Mic test",
        col().w_full().gap(12.0).child(desc(t("Check how loud your microphone is."), p)).child(row().w_full().gap(16.0).center_y().child(test).child(meter(v.mic_test, v.level, threshold, p))),
        false,
        p,
    ));
    out = out.child(section(t("Entrance sound"), entrance(st, p, acc, layer), false, p));

    // ---- Voice channels
    out = out.child(section(
        "Voice channels",
        Switch::new(set_.show_connection_ids).label(t("Show connection IDs")).on_toggle(set(acc, |st: &mut Settings| st.show_connection_ids ^= true)).el(p),
        false,
        p,
    ));

    // ---- Video
    let cams = v.cameras.clone().unwrap_or_default();
    let cam_opts: Vec<(&str, &str)> = std::iter::once(("", "Default")).chain(cams.iter().map(|n| (n.as_str(), n.as_str()))).collect();
    let cam_ok = crate::gst::available();
    let camera = Select::new("settings-camera", &cam_opts, Some(set_.camera.as_deref().unwrap_or("")))
        .label(t("Camera"))
        .width(340.0)
        .disabled(!cam_ok)
        .on_choose(move |s: &mut S, val| {
            let val = (!val.is_empty()).then(|| val.to_string());
            set(acc, move |st: &mut Settings| st.camera = val.clone())(s)
        })
        .el(&st.popups, pops, p, layer);
    let preview = Button::new(t("Start camera test and configure effects"))
        .variant(ButtonVariant::Secondary)
        .key("camera-test")
        .disabled(!cam_ok)
        .on_press(move |s: &mut S| {
            let sh = acc(s);
            sh.voice.modal = Some(Modal::CameraPreview);
            sh.intents.push(Intent::Voice(Op::LoadCameras));
            sh.intents.push(Intent::Voice(Op::Preview(true)));
        })
        .el(p);
    let quality = [("low", "480p"), ("medium", "720p"), ("high", "1080p")];
    let cam_quality = Select::new("camera-quality", &quality, Some(set_.camera_quality.as_str()))
        .label(t("Camera quality"))
        .width(340.0)
        .on_choose(move |s: &mut S, val| {
            let val = val.to_string();
            set(acc, move |st: &mut Settings| st.camera_quality = val.clone())(s)
        })
        .el(&st.popups, pops, p, layer);
    let mut video = col().w_full().gap(20.0).child(camera);
    if !cam_ok {
        video = video.child(desc("Camera needs Reflux built with GStreamer.", p));
    }
    video = video
        .child(Switch::new(set_.mirror_camera).label(t("Mirror camera")).on_toggle(set(acc, |st: &mut Settings| st.mirror_camera ^= true)).el(p))
        .child(row().child(preview))
        .child(cam_quality);
    out = out.child(section(t("Video"), video, false, p));

    // ---- Screen share
    let res = [("720", "720p"), ("1080", "1080p"), ("1440", "1440p"), ("source", "Source")];
    let fps = [("15", "15 FPS"), ("30", "30 FPS"), ("60", "60 FPS")];
    let rates = [("auto", "Automatic"), ("2500000", "2.5 Mbps"), ("5000000", "5 Mbps"), ("8000000", "8 Mbps"), ("12000000", "12 Mbps")];
    let res_v = set_.screen_height.map_or("source".to_string(), |h| h.to_string());
    let fps_v = set_.screen_fps.to_string();
    let rate_v = set_.screen_bitrate.map_or("auto".to_string(), |b| b.to_string());
    let r = Select::new("settings-share-res", &res, Some(&res_v))
        .label(t("Screen share quality"))
        .width(220.0)
        .on_choose(move |s: &mut S, val| {
            let h = val.parse().ok();
            set(acc, move |st: &mut Settings| st.screen_height = h)(s)
        })
        .el(&st.popups, pops, p, layer);
    let f = Select::new("settings-share-fps", &fps, Some(&fps_v))
        .label(t("Frame rate"))
        .width(220.0)
        .on_choose(move |s: &mut S, val| {
            let n = val.parse().unwrap_or(30);
            set(acc, move |st: &mut Settings| st.screen_fps = n)(s)
        })
        .el(&st.popups, pops, p, layer);
    let b = Select::new("settings-share-bitrate", &rates, Some(&rate_v))
        .label(t("Bitrate"))
        .width(220.0)
        .on_choose(move |s: &mut S, val| {
            let b = val.parse().ok();
            set(acc, move |st: &mut Settings| st.screen_bitrate = b)(s)
        })
        .el(&st.popups, pops, p, layer);
    let share = col()
        .w_full()
        .gap(20.0)
        .child(row().w_full().gap(16.0).child(r).child(f))
        .child(b)
        .child(desc(t("With Automatic, the bitrate follows your resolution and frame rate. Higher values keep fast motion sharper and use more upload bandwidth."), p));
    out = out.child(section(t("Screen share"), share, false, p));

    // ---- Encoder
    let encoders = [
        ("auto", "Automatic", Some("Use a hardware encoder when one is available.")),
        ("nvenc", "NVIDIA (NVENC)", None),
        ("vaapi", "VA-API", None),
        ("software", "Software", Some("Encode on the CPU. Slower, but works everywhere.")),
    ];
    out = out.child(section(
        "Video encoder",
        RadioGroup::new(&encoders, Some(set_.encoder.as_str()))
            .on_change(move |s: &mut S, val| {
                let val = val.to_string();
                set(acc, move |st: &mut Settings| st.encoder = val.clone())(s)
            })
            .el(p),
        false,
        p,
    ));
    out
}

/// `EntranceSoundSection`: the clip for every voice channel and call (the `global` scope), the
/// library with preview / delete, and upload through the trimmer.
fn entrance<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let Some(lib) = &st.voice.entrance else {
        return col().w_full().gap(8.0).child(desc(t("Loading…"), p)).child(
            Button::new(t("Load")).variant(ButtonVariant::Secondary).key("entrance-load").on_press(move |s: &mut S| acc(s).intents.push(Intent::Voice(Op::LoadEntrance))).el(p),
        );
    };
    let pops = crate::ui::popup_access(move |s: &mut S| &mut acc(s).popups);
    let ids: Vec<String> = lib.sounds.iter().map(|x| x.id.to_string()).collect();
    let opts: Vec<(&str, &str)> = std::iter::once(("", t("None"))).chain(lib.sounds.iter().zip(&ids).map(|(x, id)| (id.as_str(), x.name.as_str()))).collect();
    let cur = lib.selections.get("global").map(|x| x.to_string()).unwrap_or_default();
    let pick = Select::new("entrance-global", &opts, Some(&cur))
        .label(t("Entrance sound"))
        .width(340.0)
        .on_choose(move |s: &mut S, v| acc(s).intents.push(Intent::Voice(Op::SelectEntrance("global".into(), v.parse().ok().map(reflux_model::Snowflake)))))
        .el(&st.popups, pops, p, layer);
    let mut c = col().w_full().gap(12.0).child(desc(t("Plays for everyone else in the voice channel when you join."), p)).child(pick);
    for x in &lib.sounds {
        let id = x.id;
        c = c.child(
            row()
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
    c = c.child(row().child(upload.el(p)));
    if full {
        c = c.child(desc(t("Library full (8 sounds max). Delete one before adding another."), p));
    }
    c
}

/// The mic level bar (-100..0 dB), with the activity threshold marked.
fn meter(on: bool, level: f32, threshold: Option<f32>, p: &Palette) -> El {
    let frac = |db: f32| ((db + 100.0) / 100.0).clamp(0.0, 1.0);
    let lvl = if on { frac(level) } else { 0.0 };
    let over = threshold.is_none_or(|t| level > t);
    let mut bar = el()
        .grow(1.0)
        .min_w(120.0)
        .h(8.0)
        .radius(4.0)
        .fill(p.background_tertiary)
        .role("meter")
        .label("Mic level")
        .child(el().pin(Some(0.0), Some(0.0), None, Some(0.0)).w_frac(lvl).radius(4.0).fill(if over { p.status_online } else { p.status_idle }));
    if let Some(t) = threshold {
        bar = bar.child(row().pin(Some(-2.0), Some(0.0), Some(0.0), Some(-2.0)).inert().child(el().w_frac(frac(t))).child(el().w(2.0).h_full().fill(p.text_primary)));
    }
    bar
}
