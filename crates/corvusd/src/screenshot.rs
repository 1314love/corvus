//! Screenshot image decoding and storage.
//!
//! Browser channels return screenshots as base64 image bytes. The daemon is
//! the single place that validates the payload, measures it, hashes it and
//! writes it below the Corvus home, so every channel and batch step obeys the
//! same limits and naming rules.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

/// Largest decoded screenshot the daemon accepts.
pub const MAX_SCREENSHOT_BYTES: usize = 8 * 1024 * 1024;

/// A screenshot that was written below the Corvus home.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredScreenshot {
    /// Absolute path to the written image.
    pub path: PathBuf,
    /// Decoded size in bytes.
    pub bytes: usize,
    /// SHA-256 of the decoded image, lowercase hex.
    pub sha256: String,
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
}

/// A screenshot payload could not be decoded or stored.
#[derive(Debug, thiserror::Error)]
pub enum ScreenshotError {
    /// The payload is not valid base64.
    #[error("screenshot image data is not valid base64")]
    Base64,
    /// The payload exceeds the configured cap.
    #[error("screenshot image is too large: {0} bytes (maximum {MAX_SCREENSHOT_BYTES})")]
    TooLarge(usize),
    /// The image header did not contain usable dimensions.
    #[error("screenshot image dimensions could not be decoded")]
    Dimensions,
    /// The image file could not be written.
    #[error("screenshot file could not be written: {0}")]
    Io(#[from] std::io::Error),
}

/// Decode a base64 screenshot and return its bytes and dimensions.
pub fn decode_screenshot(format: &str, data: &str) -> Result<(Vec<u8>, u32, u32), ScreenshotError> {
    // Reject an obviously oversized encoded payload before allocating.
    let max_encoded = MAX_SCREENSHOT_BYTES.div_ceil(3) * 4 + 4;
    if data.len() > max_encoded {
        return Err(ScreenshotError::TooLarge(data.len()));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data.as_bytes())
        .map_err(|_| ScreenshotError::Base64)?;
    if bytes.len() > MAX_SCREENSHOT_BYTES {
        return Err(ScreenshotError::TooLarge(bytes.len()));
    }
    let (width, height) = image_dimensions(format, &bytes)?;
    Ok((bytes, width, height))
}

/// Write a screenshot below root and return its metadata.
///
/// The file name contains the current Unix time in milliseconds and a random
/// suffix, and the file is created exclusively so an existing image is never
/// overwritten.
pub async fn write_screenshot(
    root: &Path,
    format: &str,
    data: &str,
) -> Result<StoredScreenshot, ScreenshotError> {
    let (bytes, width, height) = decode_screenshot(format, data)?;
    tokio::fs::create_dir_all(root).await?;
    let extension = match format {
        "png" => "png",
        "jpeg" | "jpg" => "jpg",
        _ => "webp",
    };
    let sha256 = hex::encode(Sha256::digest(&bytes));
    for _ in 0..16 {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or_default();
        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let name = format!(
            "screenshot-{millis}-{}.{}",
            &suffix[..8.min(suffix.len())],
            extension
        );
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
                return Ok(StoredScreenshot {
                    path,
                    bytes: bytes.len(),
                    sha256,
                    width,
                    height,
                });
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(ScreenshotError::Io(error)),
        }
    }
    Err(ScreenshotError::Io(std::io::Error::new(
        ErrorKind::AlreadyExists,
        "could not allocate a unique screenshot file name",
    )))
}

fn image_dimensions(format: &str, bytes: &[u8]) -> Result<(u32, u32), ScreenshotError> {
    match format {
        "png" => png_dimensions(bytes),
        "webp" => webp_dimensions(bytes),
        "jpeg" | "jpg" => jpeg_dimensions(bytes),
        _ => Err(ScreenshotError::Dimensions),
    }
}

fn png_dimensions(bytes: &[u8]) -> Result<(u32, u32), ScreenshotError> {
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    if bytes.len() < 24 || bytes[..8] != SIGNATURE || &bytes[12..16] != b"IHDR" {
        return Err(ScreenshotError::Dimensions);
    }
    let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    positive(width, height)
}

fn webp_dimensions(bytes: &[u8]) -> Result<(u32, u32), ScreenshotError> {
    if bytes.len() < 30 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        return Err(ScreenshotError::Dimensions);
    }
    match &bytes[12..16] {
        b"VP8X" => {
            let width = 1
                + (u32::from(bytes[24])
                    | (u32::from(bytes[25]) << 8)
                    | (u32::from(bytes[26]) << 16));
            let height = 1
                + (u32::from(bytes[27])
                    | (u32::from(bytes[28]) << 8)
                    | (u32::from(bytes[29]) << 16));
            positive(width, height)
        }
        b"VP8 " => {
            if bytes.len() < 30 || bytes[23..26] != [0x9d, 0x01, 0x2a] {
                return Err(ScreenshotError::Dimensions);
            }
            let width = u32::from(u16::from_le_bytes([bytes[26], bytes[27]]) & 0x3fff);
            let height = u32::from(u16::from_le_bytes([bytes[28], bytes[29]]) & 0x3fff);
            positive(width, height)
        }
        b"VP8L" => {
            if bytes.len() < 25 || bytes[20] != 0x2f {
                return Err(ScreenshotError::Dimensions);
            }
            let b0 = u32::from(bytes[21]);
            let b1 = u32::from(bytes[22]);
            let b2 = u32::from(bytes[23]);
            let b3 = u32::from(bytes[24]);
            let width = 1 + ((b0 | (b1 << 8)) & 0x3fff);
            let height = 1 + (((b1 >> 6) | (b2 << 2) | (b3 << 10)) & 0x3fff);
            positive(width, height)
        }
        _ => Err(ScreenshotError::Dimensions),
    }
}

