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
pub use real::*;

// shortcut: a second, hand-maintained implementation of the same API (`stub.rs`); delete it when
// libwebrtc builds fast enough that the split no longer pays for itself.
#[cfg(not(feature = "webrtc"))]
mod stub;
#[cfg(not(feature = "webrtc"))]
pub use stub::*;

// Shared with both implementations: `reflux_voice::audio::Resampler`.
mod resample;
