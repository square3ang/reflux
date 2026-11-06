//! ALTCHA v2 proof-of-work solver (`topics/captcha.md`, altcha-lib 2.x `solveChallenge`).
//!
//! The key is PBKDF2(password = nonce bytes ‖ counter as big-endian u32, salt, cost, keyLength);
//! the answer is the first counter whose hex key starts with `keyPrefix`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use base64::Engine;
use reflux_model::{AltchaChallenge, AltchaSolution, AltchaToken};
use serde_json::Value;
use sha2::Sha256;

use crate::{Error, Result};

const SOLVE_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_THREADS: usize = 8;

/// Solves the challenge on up to 8 threads. Blocking: call from `spawn_blocking`.
pub fn solve(challenge: &AltchaChallenge) -> Result<AltchaSolution> {
    let p = &challenge.parameters;
    let str_param = |k: &str| p.get(k).and_then(Value::as_str).ok_or_else(|| bad(k));
    let int_param = |k: &str, default: Option<u64>| {
        p.get(k).and_then(Value::as_u64).or(default).filter(|v| *v > 0).ok_or_else(|| bad(k))
    };
    let algorithm = str_param("algorithm")?;
    if algorithm != "PBKDF2/SHA-256" {
        return Err(Error::Captcha(format!("unsupported algorithm {algorithm}")));
    }
    let nonce = hex::decode(str_param("nonce")?).map_err(|_| bad("nonce"))?;
    let salt = hex::decode(str_param("salt")?).map_err(|_| bad("salt"))?;
    let cost = u32::try_from(int_param("cost", None)?).map_err(|_| bad("cost"))?;
    let key_len = int_param("keyLength", Some(32))? as usize;
    let prefix = str_param("keyPrefix")?.to_ascii_lowercase();
    // An empty prefix would match counter 0 and a too long one never matches.
    if prefix.is_empty() || prefix.len() > key_len * 2 || !prefix.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(bad("keyPrefix"));
    }

    let threads = std::thread::available_parallelism().map_or(2, |n| n.get()).min(MAX_THREADS);
    let start = Instant::now();
    let found = AtomicBool::new(false);
    let result = std::thread::scope(|s| {
        let workers: Vec<_> = (0..threads)
            .map(|i| {
                let (nonce, salt, prefix, found) = (&nonce, &salt, &prefix, &found);
                s.spawn(move || {
                    let mut password = nonce.clone();
                    password.extend_from_slice(&[0; 4]);
                    let mut key = vec![0; key_len];
                    let mut counter = i as u32;
                    while !found.load(Ordering::Relaxed) && start.elapsed() < SOLVE_TIMEOUT {
                        password[nonce.len()..].copy_from_slice(&counter.to_be_bytes());
                        pbkdf2::pbkdf2_hmac::<Sha256>(&password, salt, cost, &mut key);
                        let hex = hex::encode(&key);
                        if hex.starts_with(prefix.as_str()) {
                            found.store(true, Ordering::Relaxed);
                            return Some((counter, hex));
                        }
                        counter = counter.checked_add(threads as u32)?;
                    }
                    None
                })
            })
            .collect();
        // Lowest counter wins so the result does not depend on thread scheduling.
        workers.into_iter().filter_map(|w| w.join().ok().flatten()).min_by_key(|(c, _)| *c)
    });
    let (counter, derived_key) = result.ok_or_else(|| Error::Captcha("no solution found".into()))?;
    Ok(AltchaSolution {
        counter: counter.into(),
        derived_key,
        time: Some(start.elapsed().as_secs_f64() * 1000.0),
    })
}

/// Solves the challenge and returns the `X-Captcha-Token` value.
pub fn token(challenge: &AltchaChallenge) -> Result<String> {
    let solution = solve(challenge)?;
    let token = AltchaToken { challenge: challenge.clone(), solution };
    Ok(base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(&token)?))
}

fn bad(field: &str) -> Error {
    Error::Captcha(format!("invalid challenge parameter {field}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Generated with altcha-lib 2.8.0 `createChallenge({cost: 1000, counter: 37, ...})` and checked
    /// with its `solveChallenge`.
    const VECTOR: &str = r#"{"parameters":{"algorithm":"PBKDF2/SHA-256","cost":1000,"expiresAt":4102444800,
        "keyLength":32,"keyPrefix":"09aa2dcff9720201b98e83bb467ea9dc",
        "keySignature":"1796b36df1563f032c4f1aba8cca3c678ceccafd6cae12edecc6d41ef3597e5d",
        "nonce":"bd0286889168625f4deb53cab94b5cef","salt":"80e06e2fd78331d8bd0606dd9265d92a"},
        "signature":"bfb170fceed07fe4c7219a7bd36b968aa4c52e78fc7b8dc07e9a92d8f28f43d2"}"#;

    #[test]
    fn solves_reference_vector() {
        let challenge: AltchaChallenge = serde_json::from_str(VECTOR).unwrap();
        let s = solve(&challenge).unwrap();
        assert_eq!(s.counter, 37);
        assert_eq!(s.derived_key, "09aa2dcff9720201b98e83bb467ea9dc5b5559c349c5f78ed408637fd66405ea");

        // The token carries the challenge unchanged (server re-canonicalizes the parameters).
        let raw = base64::engine::general_purpose::STANDARD.decode(token(&challenge).unwrap()).unwrap();
        let v: Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(v["challenge"], serde_json::from_str::<Value>(VECTOR).unwrap());
        assert_eq!(v["solution"]["counter"], 37);
        assert!(v["solution"]["derivedKey"].is_string());
    }

    #[test]
    fn rejects_bad_parameters() {
        let mut challenge: AltchaChallenge = serde_json::from_str(VECTOR).unwrap();
        challenge.parameters["keyPrefix"] = "".into();
        assert!(solve(&challenge).is_err());
        challenge.parameters["algorithm"] = "SHA-256".into();
        assert!(solve(&challenge).is_err());
    }
}
