//! GStreamer media: video attachment playback (Rune's `Player` decodes only on macOS) and camera
//! capture. Both hand straight RGBA frames to the same `Pixels` path as the stream viewer.
//! Built with the `gst` feature on Linux; elsewhere [`available`] is false and callers keep their
//! fallbacks ("Open in browser", the camera button disabled).

#[cfg(all(feature = "gst", target_os = "linux"))]
pub use imp::*;
#[cfg(not(all(feature = "gst", target_os = "linux")))]
pub use stub::*;

/// Copies `h` rows of `w * 4` bytes out of a buffer whose rows are `stride` apart.
#[cfg_attr(not(all(feature = "gst", target_os = "linux")), allow(dead_code))]
fn tight(data: &[u8], w: usize, h: usize, stride: usize) -> Vec<u8> {
    if stride == w * 4 {
        return data[..w * h * 4].to_vec();
    }
    let mut out = Vec::with_capacity(w * h * 4);
    for r in 0..h {
        out.extend_from_slice(&data[r * stride..r * stride + w * 4]);
    }
    out
}

/// Applications playing audio, from `pw-dump`'s JSON: (`object.serial`, name), without our
/// own process (its audio is the call).
#[cfg_attr(not(all(feature = "gst", target_os = "linux")), allow(dead_code))]
fn parse_apps(json: &str, own_pid: u32) -> Vec<(String, String)> {
    let Ok(serde_json::Value::Array(objects)) = serde_json::from_str(json) else { return Vec::new() };
    let mut out: Vec<(String, String)> = objects
        .iter()
        .filter_map(|o| {
            let props = &o["info"]["props"];
            (props["media.class"] == "Stream/Output/Audio").then_some(())?;
            if props["application.process.id"].as_u64() == Some(own_pid as u64) || props["application.process.id"].as_str() == Some(&own_pid.to_string()) {
                return None;
            }
            let serial = match &props["object.serial"] {
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::String(s) => s.clone(),
                _ => return None,
            };
            let app = props["application.name"].as_str().or(props["node.name"].as_str()).unwrap_or("Unknown app");
            let title = props["media.name"].as_str().filter(|m| !m.is_empty() && *m != app);
            Some((serial, title.map_or(app.to_string(), |t| format!("{app} — {t}"))))
        })
        .collect();
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

/// Mirrors an RGBA image left to right (the camera self-view, `mirrorCamera`).
pub fn mirror(rgba: &mut [u8], w: usize) {
    for row in rgba.chunks_exact_mut(w * 4) {
        for x in 0..w / 2 {
            for c in 0..4 {
                row.swap(x * 4 + c, (w - 1 - x) * 4 + c);
            }
        }
    }
}

#[cfg(all(feature = "gst", target_os = "linux"))]
mod imp {
    use std::sync::OnceLock;

    use gstreamer as gst;
    use gstreamer::prelude::*;
    use gstreamer_app as gst_app;
    use gstreamer_video as gst_video;

    fn init() -> bool {
        static OK: OnceLock<bool> = OnceLock::new();
        *OK.get_or_init(|| gst::init().map_err(|e| eprintln!("reflux: gstreamer: {e}")).is_ok())
    }

    pub fn available() -> bool {
        init()
    }

    /// An RGBA appsink that hands every frame to `sink`.
    fn rgba_sink(caps: gst::Caps, mut sink: impl FnMut(u32, u32, Vec<u8>) + Send + 'static) -> gst_app::AppSink {
        gst_app::AppSink::builder()
            .caps(&caps)
            .max_buffers(1)
            .drop(true)
            .callbacks(
                gst_app::AppSinkCallbacks::builder()
                    .new_sample(move |s| {
                        let sample = s.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                        let (Some(buffer), Some(caps)) = (sample.buffer(), sample.caps()) else { return Ok(gst::FlowSuccess::Ok) };
                        let Ok(info) = gst_video::VideoInfo::from_caps(caps) else { return Ok(gst::FlowSuccess::Ok) };
                        let Ok(map) = buffer.map_readable() else { return Ok(gst::FlowSuccess::Ok) };
                        let (w, h) = (info.width() as usize, info.height() as usize);
                        let stride = info.stride()[0] as usize;
                        sink(w as u32, h as u32, super::tight(&map, w, h, stride));
                        Ok(gst::FlowSuccess::Ok)
                    })
                    .build(),
            )
            .build()
    }

    /// One video clip: decoded by `playbin`, frames into `pixels`, sound to the default output.
    pub struct VideoPlayer {
        playbin: gst::Element,
        pub ended: bool,
        pub playing: bool,
    }

    impl VideoPlayer {
        pub fn new(url: &str, pixels: rune::Pixels) -> Option<Self> {
            if !init() {
                return None;
            }
            let caps = gst::Caps::builder("video/x-raw").field("format", "RGBA").build();
            let sink = rgba_sink(caps, move |w, h, rgba| pixels.set(w, h, rgba));
            let playbin = gst::ElementFactory::make("playbin").property("uri", url).property("video-sink", &sink).build().ok()?;
            Some(Self { playbin, ended: false, playing: false })
        }

        pub fn play(&mut self) {
            if self.ended {
                self.seek(0.0);
                self.ended = false;
            }
            self.playing = self.playbin.set_state(gst::State::Playing).is_ok();
        }

        pub fn pause(&mut self) {
            let _ = self.playbin.set_state(gst::State::Paused);
            self.playing = false;
        }

        pub fn seek(&self, secs: f64) {
            let _ = self.playbin.seek_simple(gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT, gst::ClockTime::from_mseconds((secs.max(0.0) * 1000.0) as u64));
        }

        /// `volume` 0..1 (cubic, like the web player's slider), `muted` on top.
        pub fn set_volume(&self, volume: f64, muted: bool) {
            self.playbin.set_property("volume", volume.clamp(0.0, 1.0).powi(3));
            self.playbin.set_property("mute", muted);
        }

        pub fn position(&self) -> Option<f64> {
            self.playbin.query_position::<gst::ClockTime>().map(|t| t.mseconds() as f64 / 1000.0)
        }

        pub fn duration(&self) -> Option<f64> {
            self.playbin.query_duration::<gst::ClockTime>().map(|t| t.mseconds() as f64 / 1000.0)
        }

        /// Drains the bus; true when something changed (the clip ended or failed).
        pub fn poll(&mut self) -> bool {
            let Some(bus) = self.playbin.bus() else { return false };
            let mut changed = false;
            while let Some(msg) = bus.pop() {
                match msg.view() {
                    gst::MessageView::Eos(_) => {
                        (self.ended, self.playing, changed) = (true, false, true);
                        let _ = self.playbin.set_state(gst::State::Paused);
                    }
                    gst::MessageView::Error(e) => {
                        eprintln!("reflux: video: {} ({:?})", e.error(), e.debug());
                        (self.playing, changed) = (false, true);
                    }
                    _ => {}
                }
            }
            changed
        }
    }

    impl Drop for VideoPlayer {
        fn drop(&mut self) {
            let _ = self.playbin.set_state(gst::State::Null);
        }
    }

    /// Cameras GStreamer's device monitor sees (PipeWire or V4L2), by display name.
    pub fn cameras() -> Vec<String> {
        if !init() {
            return Vec::new();
        }
        let monitor = gst::DeviceMonitor::new();
        monitor.add_filter(Some("Video/Source"), None);
        let mut names: Vec<String> = monitor.devices().iter().map(|d| d.display_name().to_string()).collect();
        names.dedup();
        names
    }

    /// A running camera: RGBA frames at `width`×`height` (letterboxed) and up to 30 fps.
    pub struct CameraCapture {
        pipeline: gst::Pipeline,
    }

    impl CameraCapture {
        /// `device`: a name from [`cameras`] (`None` = the first one).
        pub fn start(device: Option<&str>, width: u32, height: u32, sink: impl FnMut(u32, u32, Vec<u8>) + Send + 'static) -> Option<Self> {
            if !init() {
                return None;
            }
            let monitor = gst::DeviceMonitor::new();
            monitor.add_filter(Some("Video/Source"), None);
            let devices = monitor.devices();
            let dev = devices.iter().find(|d| device.is_none_or(|n| d.display_name() == n)).or(devices.front())?;
            let src = dev.create_element(None).ok()?;
            let convert = gst::ElementFactory::make("videoconvert").build().ok()?;
            let scale = gst::ElementFactory::make("videoscale").build().ok()?;
            let rate = gst::ElementFactory::make("videorate").property("drop-only", true).build().ok()?;
            let caps = gst::Caps::builder("video/x-raw")
                .field("format", "RGBA")
                .field("width", width as i32)
                .field("height", height as i32)
                .field("pixel-aspect-ratio", gst::Fraction::new(1, 1))
                .field("framerate", gst::Fraction::new(30, 1))
                .build();
            let appsink = rgba_sink(caps, sink);
            let pipeline = gst::Pipeline::new();
            pipeline.add_many([&src, &convert, &scale, &rate, appsink.upcast_ref()]).ok()?;
            gst::Element::link_many([&src, &convert, &scale, &rate, appsink.upcast_ref()]).ok()?;
            pipeline.set_state(gst::State::Playing).map_err(|e| eprintln!("reflux: camera: {e}")).ok()?;
            Some(Self { pipeline })
        }
    }

    impl Drop for CameraCapture {
        fn drop(&mut self) {
            let _ = self.pipeline.set_state(gst::State::Null);
        }
    }

    /// The applications playing audio right now (PipeWire), for screen share audio.
    pub fn audio_apps() -> Vec<(String, String)> {
        let out = std::process::Command::new("pw-dump").output();
        match out {
            Ok(o) if o.status.success() => super::parse_apps(&String::from_utf8_lossy(&o.stdout), std::process::id()),
            _ => Vec::new(),
        }
    }

    /// Captures one PipeWire node's audio as interleaved stereo f32 at 48 kHz.
    pub struct AudioCapture {
        pipeline: gst::Pipeline,
    }

    impl AudioCapture {
        /// `node`: an `object.serial` from [`audio_apps`].
        pub fn start(node: &str, mut sink: impl FnMut(&[f32]) + Send + 'static) -> Option<Self> {
            if !init() {
                return None;
            }
            let src = gst::ElementFactory::make("pipewiresrc").property_from_str("target-object", node).build().ok()?;
            let convert = gst::ElementFactory::make("audioconvert").build().ok()?;
            let resample = gst::ElementFactory::make("audioresample").build().ok()?;
            let caps = gst::Caps::builder("audio/x-raw").field("format", "F32LE").field("rate", 48_000i32).field("channels", 2i32).field("layout", "interleaved").build();
            let appsink = gst_app::AppSink::builder()
                .caps(&caps)
                .callbacks(
                    gst_app::AppSinkCallbacks::builder()
                        .new_sample(move |s| {
                            let sample = s.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                            if let Some(map) = sample.buffer().and_then(|b| b.map_readable().ok()) {
                                let pcm: Vec<f32> = map.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect();
                                sink(&pcm);
                            }
                            Ok(gst::FlowSuccess::Ok)
                        })
                        .build(),
                )
                .build();
            let pipeline = gst::Pipeline::new();
            pipeline.add_many([&src, &convert, &resample, appsink.upcast_ref()]).ok()?;
            gst::Element::link_many([&src, &convert, &resample, appsink.upcast_ref()]).ok()?;
            pipeline.set_state(gst::State::Playing).map_err(|e| eprintln!("reflux: stream audio: {e}")).ok()?;
            Some(Self { pipeline })
        }
    }

    impl Drop for AudioCapture {
        fn drop(&mut self) {
            let _ = self.pipeline.set_state(gst::State::Null);
        }
    }
}

#[cfg(not(all(feature = "gst", target_os = "linux")))]
mod stub {
    pub fn available() -> bool {
        false
    }

    pub struct VideoPlayer {
        pub ended: bool,
        pub playing: bool,
    }

    impl VideoPlayer {
        pub fn new(_url: &str, _pixels: rune::Pixels) -> Option<Self> {
            None
        }
        pub fn play(&mut self) {}
        pub fn pause(&mut self) {}
        pub fn seek(&self, _secs: f64) {}
        pub fn set_volume(&self, _volume: f64, _muted: bool) {}
        pub fn position(&self) -> Option<f64> {
            None
        }
        pub fn duration(&self) -> Option<f64> {
            None
        }
        pub fn poll(&mut self) -> bool {
            false
        }
    }

    pub fn cameras() -> Vec<String> {
        Vec::new()
    }

    pub struct CameraCapture;

    impl CameraCapture {
        pub fn start(_device: Option<&str>, _width: u32, _height: u32, _sink: impl FnMut(u32, u32, Vec<u8>) + Send + 'static) -> Option<Self> {
            None
        }
    }

    pub fn audio_apps() -> Vec<(String, String)> {
        Vec::new()
    }

    pub struct AudioCapture;

    impl AudioCapture {
        pub fn start(_node: &str, _sink: impl FnMut(&[f32]) + Send + 'static) -> Option<Self> {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn lists_audio_apps_but_not_ours() {
        let json = r#"[
            {"info":{"props":{"media.class":"Stream/Output/Audio","object.serial":42,"application.name":"Firefox","media.name":"Video","application.process.id":10}}},
            {"info":{"props":{"media.class":"Stream/Output/Audio","object.serial":43,"application.name":"reflux","application.process.id":7}}},
            {"info":{"props":{"media.class":"Audio/Sink","object.serial":1}}},
            {"type":"PipeWire:Interface:Client"}
        ]"#;
        assert_eq!(super::parse_apps(json, 7), vec![("42".to_string(), "Firefox — Video".to_string())]);
    }

    #[test]
    fn tight_and_mirror() {
        // 2×2 with 4 bytes of row padding.
        let data = [1, 1, 1, 1, 2, 2, 2, 2, 0, 0, 0, 0, 3, 3, 3, 3, 4, 4, 4, 4, 0, 0, 0, 0];
        let mut t = super::tight(&data, 2, 2, 12);
        assert_eq!(t, [1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4]);
        super::mirror(&mut t, 2);
        assert_eq!(t, [2, 2, 2, 2, 1, 1, 1, 1, 4, 4, 4, 4, 3, 3, 3, 3]);
    }
}
