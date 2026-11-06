//! Serde models for the Fluxer HTTP API and Gateway.
//!
//! Every response struct uses `#[serde(default)]`, so missing fields never fail parsing and
//! unknown fields are ignored. Timestamps are kept as their ISO 8601 strings.

/// Response structs: derives plus `#[serde(default)]`.
macro_rules! model {
    ($($(#[$m:meta])* pub struct $name:ident $(<$g:ident>)? { $($body:tt)* })*) => {$(
        #[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
        #[serde(default)]
        $(#[$m])*
        pub struct $name $(<$g>)? { $($body)* }
    )*};
}

/// Integer-valued enums kept open: unknown values survive a round trip.
macro_rules! int_enum {
    ($($(#[$m:meta])* pub struct $name:ident($t:ty) { $($c:ident = $v:expr),* $(,)? })*) => {$(
        $(#[$m])*
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub $t);
        impl $name { $(pub const $c: Self = Self($v);)* }
    )*};
}

/// Bitflags serialized as a JSON integer; unknown bits are retained.
macro_rules! int_flags {
    ($($(#[$m:meta])* pub struct $name:ident { $(const $c:ident = $v:expr;)* })*) => {$(
        bitflags::bitflags! {
            $(#[$m])*
            #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
            #[serde(from = "u64", into = "u64")]
            pub struct $name: u64 { $(const $c = $v;)* }
        }
        impl From<u64> for $name { fn from(v: u64) -> Self { Self::from_bits_retain(v) } }
        impl From<$name> for u64 { fn from(v: $name) -> u64 { v.bits() } }
    )*};
}

pub mod auth;
pub mod channel;
pub mod gateway;
pub mod guild;
pub mod instance;
pub mod message;
pub mod permissions;
pub mod snowflake;
pub mod user;

pub use auth::*;
pub use channel::*;
pub use gateway::*;
pub use guild::*;
pub use instance::*;
pub use message::*;
pub use permissions::*;
pub use snowflake::*;
pub use user::*;

/// ISO 8601 timestamp as sent by the API.
pub type Timestamp = String;

/// `serde_json::from_slice`, with the JSON path of a failure (`d.guilds[3].roles: missing field`)
/// in the error message.
pub fn from_slice<'de, T: serde::Deserialize<'de>>(bytes: &'de [u8]) -> Result<T, serde_json::Error> {
    serde_path_to_error::deserialize(&mut serde_json::Deserializer::from_slice(bytes)).map_err(with_path)
}

/// `T::deserialize(v)`, with the JSON path of a failure in the error message.
pub fn from_value<'de, T: serde::Deserialize<'de>>(v: &'de serde_json::Value) -> Result<T, serde_json::Error> {
    serde_path_to_error::deserialize(v).map_err(with_path)
}

fn with_path(e: serde_path_to_error::Error<serde_json::Error>) -> serde_json::Error {
    serde::de::Error::custom(format_args!("{}: {}", e.path(), e.inner()))
}

/// Generic `{guild_id, ...inner}` payload used by several Gateway events.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GuildScoped<T> {
    #[serde(default)]
    pub guild_id: Snowflake,
    #[serde(flatten)]
    pub inner: T,
}

/// Deserializes a present `null` as `Some(None)` so "omitted" and "null" stay distinct.
pub(crate) fn double_option<'de, T, D>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    T: serde::Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    serde::Deserialize::deserialize(d).map(Some)
}

pub(crate) fn is_false(b: &bool) -> bool {
    !*b
}
