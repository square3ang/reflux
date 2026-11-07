use std::sync::Arc;
use std::time::Duration;

use futures_util::{FutureExt, StreamExt};
use livekit::webrtc::audio_source::native::NativeAudioSource;
use livekit::webrtc::audio_stream::native::NativeAudioStream;
use livekit::webrtc::peer_connection_factory::native::PeerConnectionFactoryExt;
use livekit::webrtc::video_source::native::NativeVideoSource;
use livekit::webrtc::video_stream::native::NativeVideoStream;
use serde_json::json;
use tokio::sync::mpsc::UnboundedReceiver;

use super::*;

#[test]
fn signals_match_fluxers_wire_format() {
    let need = Signal::NeedOffer { g: 0, restart: false };
    assert_eq!(serde_json::to_value(&need).unwrap(), json!({"t": "need-offer", "g": 0}));
    assert_eq!(Signal::parse(&json!({"t": "need-offer", "g": 3, "restart": true})), Some(Signal::NeedOffer { g: 3, restart: true }));
    assert_eq!(Signal::parse(&json!({"t": "need-offer", "g": 3, "restart": false})), None, "restart is absent or true");
    let tracks = Signal::Tracks { v: 7, tracks: vec![TrackInfo { src: Src::ScreenShare, sid: track_sid("c1", Src::ScreenShare), m: false, w: Some(1920.0), h: None }] };
    assert_eq!(serde_json::to_value(&tracks).unwrap(), json!({"t": "tracks", "v": 7, "tracks": [{"src": "screen_share", "sid": "TR_c1_screen_share", "m": false, "w": 1920.0}]}));
    let ice = json!({"t": "ice", "g": 5, "c": {"candidate": "candidate:1 1 udp 1 1.2.3.4 5 typ host", "sdpMid": "0", "sdpMLineIndex": 0, "usernameFragment": "u"}});
    assert!(matches!(Signal::parse(&ice), Some(Signal::Ice { g: 5, c: Some(Candidate { sdp_mline_index: Some(0), .. }) })));
    assert_eq!(Signal::parse(&json!({"t": "ice", "g": 5, "c": null})), Some(Signal::Ice { g: 5, c: None }));
    assert_eq!(Signal::parse(&json!({"t": "offer", "g": -1, "sdp": ""})), None, "counters are non-negative");
    assert_eq!(Signal::parse(&json!({"t": "bogus"})), None);
    assert_eq!(Src::of_sid("TR_abc_screen_share_audio"), Some(Src::ScreenShareAudio));
    assert_eq!(Src::of_sid("TR_abc_screen_share"), Some(Src::ScreenShare));
}

/// One side of the loopback: its mesh, events, and the local sources it publishes.
struct Side {
    mesh: Arc<Mesh>,
    user: u64,
    mic: NativeAudioSource,
    camera: NativeVideoSource,
}

fn side(conn: &str, user: u64) -> (Side, UnboundedReceiver<Event>) {
    let target = Target { guild_id: None, channel_id: 50, connection_id: conn.into(), ice_servers: Vec::new() };
    let (mesh, events) = Mesh::new(target, tokio::runtime::Handle::current());
    let factory = LkRuntime::instance();
    let mic = NativeAudioSource::new(Default::default(), 48_000, 1, 0);
    let camera = NativeVideoSource::new(VideoResolution { width: 64, height: 64 }, false);
    let mic_track = factory.pc_factory().create_audio_track("mic", mic.clone());
    let cam_track = factory.pc_factory().create_video_track("cam", camera.clone());
    mesh.publish(Src::Microphone, Some(mic_track.into()), false, (0, 0), None);
    mesh.publish(Src::Camera, Some(cam_track.into()), false, (64, 64), None);
    (Side { mesh: Arc::new(mesh), user, mic, camera }, events)
}

