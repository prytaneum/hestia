//! Xbox request signing: an ECDSA P-256 proof key and the FILETIME-stamped
//! `Signature` header. One cross-platform implementation (`p256`) — the C++ tree
//! needed an OpenSSL/CNG split; Rust does not.

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine as _;
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use p256::elliptic_curve::Generate;

/// A per-login ECDSA P-256 proof key. Its public point (x, y) is advertised in
/// the Xbox proof-of-possession JWK; `sign` produces the raw 64-byte (r‖s)
/// signature the `Signature` header carries.
pub struct ProofKey {
    signing_key: SigningKey,
    id: String,
    x: String,
    y: String,
}

impl ProofKey {
    pub fn generate() -> ProofKey {
        let signing_key = SigningKey::generate();
        let point = signing_key.verifying_key().to_sec1_point(false);
        let x = point.x().expect("P-256 public point has an x coordinate");
        let y = point.y().expect("P-256 public point has a y coordinate");
        ProofKey {
            id: uuid::Uuid::new_v4().to_string(),
            x: base64url_nopad(x),
            y: base64url_nopad(y),
            signing_key,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn x(&self) -> &str {
        &self.x
    }
    pub fn y(&self) -> &str {
        &self.y
    }

    /// ECDSA/SHA-256 over `message`, serialized as fixed-width raw r‖s (64 bytes).
    /// Xbox verifies against the advertised proof key, so a fresh (randomized or
    /// deterministic) signature is accepted — byte-identity with any other signer
    /// is neither required nor possible.
    pub fn sign(&self, message: &[u8]) -> Vec<u8> {
        let signature: Signature = self.signing_key.sign(message);
        signature.to_bytes().to_vec()
    }
}

pub fn random_bytes(count: usize) -> Vec<u8> {
    let mut out = vec![0u8; count];
    getrandom::fill(&mut out).expect("system RNG must be available for Xbox proof-key material");
    out
}

pub fn base64url_nopad(data: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(data)
}

/// Build the base64 `Signature` header: a length-delimited message
/// (version, FILETIME, method, path, authorization, body) is signed, then the
/// (version ‖ FILETIME ‖ signature) envelope is base64-encoded.
pub fn xbox_signature_header(
    key: &ProofKey,
    url_path: &str,
    authorization: &str,
    body: &str,
    unix_time: i64,
) -> String {
    let filetime = (unix_time as u64).wrapping_add(11_644_473_600) * 10_000_000;

    let mut message = Vec::new();
    message.extend_from_slice(&1u32.to_be_bytes());
    message.push(0);
    message.extend_from_slice(&filetime.to_be_bytes());
    message.push(0);
    message.extend_from_slice(b"POST");
    message.push(0);
    message.extend_from_slice(url_path.as_bytes());
    message.push(0);
    message.extend_from_slice(authorization.as_bytes());
    message.push(0);
    message.extend_from_slice(body.as_bytes());
    message.push(0);

    let signature = key.sign(&message);
    debug_assert_eq!(signature.len(), 64, "ECDSA signature must be 64 bytes");

    let mut envelope = Vec::with_capacity(4 + 8 + signature.len());
    envelope.extend_from_slice(&1u32.to_be_bytes());
    envelope.extend_from_slice(&filetime.to_be_bytes());
    envelope.extend_from_slice(&signature);
    STANDARD.encode(envelope)
}

#[cfg(test)]
mod tests {
    //! Signing oracle tests. The C++ tree was the byte-diff oracle before the
    //! cutover; with one cross-platform `p256` signer there is nothing to diff
    //! against, so these pin the wire-visible envelope layout and lean on ECDSA's
    //! RFC 6979 determinism to catch a nonce/serialization regression.

    use super::*;
    use p256::ecdsa::signature::Verifier;
    use p256::ecdsa::{Signature, SigningKey, VerifyingKey};

    impl ProofKey {
        /// Build a proof key from a fixed scalar so a test signs deterministically.
        fn from_scalar_bytes(bytes: &[u8; 32]) -> ProofKey {
            let signing_key = SigningKey::from_slice(bytes).expect("valid P-256 scalar");
            let point = signing_key.verifying_key().to_sec1_point(false);
            let x = point.x().expect("x coordinate");
            let y = point.y().expect("y coordinate");
            ProofKey {
                id: "00000000-0000-4000-8000-000000000000".to_string(),
                x: base64url_nopad(x),
                y: base64url_nopad(y),
                signing_key,
            }
        }

        fn verifying_key(&self) -> VerifyingKey {
            *self.signing_key.verifying_key()
        }
    }

    /// Rebuild the exact message `xbox_signature_header` signs, so the test can
    /// verify the extracted signature against it.
    fn signed_message(filetime: u64, path: &str, authorization: &str, body: &str) -> Vec<u8> {
        let mut message = Vec::new();
        message.extend_from_slice(&1u32.to_be_bytes());
        message.push(0);
        message.extend_from_slice(&filetime.to_be_bytes());
        message.push(0);
        message.extend_from_slice(b"POST");
        message.push(0);
        message.extend_from_slice(path.as_bytes());
        message.push(0);
        message.extend_from_slice(authorization.as_bytes());
        message.push(0);
        message.extend_from_slice(body.as_bytes());
        message.push(0);
        message
    }

    #[test]
    fn filetime_conversion_is_a_fixed_vector() {
        // The Windows FILETIME epoch is 1601-01-01; 11_644_473_600 s before the
        // Unix epoch, in 100 ns ticks.
        let key = ProofKey::from_scalar_bytes(&[0x42; 32]);
        let header = xbox_signature_header(&key, "/authorize", "", "{}", 0);
        let envelope = STANDARD.decode(header).expect("base64 envelope");
        let filetime = u64::from_be_bytes(envelope[4..12].try_into().unwrap());
        assert_eq!(filetime, 116_444_736_000_000_000);
    }

    #[test]
    fn signature_header_envelope_layout() {
        let key = ProofKey::from_scalar_bytes(&[0x11; 32]);
        let header = xbox_signature_header(
            &key,
            "/device/authenticate",
            "XBL3.0 x=1;t",
            "{}",
            1_700_000_000,
        );
        let envelope = STANDARD.decode(header).expect("base64 envelope");
        // version(4) + filetime(8) + raw r‖s signature(64)
        assert_eq!(envelope.len(), 4 + 8 + 64);
        assert_eq!(u32::from_be_bytes(envelope[0..4].try_into().unwrap()), 1);
    }

    #[test]
    fn signature_verifies_against_the_proof_key() {
        let key = ProofKey::from_scalar_bytes(&[0x11; 32]);
        let (path, auth, body, unix) =
            ("/authorize", "XBL3.0 x=1;t", "{\"a\":1}", 1_700_000_000i64);
        let header = xbox_signature_header(&key, path, auth, body, unix);
        let envelope = STANDARD.decode(header).expect("base64 envelope");

        let filetime = (unix as u64 + 11_644_473_600) * 10_000_000;
        let message = signed_message(filetime, path, auth, body);
        let signature = Signature::from_slice(&envelope[12..]).expect("64-byte signature");
        key.verifying_key()
            .verify(&message, &signature)
            .expect("the envelope's signature must verify against the advertised proof key");
    }

    #[test]
    fn signing_is_deterministic() {
        // RFC 6979 nonces make two keys built from the same scalar sign a message
        // identically; a regression to randomized nonces would break replay diffs.
        let a = ProofKey::from_scalar_bytes(&[0x24; 32]);
        let b = ProofKey::from_scalar_bytes(&[0x24; 32]);
        let message = b"hestia-signing-oracle";
        assert_eq!(a.sign(message), b.sign(message));
        assert_eq!(a.sign(message), a.sign(message));
    }
}
