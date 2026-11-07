//! `synced_preferences` (`fluxer.user.preferences.v1.SyncedPreferences`, base64 protobuf in the
//! user settings) at the wire level: read one top-level field, or replace it and send the whole
//! snapshot back with every other field's bytes untouched (`settings-protobuf.md`).
// shortcut: no `synced_preferences_base_hash` / 409 retry like `UserSettings.flushSyncedPreferences`;
// the splice always starts from the latest snapshot the gateway delivered.

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE};

/// One field: its number, wire type, and the whole record (tag included).
#[derive(Clone, Debug, PartialEq)]
pub struct Field<'a> {
    pub num: u32,
    pub wire: u8,
    pub raw: &'a [u8],
    /// The value: varint bytes, fixed bytes, or a length-delimited payload.
    pub value: &'a [u8],
}

fn varint(b: &[u8], at: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    for shift in (0..64).step_by(7) {
        let byte = *b.get(*at)?;
        *at += 1;
        v |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(v);
        }
    }
    None
}

pub fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push(v as u8 | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

/// A message's fields in order; `None` if it doesn't parse.
pub fn fields(b: &[u8]) -> Option<Vec<Field<'_>>> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < b.len() {
        let start = at;
        let key = varint(b, &mut at)?;
        let (num, wire) = ((key >> 3) as u32, (key & 7) as u8);
        let vstart;
        match wire {
            0 => {
                vstart = at;
                varint(b, &mut at)?;
            }
            1 | 5 => {
                vstart = at;
                at += if wire == 1 { 8 } else { 4 };
            }
            2 => {
                let len = varint(b, &mut at)? as usize;
                vstart = at;
                at = at.checked_add(len)?;
            }
            _ => return None,
        }
        if at > b.len() {
            return None;
        }
        out.push(Field { num, wire, raw: &b[start..at], value: &b[vstart..at] });
    }
    Some(out)
}

pub fn decode(b64: &str) -> Vec<u8> {
    STANDARD.decode(b64).or_else(|_| URL_SAFE.decode(b64)).unwrap_or_default()
}

/// The payload of top-level message field `num` (the last one, as protobuf merges), if set.
pub fn get(b64: &str, num: u32) -> Option<Vec<u8>> {
    let b = decode(b64);
    fields(&b)?.into_iter().rev().find(|f| f.num == num && f.wire == 2).map(|f| f.value.to_vec())
}

/// `b64` with message field `num` replaced by `payload` (`None` removes it).
pub fn set(b64: &str, num: u32, payload: Option<&[u8]>) -> String {
    let b = decode(b64);
    let mut out: Vec<u8> = fields(&b).unwrap_or_default().iter().filter(|f| f.num != num).flat_map(|f| f.raw.iter().copied()).collect();
    if let Some(p) = payload {
        bytes(&mut out, num, p);
    }
    if out.is_empty() { String::new() } else { STANDARD.encode(out) }
}

/// Length-delimited field `num`.
pub fn bytes(out: &mut Vec<u8>, num: u32, v: &[u8]) {
    put_varint(out, u64::from(num) << 3 | 2);
    put_varint(out, v.len() as u64);
    out.extend_from_slice(v);
}

/// Varint field `num` (proto3 skips zero unless `optional`).
pub fn uint(out: &mut Vec<u8>, num: u32, v: u64) {
    put_varint(out, u64::from(num) << 3);
    put_varint(out, v);
}

pub fn str_of(f: &Field) -> String {
    String::from_utf8_lossy(f.value).into_owned()
}

pub fn uint_of(f: &Field) -> u64 {
    varint(f.value, &mut 0).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splices_one_field_and_keeps_the_rest() {
        let mut m = Vec::new();
        uint(&mut m, 25, 1); // sanitize_urls
        bytes(&mut m, 40, b"\x20\x01"); // favorites { hide_muted_channels }
        bytes(&mut m, 111, b"\x12\x03ptt");
        let b64 = STANDARD.encode(&m);
        assert_eq!(get(&b64, 40).as_deref(), Some(&b"\x20\x01"[..]));
        let out = decode(&set(&b64, 40, Some(b"\x28\x01")));
        let f = fields(&out).unwrap();
        assert_eq!(f.iter().map(|f| f.num).collect::<Vec<_>>(), [25, 111, 40]);
        assert_eq!(f[2].value, b"\x28\x01");
        assert_eq!(set("", 40, None), "");
        assert!(fields(b"\x0a\x05ab").is_none(), "truncated");
    }
}
