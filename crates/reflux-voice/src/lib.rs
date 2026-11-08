//! Voice and screen share over LiveKit.
//!
//! Fluxer hands out LiveKit grants over the gateway: send Voice State Update (op 4), receive
//! VOICE_SERVER_UPDATE with `endpoint` + `token`, then call [`Call::connect`] with them.
//! Participant identities are `user_{user_id}_{connection_id}` ([`user_of`]).
//!
//! The default `webrtc` feature builds the real implementation in `real.rs` (LiveKit +
//! libwebrtc). Without it, `stub.rs` provides the same public API with everything unavailable,
//! so the app builds for UI work without compiling libwebrtc.

#[cfg(feature = "webrtc")]
mod real;
#[cfg(feature = "webrtc")]
mod deepfilter;
#[cfg(feature = "webrtc")]
pub use real::*;

// shortcut: a second, hand-maintained implementation of the same API (`stub.rs`); delete it when
// libwebrtc builds fast enough that the split no longer pays for itself.
#[cfg(not(feature = "webrtc"))]
mod stub;
#[cfg(not(feature = "webrtc"))]
pub use stub::*;

/// `VoiceNoiseSuppressionBackend`: what cleans the mic before it is sent.
// shortcut: Fluxer's Noise gate, Speex and GTCRN backends aren't offered (no Rust ports wired).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Noise {
    None,
    /// WebRTC's own noise suppression (the APM).
    Standard,
    #[default]
    Rnnoise,
    /// DeepFilterNet3; RNNoise stands in while the model loads or if it can't start.
    DeepFilter,
}

/// Where the DeepFilterNet model is (`NoiseSuppressionAvailability`'s fallback state).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NoiseState {
    #[default]
    Idle,
    Loading,
    Ready,
    Failed,
}

// Shared with both implementations: `reflux_voice::audio::Resampler`.
mod resample;
// The stub only carries the settings.
#[cfg_attr(not(feature = "webrtc"), allow(dead_code))]
mod gate;
pub use gate::GateConfig;