/// Feeds a tone into the mic and grey frames into the camera until the test ends.
fn feed(s: &Side) {
    let (mic, camera) = (s.mic.clone(), s.camera.clone());
    tokio::spawn(async move {
        let mut n = 0u64;
        let mut buf = I420Buffer::new(64, 64);
        let (y, _, _) = buf.data_mut();
        y.fill(200);
        loop {
            let data: Vec<i16> = (0..480).map(|_| (crate::audio::test_tone({ n += 1; n }) * 32767.0) as i16).collect();
            let frame = AudioFrame { data: data.into(), sample_rate: 48_000, num_channels: 1, samples_per_channel: 480 };
            let _ = mic.capture_frame(&frame).now_or_never();
            if n.is_multiple_of(4800) {
                camera.capture_frame(&VideoFrame { rotation: VideoRotation::VideoRotation0, timestamp_us: 0, frame_metadata: None, buffer: &buf });
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    });
}

#[derive(Default, Debug)]
struct Seen {
    connected: bool,
    failed: bool,
    mic: Option<RtcAudioTrack>,
    camera: Option<RtcVideoTrack>,
    reports: Vec<Value>,
}

/// The in-memory gateway: relays each side's signals to the other (as JSON text, like the
/// wire), and records what each side saw. `drop_from_b` simulates a peer that never answers.
async fn route(a: &Side, mut ea: UnboundedReceiver<Event>, b: &Side, mut eb: UnboundedReceiver<Event>, until: impl Fn(&Seen, &Seen) -> bool, drop_from_b: bool) -> (Seen, Seen) {
    let (mut sa, mut sb) = (Seen::default(), Seen::default());
    let deadline = tokio::time::Instant::now() + Duration::from_secs(25);
    let relay = |to: &Side, from: &Side, from_conn: &str, data: Value| {
        let wire = serde_json::to_string(&data).unwrap();
        to.mesh.signal(from_conn, from.user, &serde_json::from_str(&wire).unwrap());
    };
    let record = |seen: &mut Seen, e: Event| match e {
        Event::Status { status: Status::Connected, .. } => seen.connected = true,
        Event::Status { status: Status::Failed, .. } => seen.failed = true,
        Event::Remote { src: Src::Microphone, track: Some(MediaStreamTrack::Audio(t)), .. } => seen.mic = Some(t),
        Event::Remote { src: Src::Camera, track: Some(MediaStreamTrack::Video(t)), .. } => seen.camera = Some(t),
        Event::Report(r) => seen.reports.push(r),
        _ => {}
    };
    while !until(&sa, &sb) {
        tokio::select! {
            Some(e) = ea.recv() => match e {
                Event::Signal { to, data } => { assert_eq!(to, "b"); relay(b, a, "a", data) }
                e => record(&mut sa, e),
            },
            Some(e) = eb.recv() => match e {
                Event::Signal { to, data } => { assert_eq!(to, "a"); if !drop_from_b { relay(a, b, "b", data) } }
                e => record(&mut sb, e),
            },
            _ = tokio::time::sleep_until(deadline) => panic!("timed out: a = {sa:?}, b = {sb:?}"),
        }
    }
    (sa, sb)
}

async fn loud(track: RtcAudioTrack) -> bool {
    let mut stream = NativeAudioStream::new(track, 48_000, 1);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while let Ok(Some(frame)) = tokio::time::timeout_at(deadline, stream.next()).await {
        if frame.data.iter().any(|&s| s.unsigned_abs() > 1000) {
            return true;
        }
    }
    false
}

async fn has_frame(track: RtcVideoTrack) -> bool {
    let mut stream = NativeVideoStream::new(track);
    matches!(tokio::time::timeout(Duration::from_secs(10), stream.next()).await, Ok(Some(f)) if f.buffer.width() == 64)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_peers_connect_and_exchange_audio_and_video() {
    let (a, ea) = side("a", 1);
    let (b, eb) = side("b", 2);
    feed(&a);
    feed(&b);
    // "a" < "b": a offers. b learns about a late, so a's first signals wait in b's inbox.
    a.mesh.set_roster(&[("b".into(), 2)]);
    let b_mesh = b.mesh.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        b_mesh.set_roster(&[("a".into(), 1)]);
    });
    let done = |x: &Seen| x.connected && x.mic.is_some() && x.camera.is_some();
    let (sa, sb) = route(&a, ea, &b, eb, |x, y| done(x) && done(y), false).await;
    assert!(!sa.failed && !sb.failed);
    // Both directions carry media: the answerer's transceivers are sendrecv too.
    let (a_hears, b_hears) = tokio::join!(loud(sa.mic.clone().unwrap()), loud(sb.mic.clone().unwrap()));
    assert!(a_hears && b_hears, "audio: a hears b {a_hears}, b hears a {b_hears}");
    let (a_sees, b_sees) = tokio::join!(has_frame(sa.camera.clone().unwrap()), has_frame(sb.camera.clone().unwrap()));
    assert!(a_sees && b_sees, "video: a sees b {a_sees}, b sees a {b_sees}");
    assert!(a.mesh.remote_video(2, Src::Camera).is_some());
    assert!(a.mesh.remote_video(2, Src::ScreenShare).is_none(), "nothing wanted that isn't published");
    assert!(a.mesh.rtt_ms().await.is_some());
    a.mesh.close();
    b.mesh.close();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_peer_that_never_answers_fails_after_the_setup_deadline() {
    let (a, ea) = side("a", 1);
    let (b, eb) = side("b", 2);
    a.mesh.set_roster(&[("b".into(), 2)]);
    b.mesh.set_roster(&[("a".into(), 1)]);
    let started = tokio::time::Instant::now();
    let (sa, _) = route(&a, ea, &b, eb, |x, _| x.failed && !x.reports.is_empty(), true).await;
    assert!(started.elapsed() >= SETUP_DEADLINE - Duration::from_millis(500));
    assert!(!sa.connected);
    let r = &sa.reports[0];
    assert_eq!((r["outcome"].as_str(), r["participant_count"].as_u64(), r["channel_id"].as_str()), (Some("failed"), Some(2), Some("50")));
}
