//! Which H.264 encoder does libwebrtc actually use here?
//!
//! Loops a synthetic 1280x720 track between two local peer connections, once per available
//! encoder backend, and prints `encoderImplementation` from the sender's outbound-rtp stats.
//! `Auto` follows LIVEKIT_PREFERRED_HW_ENCODER (read once, when the factory is created):
//!   LIVEKIT_PREFERRED_HW_ENCODER=nvenc cargo run -p reflux-voice --example encode_probe
//!   LIVEKIT_PREFERRED_HW_ENCODER=vaapi cargo run -p reflux-voice --example encode_probe

use std::time::Duration;

use livekit::webrtc::peer_connection_factory::native::PeerConnectionFactoryExt;
use livekit::webrtc::prelude::*;
use livekit::webrtc::stats::RtcStats;
use livekit::webrtc::video_source::native::NativeVideoSource;
use tokio::sync::mpsc;

const W: u32 = 1280;
const H: u32 = 720;

/// libwebrtc's RTC_LOG lines arrive as `log::debug!(target: "libwebrtc")`; print them with
/// PROBE_LOG=1 to see why an encoder was rejected.
struct StderrLog;
impl log::Log for StderrLog {
    fn enabled(&self, _: &log::Metadata) -> bool { true }
    fn log(&self, r: &log::Record) { eprintln!("[{}] {}", r.target(), r.args()) }
    fn flush(&self) {}
}

#[tokio::main]
async fn main() {
    if std::env::var_os("PROBE_LOG").is_some() {
        _ = log::set_logger(&StderrLog).map(|()| log::set_max_level(log::LevelFilter::Debug));
    }
    println!("LIVEKIT_PREFERRED_HW_ENCODER={:?}", std::env::var("LIVEKIT_PREFERRED_HW_ENCODER").ok());
    let available: Vec<_> = VideoEncoderBackend::list_available().into_iter().collect();
    println!("available backends: {available:?}");

    let factory = PeerConnectionFactory::default();
    for backend in [VideoEncoderBackend::Auto, VideoEncoderBackend::Nvenc, VideoEncoderBackend::Vaapi, VideoEncoderBackend::Software] {
        if !available.contains(&backend) {
            println!("{backend:?}: not available");
            continue;
        }
        match probe(&factory, backend).await {
            Ok((imp, frames)) => println!("{backend:?}: encoderImplementation={imp:?} framesEncoded={frames}"),
            Err(e) => println!("{backend:?}: error {e}"),
        }
    }
}

async fn probe(factory: &PeerConnectionFactory, backend: VideoEncoderBackend) -> Result<(String, u32), RtcError> {
    let tx_pc = factory.create_peer_connection(RtcConfiguration::default())?;
    let rx_pc = factory.create_peer_connection(RtcConfiguration::default())?;

    let source = NativeVideoSource::new(VideoResolution { width: W, height: H }, true);
    let track = factory.create_video_track("probe", source.clone());
    let init = RtpTransceiverInit {
        direction: RtpTransceiverDirection::SendOnly,
        stream_ids: vec!["probe".into()],
        send_encodings: vec![],
    };
    let transceiver = tx_pc.add_transceiver(track.into(), init)?;
    let h264 = factory
        .get_rtp_sender_capabilities(MediaType::Video)
        .codecs
        .into_iter()
        // Constrained Baseline only: the NVENC/VAAPI factories advertise just 42e01f, and if
        // Baseline (42001f) wins negotiation, profile matching rejects them and OpenH264 is used.
        .filter(|c| {
            c.mime_type.eq_ignore_ascii_case("video/H264")
                && c.sdp_fmtp_line.as_deref().is_some_and(|f| f.contains("profile-level-id=42e01f"))
        })
        .collect();
    transceiver.set_codec_preferences(h264)?;
    transceiver.sender().set_video_encoder_backend(backend);

    // Candidates are buffered until both descriptions are set, then trickled across.
    let (a_tx, a_rx) = mpsc::unbounded_channel();
    let (b_tx, b_rx) = mpsc::unbounded_channel();
    tx_pc.on_ice_candidate(Some(Box::new(move |c| _ = a_tx.send(c))));
    rx_pc.on_ice_candidate(Some(Box::new(move |c| _ = b_tx.send(c))));

    let offer = tx_pc.create_offer(OfferOptions::default()).await?;
    tx_pc.set_local_description(offer.clone()).await?;
    rx_pc.set_remote_description(offer).await?;
    let answer = rx_pc.create_answer(AnswerOptions::default()).await?;
    rx_pc.set_local_description(answer.clone()).await?;
    tx_pc.set_remote_description(answer).await?;
    let fwd_a = tokio::spawn(trickle(a_rx, rx_pc.clone()));
    let fwd_b = tokio::spawn(trickle(b_rx, tx_pc.clone()));

    // ~3 s of a moving gradient at 30 fps.
    let mut frame = VideoFrame { rotation: VideoRotation::VideoRotation0, timestamp_us: 0, frame_metadata: None, buffer: I420Buffer::new(W, H) };
    for n in 0..90u32 {
        let (y, u, v) = frame.buffer.data_mut();
        for (i, px) in y.iter_mut().enumerate() {
            *px = (i as u32 % W + n * 8) as u8;
        }
        u.fill(128);
        v.fill((n * 2) as u8);
        source.capture_frame(&frame);
        tokio::time::sleep(Duration::from_millis(33)).await;
    }

    let stats = transceiver.sender().get_stats().await?;
    fwd_a.abort();
    fwd_b.abort();
    tx_pc.close();
    rx_pc.close();
    Ok(stats
        .into_iter()
        .find_map(|s| match s {
            RtcStats::OutboundRtp(o) => Some((o.outbound.encoder_implementation, o.outbound.frames_encoded)),
            _ => None,
        })
        .unwrap_or_else(|| ("<no outbound-rtp stats>".into(), 0)))
}

async fn trickle(mut rx: mpsc::UnboundedReceiver<IceCandidate>, pc: PeerConnection) {
    while let Some(c) = rx.recv().await {
        _ = pc.add_ice_candidate(c).await;
    }
}
