//! Cryptographic primitives used by the provider handshake and frames.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use hkdf::Hkdf;
use ring::agreement::{agree_ephemeral, EphemeralPrivateKey, UnparsedPublicKey, ECDH_P256};
use ring::hmac;
use ring::rand::{SecureRandom, SystemRandom};
use sha2::{Digest, Sha256};

use crate::error::ProviderError;

/// Label prefixed to the pairing token hash.
pub const TOKEN_LABEL: &[u8] = b"corvus-provider-v2-token";
/// Label prefixed to the handshake transcript.
pub const TRANSCRIPT_LABEL: &[u8] = b"corvus-provider-v2-transcript\0";
/// Label prefixed to every frame AAD.
pub const FRAME_LABEL: &[u8] = b"corvus-provider-v2-frame\0";
/// Prefix of every derived key label.
pub const KEY_LABEL: &[u8] = b"corvus-provider-v2/";

/// Four 256-bit keys derived for one connection.
pub struct SessionKeys {
    /// Client-to-server application key.
    pub c2s: [u8; 32],
    /// Server-to-client application key.
    pub s2c: [u8; 32],
    /// Server confirmation proof key.
    pub server_confirm: [u8; 32],
    /// Client confirmation proof key.
    pub client_confirm: [u8; 32],
}

/// Cryptographically random bytes.
pub fn random_bytes(length: usize) -> Result<Vec<u8>, ProviderError> {
    let mut bytes = vec![0_u8; length];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| ProviderError::Io("secure random source failed".into()))?;
    Ok(bytes)
}

/// Cryptographically random fixed-size array.
pub fn random_array<const N: usize>() -> Result<[u8; N], ProviderError> {
    let bytes = random_bytes(N)?;
    let mut out = [0_u8; N];
    out.copy_from_slice(&bytes);
    Ok(out)
}

/// Standard base64 with padding.
pub fn base64_encode(bytes: &[u8]) -> String {
    STANDARD.encode(bytes)
}

/// Decode standard base64.
pub fn base64_decode(raw: &str) -> Result<Vec<u8>, ProviderError> {
    STANDARD
        .decode(raw.trim())
        .map_err(|_| ProviderError::Malformed("invalid base64".into()))
}

/// Base64url without padding, used for identifiers and the pairing token.
pub fn base64url_encode(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Decode unpadded base64url.
pub fn base64url_decode(raw: &str) -> Result<Vec<u8>, ProviderError> {
    URL_SAFE_NO_PAD
        .decode(raw.trim())
        .map_err(|_| ProviderError::Malformed("invalid base64url".into()))
}

/// Generate an ephemeral P-256 keypair and its 65-byte uncompressed public key.
pub fn p256_keypair() -> Result<(EphemeralPrivateKey, Vec<u8>), ProviderError> {
    let rng = SystemRandom::new();
    let private = EphemeralPrivateKey::generate(&ECDH_P256, &rng)
        .map_err(|_| ProviderError::Io("P-256 key generation failed".into()))?;
    let public = private
        .compute_public_key()
        .map_err(|_| ProviderError::Io("P-256 public key failed".into()))?
        .as_ref()
        .to_vec();
    Ok((private, public))
}

/// Whether a 65-byte uncompressed P-256 public key has the expected prefix.
pub fn valid_p256_public(bytes: &[u8]) -> bool {
    bytes.len() == 65 && bytes[0] == 0x04
}

/// Complete a P-256 ECDH agreement and return the shared secret.
pub fn p256_agree(
    private: EphemeralPrivateKey,
    peer_public: &[u8],
) -> Result<Vec<u8>, ProviderError> {
    if !valid_p256_public(peer_public) {
        return Err(ProviderError::Authentication);
    }
    let peer = UnparsedPublicKey::new(&ECDH_P256, peer_public);
    agree_ephemeral(private, &peer, |shared| shared.to_vec())
        .map_err(|_| ProviderError::Authentication)
}

/// SHA-256 of one input.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(data);
    let mut out = [0_u8; 32];
    out.copy_from_slice(&digest);
    out
}

/// SHA-256 of a sequence of inputs without concatenating them first.
pub fn sha256_parts(parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    let digest = hasher.finalize();
    let mut out = [0_u8; 32];
    out.copy_from_slice(&digest);
    out
}

/// HMAC-SHA256.
pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let key = hmac::Key::new(hmac::HMAC_SHA256, key);
    let tag = hmac::sign(&key, data);
    let mut out = [0_u8; 32];
    out.copy_from_slice(tag.as_ref());
    out
}

/// Verify HMAC-SHA256 in constant time.
pub fn hmac_verify(key: &[u8], data: &[u8], proof: &[u8]) -> bool {
    let key = hmac::Key::new(hmac::HMAC_SHA256, key);
    hmac::verify(&key, data, proof).is_ok()
}

/// Compute the handshake transcript in the spec's exact field order.
pub fn transcript(
    server_nonce: &[u8],
    client_nonce: &[u8],
    server_ephemeral: &[u8],
    client_ephemeral: &[u8],
    client_id: &[u8],
    client_public_key: &[u8],
) -> [u8; 32] {
    sha256_parts(&[
        TRANSCRIPT_LABEL,
        server_nonce,
        client_nonce,
        server_ephemeral,
        client_ephemeral,
        client_id,
        client_public_key,
    ])
}

/// SHA-256 of the token label plus the raw pairing token.
pub fn token_hash(token: &[u8; 32]) -> [u8; 32] {
    sha256_parts(&[TOKEN_LABEL, token])
}

fn expand_key(hkdf: &Hkdf<Sha256>, label: &[u8], hash: &[u8; 32]) -> [u8; 32] {
    let mut info = Vec::with_capacity(KEY_LABEL.len() + label.len() + 1 + hash.len());
    info.extend_from_slice(KEY_LABEL);
    info.extend_from_slice(label);
    info.push(0);
    info.extend_from_slice(hash);
    let mut key = [0_u8; 32];
    // HKDF-SHA256 output for 32 bytes cannot fail with a valid length.
    let _ = hkdf.expand(&info, &mut key);
    key
}

/// Derive the four connection keys from the ECDH secret.
pub fn derive_keys(shared: &[u8], salt: &[u8; 32], token: &[u8; 32]) -> SessionKeys {
    let hkdf = Hkdf::<Sha256>::new(Some(salt), shared);
    let hash = token_hash(token);
    SessionKeys {
        c2s: expand_key(&hkdf, b"c2s", &hash),
        s2c: expand_key(&hkdf, b"s2c", &hash),
        server_confirm: expand_key(&hkdf, b"server-confirm", &hash),
        client_confirm: expand_key(&hkdf, b"client-confirm", &hash),
    }
}

/// Encrypt one plaintext with AES-256-GCM.
pub fn seal(
    key: &[u8; 32],
    nonce: &[u8; 12],
    aad: &[u8],
    plaintext: &[u8],
) -> Result<Vec<u8>, ProviderError> {
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|_| ProviderError::Malformed("invalid AES key".into()))?;
    cipher
        .encrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| ProviderError::Authentication)
}

/// Decrypt one AES-256-GCM ciphertext.
pub fn open(
    key: &[u8; 32],
    nonce: &[u8; 12],
    aad: &[u8],
    ciphertext: &[u8],
) -> Result<Vec<u8>, ProviderError> {
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|_| ProviderError::Malformed("invalid AES key".into()))?;
    cipher
        .decrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|_| ProviderError::Authentication)
}
