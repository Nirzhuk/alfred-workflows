//! Authenticated encryption and keyed lookup digests for the WhatsApp protocol
//! store (Plan 023 Step 2).
//!
//! Every sensitive value written to the protocol database is sealed with
//! ChaCha20-Poly1305 under a versioned envelope. Every sensitive lookup key is
//! replaced by a keyed HMAC-SHA256 digest, so the database never contains a
//! plaintext JID, phone number, address, or message identifier — not even as an
//! index key.
//!
//! The master key is random, generated once at pairing, and lives in the OS
//! credential store. It is never derived from a phone number, JID, device id, or
//! user password.

use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, KeyInit, Nonce};
use hmac::{Hmac, Mac};
use rand::rngs::OsRng;
use rand::RngCore;
use sha2::Sha256;
use thiserror::Error;
use zeroize::Zeroize;

type HmacSha256 = Hmac<Sha256>;

/// Envelope format version. Bump only alongside a migration that can still read
/// every older version.
const ENVELOPE_V1: u8 = 1;
const NONCE_LEN: usize = 12;
/// `version || nonce || tag` with an empty body.
const MIN_ENVELOPE_LEN: usize = 1 + NONCE_LEN + 16;

/// Domain-separation labels. The master key is never used directly; each
/// purpose gets its own subkey so a digest can never act as a cipher key.
const LABEL_AEAD: &[u8] = b"alfred.whatsapp.store.aead.v1";
const LABEL_INDEX: &[u8] = b"alfred.whatsapp.store.index.v1";

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("protocol store key material is malformed")]
    MalformedKey,
    #[error("protocol store record is malformed")]
    MalformedRecord,
    #[error("protocol store record version {0} is not supported")]
    UnsupportedVersion(u8),
    #[error("protocol store record failed authentication")]
    NotAuthentic,
}

/// The master key for one paired WhatsApp account's protocol store.
///
/// Zeroized on drop. Deliberately has no `Debug`, `Display`, `Serialize`, or
/// `Clone` implementation: the only way out is [`StoreKey::expose_base64`], which
/// exists solely to hand the key to the OS credential store.
pub struct StoreKey {
    master: [u8; 32],
}

impl Drop for StoreKey {
    fn drop(&mut self) {
        self.master.zeroize();
    }
}

impl StoreKey {
    /// Fresh random key. Used once, at pairing.
    pub fn generate() -> Self {
        let mut master = [0u8; 32];
        OsRng.fill_bytes(&mut master);
        Self { master }
    }

