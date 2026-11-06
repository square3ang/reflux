use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

/// Unix milliseconds of the Fluxer epoch, `2015-01-01T00:00:00.000Z`.
pub const FLUXER_EPOCH_MS: u64 = 1_420_070_400_000;

/// A 64-bit Fluxer ID. Serialized as a decimal string; deserializes from a string or a number.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Snowflake(pub u64);

impl Snowflake {
    /// Creation time in Unix milliseconds.
    pub fn timestamp_ms(self) -> u64 {
        (self.0 >> 22) + FLUXER_EPOCH_MS
    }

    /// The smallest snowflake for a Unix-millisecond time, for use as a pagination cursor.
    pub fn from_timestamp_ms(ms: u64) -> Self {
        Self(ms.saturating_sub(FLUXER_EPOCH_MS) << 22)
    }
}

impl From<u64> for Snowflake {
    fn from(v: u64) -> Self {
        Self(v)
    }
}

impl fmt::Display for Snowflake {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for Snowflake {
    type Err = std::num::ParseIntError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse().map(Self)
    }
}

impl Serialize for Snowflake {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Snowflake {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl de::Visitor<'_> for V {
            type Value = Snowflake;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a snowflake as a decimal string or integer")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Snowflake, E> {
                v.parse().map_err(E::custom)
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Snowflake, E> {
                Ok(Snowflake(v))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Snowflake, E> {
                u64::try_from(v).map(Snowflake).map_err(E::custom)
            }
        }
        d.deserialize_any(V)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_timestamp() {
        let s: Snowflake = serde_json::from_str("\"1189375284394692608\"").unwrap();
        assert_eq!(s.0, 1189375284394692608);
        assert_eq!(serde_json::from_str::<Snowflake>("42").unwrap(), Snowflake(42));
        assert_eq!(serde_json::to_string(&s).unwrap(), "\"1189375284394692608\"");
        assert_eq!(Snowflake(0).timestamp_ms(), FLUXER_EPOCH_MS);
        let t = 1_700_000_000_000;
        assert_eq!(Snowflake::from_timestamp_ms(t).timestamp_ms(), t);
        assert!(serde_json::from_str::<Snowflake>("-1").is_err());
        assert!(serde_json::from_str::<Snowflake>("\"abc\"").is_err());
    }

    #[test]
    fn map_keys() {
        let m: std::collections::HashMap<Snowflake, String> =
            serde_json::from_str(r#"{"5":"note"}"#).unwrap();
        assert_eq!(m[&Snowflake(5)], "note");
        assert_eq!(serde_json::to_string(&m).unwrap(), r#"{"5":"note"}"#);
    }
}
