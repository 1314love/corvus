//! Page PDF decoding and storage.
//!
//! Browser channels return printed pages as base64 PDF bytes. The daemon is the
//! single place that validates the payload, measures it, hashes it and writes it
//! below the Corvus home, so every channel and caller obeys the same limits and
//! naming rules.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

/// Largest decoded PDF the daemon accepts.
pub const MAX_PDF_BYTES: usize = 32 * 1024 * 1024;

/// A PDF that was written below the Corvus home.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPdf {
    /// Absolute path to the written PDF.
    pub path: PathBuf,
    /// Decoded size in bytes.
    pub bytes: usize,
    /// SHA-256 of the decoded PDF, lowercase hex.
    pub sha256: String,
}

/// A PDF payload could not be decoded or stored.
#[derive(Debug, thiserror::Error)]
pub enum PdfError {
    /// The payload is not valid base64.
    #[error("pdf data is not valid base64")]
    Base64,
    /// The payload exceeds the configured cap.
    #[error("pdf data is too large: {0} bytes (maximum {MAX_PDF_BYTES})")]
    TooLarge(usize),
    /// The PDF file could not be written.
    #[error("pdf file could not be written: {0}")]
    Io(#[from] std::io::Error),
}

/// Decode a base64 PDF payload.
pub fn decode_pdf(data: &str) -> Result<Vec<u8>, PdfError> {
    let max_encoded = MAX_PDF_BYTES.div_ceil(3) * 4 + 4;
    if data.len() > max_encoded {
        return Err(PdfError::TooLarge(data.len()));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data.as_bytes())
        .map_err(|_| PdfError::Base64)?;
    if bytes.len() > MAX_PDF_BYTES {
        return Err(PdfError::TooLarge(bytes.len()));
    }
    Ok(bytes)
}

/// Write a PDF below root and return its metadata.
///
/// The file name contains the current Unix time in milliseconds and a random
/// suffix, and the file is created exclusively so an existing PDF is never
/// overwritten.
pub async fn write_pdf(root: &Path, data: &str) -> Result<StoredPdf, PdfError> {
    let bytes = decode_pdf(data)?;
    tokio::fs::create_dir_all(root).await?;
    let sha256 = hex::encode(Sha256::digest(&bytes));
    for _ in 0..16 {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or_default();
        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let name = format!("page-{millis}-{}.pdf", &suffix[..8.min(suffix.len())]);
        let path = root.join(name);
        match tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .await
        {
            Ok(mut file) => {
                file.write_all(&bytes).await?;
                file.flush().await?;
                return Ok(StoredPdf {
                    path,
                    bytes: bytes.len(),
                    sha256,
                });
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(PdfError::Io(error)),
        }
    }
    Err(PdfError::Io(std::io::Error::new(
        ErrorKind::AlreadyExists,
        "could not allocate a unique pdf file name",
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_root(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        std::env::temp_dir().join(format!("corvus-pdf-{}-{stamp}-{name}", std::process::id()))
    }

    #[test]
    fn decoded_pdf_is_capped_before_and_after_decoding() {
        assert!(matches!(decode_pdf("not base64!"), Err(PdfError::Base64)));
        let oversized = "A".repeat(MAX_PDF_BYTES.div_ceil(3) * 4 + 8);
        assert!(matches!(decode_pdf(&oversized), Err(PdfError::TooLarge(_))));
    }

    #[tokio::test]
    async fn pdfs_are_written_with_unique_names_and_a_hash() {
        let root = temporary_root("write");
        let encoded = base64::engine::general_purpose::STANDARD.encode(
            b"%PDF-1.7
%%EOF
",
        );
        let first = write_pdf(&root, &encoded).await.unwrap();
        let second = write_pdf(&root, &encoded).await.unwrap();
        assert_ne!(first.path, second.path);
        assert!(first.path.extension().and_then(|value| value.to_str()) == Some("pdf"));
        assert_eq!(first.bytes, 15);
        assert_eq!(first.sha256, second.sha256);
        assert_eq!(
            tokio::fs::read(&first.path).await.unwrap(),
            b"%PDF-1.7
%%EOF
"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
