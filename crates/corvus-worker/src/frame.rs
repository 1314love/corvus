//! Length-prefixed JSON frames used between the daemon and a CDP worker.

use futures::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::LengthDelimitedCodec;

use crate::error::WorkerError;

/// Maximum generic worker frame in bytes.
pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;
/// Maximum worker handshake frame in bytes.
pub const MAX_HANDSHAKE_BYTES: usize = 64 * 1024;

/// Read one length-prefixed frame with a caller-selected payload limit.
pub async fn read_frame_with_limit<R: AsyncRead + Unpin>(
    reader: &mut R,
    max: usize,
) -> Result<Value, WorkerError> {
    let mut framed = LengthDelimitedCodec::builder()
        .max_frame_length(max)
        .length_field_type::<u32>()
        .new_read(reader);
    match framed.next().await {
        Some(Ok(payload)) => Ok(serde_json::from_slice(&payload)?),
        Some(Err(error)) if error.kind() == std::io::ErrorKind::InvalidData => {
            Err(WorkerError::FrameTooLarge)
        }
        Some(Err(error)) => Err(WorkerError::Io(error.to_string())),
        None => Err(WorkerError::Io("worker closed its output".into())),
    }
}

/// Read one generic frame.
pub async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Value, WorkerError> {
    read_frame_with_limit(reader, MAX_FRAME_BYTES).await
}

/// Read the first worker handshake frame with its tighter limit.
pub async fn read_handshake<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Value, WorkerError> {
    read_frame_with_limit(reader, MAX_HANDSHAKE_BYTES).await
}

/// Write one length-prefixed frame and flush it.
pub async fn write_frame<W: AsyncWrite + Unpin>(
    writer: &mut W,
    value: &Value,
) -> Result<(), WorkerError> {
    let payload = serde_json::to_vec(value)?;
    if payload.len() > MAX_FRAME_BYTES || payload.len() > u32::MAX as usize {
        return Err(WorkerError::FrameTooLarge);
    }
    let mut framed = LengthDelimitedCodec::builder()
        .max_frame_length(MAX_FRAME_BYTES)
        .length_field_type::<u32>()
        .new_write(writer);
    framed.send(payload.as_slice()).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::io::duplex;

    #[tokio::test]
    async fn frame_round_trip_preserves_json() {
        let (mut left, mut right) = duplex(1024);
        let value = json!({"kind": "invoke", "id": "i-1", "args": {"n": 7}});
        write_frame(&mut left, &value).await.expect("encode");
        let decoded = read_frame(&mut right).await.expect("decode");
        assert_eq!(decoded, value);
    }

    #[tokio::test]
    async fn oversized_declared_frame_is_rejected_before_allocation() {
        let mut bytes = ((MAX_HANDSHAKE_BYTES + 1) as u32).to_be_bytes().to_vec();
        bytes.push(b'{');
        let (mut left, mut right) = duplex(bytes.len() + 16);
        use tokio::io::AsyncWriteExt;
        left.write_all(&bytes).await.expect("write prefix");
        left.shutdown().await.expect("shutdown");
        let error = read_handshake(&mut right).await.expect_err("oversized");
        assert_eq!(error, WorkerError::FrameTooLarge);
    }

    #[tokio::test]
    async fn truncated_frame_is_rejected() {
        let (mut left, mut right) = duplex(64);
        use tokio::io::AsyncWriteExt;
        left.write_all(&[0, 0, 0, 16]).await.expect("prefix");
        left.write_all(b"{}").await.expect("body");
        left.shutdown().await.expect("shutdown");
        let error = read_frame(&mut right).await.expect_err("truncated");
        assert!(matches!(error, WorkerError::Io(_)));
    }
}
