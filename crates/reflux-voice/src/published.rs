//! What actually left this client for each published track, and what the SFU said about it,
//! for the live voice test (`reflux --live=voice`).

use livekit::webrtc::stats::RtcStats;

use crate::Call;

/// One of our published tracks, joined with its WebRTC send stats.
#[derive(Debug, Default)]
pub struct Published {
    /// `Camera`, `Screenshare`, `ScreenshareAudio`, `Microphone`.
    pub source: String,
    /// Assigned by the server when it accepted the publish (`TR_…`).
    pub sid: String,
    pub encoder: String,
    pub frames_encoded: u32,
    pub packets_sent: u64,
    pub bytes_sent: u64,
    /// RTCP receiver reports the SFU sent back about this stream (it receives it).
    pub sfu_reports: u64,
    pub sfu_lost: i64,
}

impl Call {
    /// Our server-acknowledged publications and the RTP each one has sent (LiveKit rooms only).
    pub async fn published(&self) -> Vec<Published> {
        let Some(room) = self.room() else { return Vec::new() };
        let Ok(stats) = room.get_stats().await else { return Vec::new() };
        let all: Vec<&RtcStats> = stats.publisher_stats.iter().collect();
        room.local_participant()
            .track_publications()
            .values()
            .map(|p| {
                let mut out = Published { source: format!("{:?}", p.source()), sid: p.sid().to_string(), ..Default::default() };
                let Some(track) = p.track() else { return out };
                let id = track.rtc_track().id();
                // track → media-source → outbound-rtp → the SFU's remote-inbound-rtp.
                let sources: Vec<&str> = all.iter().filter_map(|s| match s {
                    RtcStats::MediaSource(m) if m.source.track_identifier == id => Some(m.rtc.id.as_str()),
                    _ => None,
                }).collect();
                for s in &all {
                    let RtcStats::OutboundRtp(o) = s else { continue };
                    if !sources.contains(&o.outbound.media_source_id.as_str()) {
                        continue;
                    }
                    out.encoder = o.outbound.encoder_implementation.clone();
                    out.frames_encoded += o.outbound.frames_encoded;
                    out.packets_sent += o.sent.packets_sent;
                    out.bytes_sent += o.sent.bytes_sent;
                    for r in &all {
                        if let RtcStats::RemoteInboundRtp(r) = r
                            && r.remote_inbound.local_id == o.rtc.id
                        {
                            out.sfu_reports += r.remote_inbound.round_trip_time_measurements;
                            out.sfu_lost += r.received.packets_lost;
                        }
                    }
                }
                out
            })
            .collect()
    }
}