    /// Rebuilds a key from what the OS credential store handed back.
    pub fn from_base64(encoded: &str) -> Result<Self, CryptoError> {
        use base64::Engine;
        let mut raw = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| CryptoError::MalformedKey)?;
        if raw.len() != 32 {
            raw.zeroize();
            return Err(CryptoError::MalformedKey);
        }
        let mut master = [0u8; 32];
        master.copy_from_slice(&raw);
        raw.zeroize();
        Ok(Self { master })
    }

    /// Encodes the key for the OS credential store. Never log, persist, or send
    /// the result anywhere else.
    pub fn expose_base64(&self) -> String {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(self.master)
    }

    /// Per-purpose subkey, so the AEAD key and the index key are independent.
    fn subkey(&self, label: &[u8]) -> [u8; 32] {
        let mut mac =
            HmacSha256::new_from_slice(&self.master).expect("HMAC accepts any key length");
        mac.update(label);
        mac.finalize().into_bytes().into()
    }

    fn cipher(&self) -> ChaCha20Poly1305 {
        let mut subkey = self.subkey(LABEL_AEAD);
        let cipher = ChaCha20Poly1305::new(&Key::try_from(&subkey[..]).expect("32-byte subkey"));
        subkey.zeroize();
        cipher
    }

    /// Seals `plaintext` into a versioned envelope bound to `aad`.
    ///
    /// `aad` must identify the row the value belongs to (table plus key digest),
    /// so a ciphertext copied into another row or another table fails to open.
    pub fn seal(&self, aad: &[u8], plaintext: &[u8]) -> Vec<u8> {
        let mut nonce_bytes = [0u8; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::try_from(&nonce_bytes[..]).expect("12-byte nonce");

        let ciphertext = self
            .cipher()
            .encrypt(
                &nonce,
                Payload {
                    msg: plaintext,
                    aad,
                },
            )
            // Only fails on absurd lengths (2^38 bytes); a protocol record is
            // never remotely that large.
            .expect("ChaCha20-Poly1305 encryption cannot fail for protocol-sized records");

        let mut envelope = Vec::with_capacity(1 + NONCE_LEN + ciphertext.len());
        envelope.push(ENVELOPE_V1);
        envelope.extend_from_slice(&nonce_bytes);
        envelope.extend_from_slice(&ciphertext);
        envelope
    }

    /// Opens an envelope produced by [`StoreKey::seal`] with the same `aad`.
    ///
    /// A wrong key, a wrong `aad`, a tampered nonce, and a tampered ciphertext
    /// are all indistinguishable [`CryptoError::NotAuthentic`] failures.
    pub fn open(&self, aad: &[u8], envelope: &[u8]) -> Result<Vec<u8>, CryptoError> {
        if envelope.len() < MIN_ENVELOPE_LEN {
            return Err(CryptoError::MalformedRecord);
        }
        let version = envelope[0];
        if version != ENVELOPE_V1 {
            return Err(CryptoError::UnsupportedVersion(version));
        }
        let nonce = Nonce::try_from(&envelope[1..1 + NONCE_LEN]).expect("12-byte nonce");
        let ciphertext = &envelope[1 + NONCE_LEN..];

        self.cipher()
            .decrypt(
                &nonce,
                Payload {
                    msg: ciphertext,
                    aad,
                },
            )
            .map_err(|_| CryptoError::NotAuthentic)
    }

    /// Keyed digest of a sensitive lookup key.
    ///
    /// `namespace` separates tables, so the same JID under `sessions` and under
    /// `identities` produces unrelated digests and cross-table correlation needs
    /// the key. Deterministic by design — equality lookups depend on it.
    pub fn digest(&self, namespace: &str, value: &[u8]) -> [u8; 32] {
        let mut subkey = self.subkey(LABEL_INDEX);
        let mut mac = HmacSha256::new_from_slice(&subkey).expect("HMAC accepts any key length");
        subkey.zeroize();
        // Length-prefixed so ("ab", "c") and ("a", "bc") cannot collide.
        mac.update(&(namespace.len() as u64).to_le_bytes());
        mac.update(namespace.as_bytes());
        mac.update(value);
        mac.finalize().into_bytes().into()
    }

    /// Digest of a composite key, e.g. `(chat, sender, msg_id)`. Each part is
    /// length-prefixed so no rearrangement of the parts collides.
    pub fn digest_parts(&self, namespace: &str, parts: &[&[u8]]) -> [u8; 32] {
        let mut buf = Vec::new();
        for part in parts {
            buf.extend_from_slice(&(part.len() as u64).to_le_bytes());
            buf.extend_from_slice(part);
        }
        let out = self.digest(namespace, &buf);
        buf.zeroize();
        out
    }
}

/// Builds the associated data binding a value to its row.
pub fn row_aad(namespace: &str, key_digest: &[u8]) -> Vec<u8> {
    let mut aad = Vec::with_capacity(namespace.len() + key_digest.len() + 8);
    aad.extend_from_slice(&(namespace.len() as u64).to_le_bytes());
    aad.extend_from_slice(namespace.as_bytes());
    aad.extend_from_slice(key_digest);
    aad
}

#[cfg(test)]
mod tests {
    use super::*;

    const JID: &[u8] = b"34600123456@s.whatsapp.net";

    fn aad() -> Vec<u8> {
        row_aad("sessions", &[7u8; 32])
    }

    #[test]
    fn seal_open_roundtrip() {
        let key = StoreKey::generate();
        let sealed = key.seal(&aad(), JID);
        assert_eq!(key.open(&aad(), &sealed).unwrap(), JID);
    }

    #[test]
    fn ciphertext_never_contains_the_plaintext() {
        let key = StoreKey::generate();
        let sealed = key.seal(&aad(), JID);
        assert!(
            sealed.windows(JID.len()).all(|w| w != JID),
            "plaintext survived into the envelope"
        );
    }

