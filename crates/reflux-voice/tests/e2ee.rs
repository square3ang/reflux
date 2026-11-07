//! The grant's `e2ee_key` must turn into the same AES key libwebrtc's frame cryptor uses and
//! livekit-client's `ExternalE2EEKeyProvider.setKey(string)` derives (Fluxer's web/desktop app):
//! PBKDF2-SHA256 over the key's UTF-8 bytes, salt "LKFrameEncryptionKey", 100 000 iterations,
//! a 128-bit AES-GCM key. Checked both ways against libwebrtc's packet cryptor, which shares the
//! frame cryptor's key handler; the key provider is built with the options `Call::connect` uses.

use std::num::NonZeroU32;

use livekit::webrtc::native::frame_cryptor::{DataPacketCryptor, EncryptedPacket, EncryptionAlgorithm, KeyDerivationAlgorithm, KeyProvider, KeyProviderOptions};
use ring::aead::{AES_128_GCM, Aad, LessSafeKey, Nonce, UnboundKey};

const GRANT_KEY: &str = "c2VydmVyLWlzc3VlZC1yb29tLWtleQ"; // any string, like the gateway's

/// livekit-client `deriveKeys` (PBKDF2 mode), written independently of libwebrtc.
fn livekit_client_key(password: &str) -> LessSafeKey {
    let mut key = [0u8; 16];
    ring::pbkdf2::derive(ring::pbkdf2::PBKDF2_HMAC_SHA256, NonZeroU32::new(100_000).unwrap(), b"LKFrameEncryptionKey", password.as_bytes(), &mut key);
    LessSafeKey::new(UnboundKey::new(&AES_128_GCM, &key).unwrap())
}

fn cryptor() -> DataPacketCryptor {
    // `Call::connect`: shared key, ratchet window 0, everything else livekit's defaults.
    let provider = KeyProvider::new(KeyProviderOptions {
        shared_key: true,
        ratchet_window_size: 0,
        ratchet_salt: b"LKFrameEncryptionKey".to_vec(),
        failure_tolerance: -1,
        key_ring_size: 16,
        key_derivation_algorithm: KeyDerivationAlgorithm::PBKDF2,
    });
    assert!(provider.set_shared_key(0, GRANT_KEY.as_bytes().to_vec()));
    DataPacketCryptor::new(EncryptionAlgorithm::AesGcm, provider)
}

#[test]
fn grant_key_matches_livekit_client_derivation() {
    let c = cryptor();
    let key = livekit_client_key(GRANT_KEY);
    let plain = b"reflux e2ee loopback".to_vec();

    // libwebrtc encrypts → livekit-client's key decrypts.
    let p = c.encrypt("user_1_a", 0, &plain).unwrap();
    let mut buf = p.data.clone();
    let nonce = Nonce::try_assume_unique_for_key(&p.iv).unwrap();
    assert_eq!(key.open_in_place(nonce, Aad::empty(), &mut buf).unwrap(), &plain[..]);

    // livekit-client's key encrypts → libwebrtc decrypts.
    let iv = [7u8; 12];
    let mut sealed = plain.clone();
    key.seal_in_place_append_tag(Nonce::assume_unique_for_key(iv), Aad::empty(), &mut sealed).unwrap();
    let back = c.decrypt("user_2_b", &EncryptedPacket { data: sealed.clone(), iv: iv.to_vec(), key_index: 0 }).unwrap();
    assert_eq!(back, plain);

    // A different grant key must not decrypt it.
    let mut wrong = sealed;
    assert!(livekit_client_key("another key").open_in_place(Nonce::assume_unique_for_key(iv), Aad::empty(), &mut wrong).is_err());
}