fn jpeg_dimensions(bytes: &[u8]) -> Result<(u32, u32), ScreenshotError> {
    if bytes.len() < 4 || bytes[0] != 0xff || bytes[1] != 0xd8 {
        return Err(ScreenshotError::Dimensions);
    }
    let mut index = 2;
    while index + 9 <= bytes.len() {
        if bytes[index] != 0xff {
            return Err(ScreenshotError::Dimensions);
        }
        let marker = bytes[index + 1];
        index += 2;
        if marker == 0xd8 || marker == 0xd9 {
            continue;
        }
        if index + 2 > bytes.len() {
            return Err(ScreenshotError::Dimensions);
        }
        let length = usize::from(u16::from_be_bytes([bytes[index], bytes[index + 1]]));
        if length < 2 || index + length > bytes.len() {
            return Err(ScreenshotError::Dimensions);
        }
        if matches!(
            marker,
            0xc0 | 0xc1
                | 0xc2
                | 0xc3
                | 0xc5
                | 0xc6
                | 0xc7
                | 0xc9
                | 0xca
                | 0xcb
                | 0xcd
                | 0xce
                | 0xcf
        ) {
            if index + 7 > bytes.len() {
                return Err(ScreenshotError::Dimensions);
            }
            let height = u32::from(u16::from_be_bytes([bytes[index + 3], bytes[index + 4]]));
            let width = u32::from(u16::from_be_bytes([bytes[index + 5], bytes[index + 6]]));
            return positive(width, height);
        }
        index += length;
    }
    Err(ScreenshotError::Dimensions)
}

fn positive(width: u32, height: u32) -> Result<(u32, u32), ScreenshotError> {
    if width == 0 || height == 0 {
        return Err(ScreenshotError::Dimensions);
    }
    Ok((width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_header(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        bytes.extend_from_slice(&13u32.to_be_bytes());
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
        bytes
    }

    fn encoded(bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    #[test]
    fn png_dimensions_are_read_from_the_header() {
        let (_, width, height) = decode_screenshot("png", &encoded(&png_header(320, 200))).unwrap();
        assert_eq!((width, height), (320, 200));
    }

    #[test]
    fn webp_and_jpeg_dimensions_are_read_from_the_header() {
        let mut webp = vec![0u8; 30];
        webp[..4].copy_from_slice(b"RIFF");
        webp[8..12].copy_from_slice(b"WEBP");
        webp[12..16].copy_from_slice(b"VP8X");
        webp[24..27].copy_from_slice(&[0x7f, 0x02, 0x00]);
        webp[27..30].copy_from_slice(&[0x8f, 0x01, 0x00]);
        let (_, width, height) = decode_screenshot("webp", &encoded(&webp)).unwrap();
        assert_eq!((width, height), (640, 400));

        let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe0, 0x00, 0x04, 0x00, 0x00];
        jpeg.extend_from_slice(&[0xff, 0xc0, 0x00, 0x11, 0x08]);
        jpeg.extend_from_slice(&240u16.to_be_bytes());
        jpeg.extend_from_slice(&320u16.to_be_bytes());
        jpeg.extend_from_slice(&[3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]);
        let (_, width, height) = decode_screenshot("jpeg", &encoded(&jpeg)).unwrap();
        assert_eq!((width, height), (320, 240));
    }

    #[test]
    fn invalid_and_oversized_payloads_are_typed() {
        assert!(matches!(
            decode_screenshot("png", "not base64"),
            Err(ScreenshotError::Base64)
        ));
        assert!(matches!(
            decode_screenshot("png", &encoded(b"not a png")),
            Err(ScreenshotError::Dimensions)
        ));
        assert!(matches!(
            decode_screenshot("png", &"A".repeat(MAX_SCREENSHOT_BYTES / 3 * 4 + 8)),
            Err(ScreenshotError::TooLarge(_))
        ));
    }

    #[tokio::test]
    async fn files_are_unique_and_metadata_matches_the_payload() {
        let root = tempfile::tempdir().unwrap();
        let bytes = png_header(10, 20);
        let first = write_screenshot(root.path(), "png", &encoded(&bytes))
            .await
            .unwrap();
        let second = write_screenshot(root.path(), "png", &encoded(&bytes))
            .await
            .unwrap();
        assert_ne!(first.path, second.path);
        assert_eq!(tokio::fs::read(&first.path).await.unwrap(), bytes);
        assert_eq!(first.bytes, bytes.len());
        assert_eq!((first.width, first.height), (10, 20));
        assert_eq!(first.sha256, hex::encode(Sha256::digest(&bytes)));
        assert!(first
            .path
            .parent()
            .is_some_and(|parent| parent == root.path()));
    }
}