    #[test]
    fn nonce_is_random_per_seal() {
        let key = StoreKey::generate();
        let a = key.seal(&aad(), JID);
        let b = key.seal(&aad(), JID);
        assert_ne!(a, b, "identical envelopes mean a repeated nonce");
    }

    #[test]
    fn wrong_key_cannot_open() {
        let sealed = StoreKey::generate().seal(&aad(), JID);
        assert!(matches!(
            StoreKey::generate().open(&aad(), &sealed),
            Err(CryptoError::NotAuthentic)
        ));
    }

    #[test]
    fn wrong_aad_cannot_open() {
        let key = StoreKey::generate();
        let sealed = key.seal(&aad(), JID);
        let other = row_aad("identities", &[7u8; 32]);
        assert!(matches!(
            key.open(&other, &sealed),
            Err(CryptoError::NotAuthentic)
        ));
    }

    #[test]
    fn moving_a_value_to_another_row_fails() {
        let key = StoreKey::generate();
        let sealed = key.seal(&row_aad("sessions", &[1u8; 32]), JID);
        assert!(matches!(
            key.open(&row_aad("sessions", &[2u8; 32]), &sealed),
            Err(CryptoError::NotAuthentic)
        ));
    }

    #[test]
    fn tampering_is_detected() {
        let key = StoreKey::generate();
        for index in [1, 5, 20] {
            let mut sealed = key.seal(&aad(), JID);
            sealed[index] ^= 0x01;
            assert!(
                matches!(key.open(&aad(), &sealed), Err(CryptoError::NotAuthentic)),
                "flipping byte {index} went undetected"
            );
        }
    }

    #[test]
    fn truncated_and_unknown_versions_are_rejected() {
        let key = StoreKey::generate();
        assert!(matches!(
            key.open(&aad(), &[ENVELOPE_V1; 4]),
            Err(CryptoError::MalformedRecord)
        ));

        let mut sealed = key.seal(&aad(), JID);
        sealed[0] = 99;
        assert!(matches!(
            key.open(&aad(), &sealed),
            Err(CryptoError::UnsupportedVersion(99))
        ));
    }

    #[test]
    fn base64_roundtrip_preserves_the_key() {
        let key = StoreKey::generate();
        let restored = StoreKey::from_base64(&key.expose_base64()).unwrap();
        let sealed = key.seal(&aad(), JID);
        assert_eq!(restored.open(&aad(), &sealed).unwrap(), JID);
    }

    #[test]
    fn malformed_key_material_is_rejected() {
        assert!(matches!(
            StoreKey::from_base64("not base64 at all!!"),
            Err(CryptoError::MalformedKey)
        ));
        // Right encoding, wrong length.
        assert!(matches!(
            StoreKey::from_base64("c2hvcnQ="),
            Err(CryptoError::MalformedKey)
        ));
    }

    #[test]
    fn digest_is_deterministic_and_hides_the_input() {
        let key = StoreKey::generate();
        let a = key.digest("sessions", JID);
        assert_eq!(a, key.digest("sessions", JID));
        assert!(
            !a.windows(4).any(|w| JID.windows(4).any(|j| w == j)),
            "digest leaked input bytes"
        );
    }

    #[test]
    fn digest_is_namespaced_and_key_bound() {
        let key = StoreKey::generate();
        assert_ne!(key.digest("sessions", JID), key.digest("identities", JID));
        assert_ne!(
            key.digest("sessions", JID),
            StoreKey::generate().digest("sessions", JID)
        );
    }

    #[test]
    fn composite_digests_cannot_be_rearranged() {
        let key = StoreKey::generate();
        assert_ne!(
            key.digest_parts("secrets", &[b"ab", b"c"]),
            key.digest_parts("secrets", &[b"a", b"bc"])
        );
    }

    #[test]
    fn aead_and_index_subkeys_are_independent() {
        let key = StoreKey::generate();
        assert_ne!(key.subkey(LABEL_AEAD), key.subkey(LABEL_INDEX));
    }
}
