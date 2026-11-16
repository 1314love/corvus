//! Pairing token creation and storage.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::crypto::{base64url_decode, base64url_encode, random_array};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::error::ProviderError;

/// Number of random bytes in a pairing token.
pub const PAIRING_TOKEN_BYTES: usize = 32;

/// A 32-byte pairing secret.
#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct PairingToken {
    bytes: [u8; PAIRING_TOKEN_BYTES],
}

impl std::fmt::Debug for PairingToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PairingToken(***)")
    }
}

impl PairingToken {
    /// Generate a fresh token from the operating system random source.
    pub fn generate() -> Result<Self, ProviderError> {
        Ok(Self {
            bytes: random_array()?,
        })
    }

    /// Parse an unpadded base64url token.
    pub fn from_base64url(raw: &str) -> Result<Self, ProviderError> {
        let decoded = base64url_decode(raw)?;
        if decoded.len() != PAIRING_TOKEN_BYTES {
            return Err(ProviderError::Malformed(
                "pairing token must be 32 bytes".into(),
            ));
        }
        let mut bytes = [0_u8; PAIRING_TOKEN_BYTES];
        bytes.copy_from_slice(&decoded);
        Ok(Self { bytes })
    }

    /// Raw token bytes for HMAC and key derivation.
    pub fn as_bytes(&self) -> &[u8; PAIRING_TOKEN_BYTES] {
        &self.bytes
    }

    /// Unpadded base64url representation written to disk.
    pub fn to_base64url(&self) -> String {
        base64url_encode(&self.bytes)
    }
}

/// Path of the gateway-owned pairing token under a Corvus home directory.
pub fn pairing_token_path(home: &Path) -> PathBuf {
    home.join("provider").join("pairing-token")
}

/// Read the pairing token from disk without creating it.
pub fn load_pairing_token(home: &Path) -> Result<PairingToken, ProviderError> {
    let path = pairing_token_path(home);
    let raw = fs::read_to_string(&path)?;
    PairingToken::from_base64url(raw.trim())
}

/// Create the token once, or read the existing token.
pub fn load_or_create_pairing_token(home: &Path) -> Result<PairingToken, ProviderError> {
    let path = pairing_token_path(home);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(&path) {
        Ok(mut file) => {
            let token = PairingToken::generate()?;
            let encoded = token.to_base64url();
            if let Err(error) = file.write_all(encoded.as_bytes()) {
                let _ = fs::remove_file(&path);
                return Err(error.into());
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = file.set_permissions(fs::Permissions::from_mode(0o600));
            }
            Ok(token)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let raw = fs::read_to_string(&path)?;
            PairingToken::from_base64url(raw.trim())
        }
        Err(error) => Err(error.into()),
    }
}

/// Use CORVUS_PAIRING_TOKEN when set, otherwise the home-directory token.
///
/// The environment override exists for tests and development only.  The
/// returned error never includes the token value.
pub fn pairing_token_from_env_or_home(home: &Path) -> Result<PairingToken, ProviderError> {
    match std::env::var("CORVUS_PAIRING_TOKEN") {
        Ok(raw) => PairingToken::from_base64url(&raw),
        Err(std::env::VarError::NotPresent) => load_or_create_pairing_token(home),
        Err(_) => Err(ProviderError::Malformed(
            "CORVUS_PAIRING_TOKEN is not valid UTF-8".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairing_token_file_is_created_once_and_reused() {
        let directory = tempfile::tempdir().expect("tempdir");
        let first = load_or_create_pairing_token(directory.path()).expect("first token");
        let second = load_or_create_pairing_token(directory.path()).expect("second token");
        assert_eq!(first, second);
        let path = pairing_token_path(directory.path());
        assert_eq!(
            fs::read_to_string(&path).expect("token file"),
            first.to_base64url()
        );
        assert_eq!(first.to_base64url().len(), 43);
        assert!(!first.to_base64url().contains('='));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).expect("metadata").permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn invalid_pairing_tokens_are_rejected_without_echoing_them() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = pairing_token_path(directory.path());
        fs::create_dir_all(path.parent().expect("parent")).expect("provider dir");
        fs::write(&path, "not-a-token").expect("write token");
        let error = load_pairing_token(directory.path()).expect_err("invalid token");
        assert!(!error.to_string().contains("not-a-token"));
    }
}
