//! AES-256-GCM framed session and typed provider messages.

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use zeroize::Zeroizing;

use crate::crypto::{base64_decode, base64_encode, open, seal, FRAME_LABEL};
use crate::error::ProviderError;
use crate::message::ProviderMessage;
use crate::{MAX_FRAME_BYTES, MAX_PLAINTEXT_BYTES};

/// Direction of one encrypted frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Extension to gateway.
    ClientToServer,
    /// Gateway to extension.
    ServerToClient,
}

impl Direction {
    fn byte(self) -> u8 {
        match self {
            Self::ClientToServer => 1,
            Self::ServerToClient => 2,
        }
    }
}

/// One encrypted frame on the wire.
#[derive(Debug, Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Frame {
    seq: u64,
    ciphertext: String,
}

struct AeadState {
    key: Zeroizing<[u8; 32]>,
    direction: Direction,
    next_seq: u64,
}

impl AeadState {
    fn new(key: [u8; 32], direction: Direction) -> Self {
        Self {
            key: Zeroizing::new(key),
            direction,
            next_seq: 0,
        }
    }

    fn nonce(&self, seq: u64) -> [u8; 12] {
        let mut nonce = [0_u8; 12];
        nonce[4..].copy_from_slice(&seq.to_be_bytes());
        nonce
    }

    fn aad(&self, seq: u64) -> [u8; 34] {
        let mut out = [0_u8; 34];
        out[..FRAME_LABEL.len()].copy_from_slice(FRAME_LABEL);
        out[FRAME_LABEL.len()] = self.direction.byte();
        out[FRAME_LABEL.len() + 1..].copy_from_slice(&seq.to_be_bytes());
        out
    }

    fn seal(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, ProviderError> {
        if plaintext.len() > MAX_PLAINTEXT_BYTES {
            return Err(ProviderError::TooLarge);
        }
        let seq = self.next_seq;
        let next = seq.checked_add(1).ok_or(ProviderError::Sequence)?;
        let nonce = self.nonce(seq);
        let aad = self.aad(seq);
        let ciphertext = seal(&self.key, &nonce, &aad, plaintext)?;
        self.next_seq = next;
        Ok(ciphertext)
    }

    fn open(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, ProviderError> {
        let seq = self.next_seq;
        let next = seq.checked_add(1).ok_or(ProviderError::Sequence)?;
        let nonce = self.nonce(seq);
        let aad = self.aad(seq);
        let plaintext = open(&self.key, &nonce, &aad, ciphertext)?;
        if plaintext.len() > MAX_PLAINTEXT_BYTES {
            return Err(ProviderError::TooLarge);
        }
        self.next_seq = next;
        Ok(plaintext)
    }
}

/// Symmetric encrypted frame codec for one direction pair.
pub struct FramedSession {
    outbound: AeadState,
    inbound: AeadState,
}

impl FramedSession {
    /// Build a codec from explicit keys and directions.
    pub fn new(
        outbound_key: [u8; 32],
        inbound_key: [u8; 32],
        outbound_direction: Direction,
        inbound_direction: Direction,
    ) -> Self {
        Self {
            outbound: AeadState::new(outbound_key, outbound_direction),
            inbound: AeadState::new(inbound_key, inbound_direction),
        }
    }

    /// Gateway side: server-to-client outbound, client-to-server inbound.
    pub fn server(c2s_key: [u8; 32], s2c_key: [u8; 32]) -> Self {
        Self::new(
            s2c_key,
            c2s_key,
            Direction::ServerToClient,
            Direction::ClientToServer,
        )
    }

    /// Client side: client-to-server outbound, server-to-client inbound.
    pub fn client(c2s_key: [u8; 32], s2c_key: [u8; 32]) -> Self {
        Self::new(
            c2s_key,
            s2c_key,
            Direction::ClientToServer,
            Direction::ServerToClient,
        )
    }

    /// Next outbound sequence number.
    pub fn outbound_seq(&self) -> u64 {
        self.outbound.next_seq
    }

    /// Next expected inbound sequence number.
    pub fn inbound_seq(&self) -> u64 {
        self.inbound.next_seq
    }

    /// Serialize and encrypt a JSON value.
    pub fn encode_value(&mut self, value: &Value) -> Result<Vec<u8>, ProviderError> {
        let plaintext = serde_json::to_vec(value)?;
        if plaintext.len() > MAX_PLAINTEXT_BYTES {
            return Err(ProviderError::TooLarge);
        }
        let seq = self.outbound.next_seq;
        let ciphertext = self.outbound.seal(&plaintext)?;
        let frame = Frame {
            seq,
            ciphertext: base64_encode(&ciphertext),
        };
        let encoded = serde_json::to_vec(&frame)?;
        if encoded.len() > MAX_FRAME_BYTES {
            return Err(ProviderError::TooLarge);
        }
        Ok(encoded)
    }

    /// Serialize and encrypt a typed message.
    pub fn encode_json<T: Serialize>(&mut self, value: &T) -> Result<Vec<u8>, ProviderError> {
        let value = serde_json::to_value(value)?;
        self.encode_value(&value)
    }

    /// Decode and decrypt one JSON frame.
    pub fn decode_value(&mut self, frame: &[u8]) -> Result<Value, ProviderError> {
        if frame.len() > MAX_FRAME_BYTES {
            return Err(ProviderError::TooLarge);
        }
        let frame: Frame = serde_json::from_slice(frame)?;
        if frame.seq != self.inbound.next_seq {
            return Err(ProviderError::Sequence);
        }
        // Reject an oversized ciphertext before allocating the decoded bytes.
        let max_ciphertext = MAX_PLAINTEXT_BYTES + 16;
        if frame.ciphertext.len() > (max_ciphertext * 4 / 3) + 8 {
            return Err(ProviderError::TooLarge);
        }
        let ciphertext = base64_decode(&frame.ciphertext)?;
        if ciphertext.len() > max_ciphertext {
            return Err(ProviderError::TooLarge);
        }
        let plaintext = self.inbound.open(&ciphertext)?;
        let value = serde_json::from_slice(&plaintext)?;
        Ok(value)
    }

    /// Decode and decrypt one typed message.
    pub fn decode_json<T: DeserializeOwned>(&mut self, frame: &[u8]) -> Result<T, ProviderError> {
        let value = self.decode_value(frame)?;
        Ok(serde_json::from_value(value)?)
    }
}

/// Typed view of a framed session that rejects unknown inner kinds.
pub struct ProviderSession {
    framed: FramedSession,
}

impl ProviderSession {
    /// Wrap a framed codec.
    pub fn new(framed: FramedSession) -> Self {
        Self { framed }
    }

    /// Encrypt one provider message.
    pub fn send_message(&mut self, message: &ProviderMessage) -> Result<Vec<u8>, ProviderError> {
        let value = serde_json::to_value(message)?;
        self.framed.encode_value(&value)
    }

    /// Decrypt and decode one provider message.
    pub fn receive_message(&mut self, frame: &[u8]) -> Result<ProviderMessage, ProviderError> {
        let value = self.framed.decode_value(frame)?;
        let kind = value
            .get("kind")
            .and_then(Value::as_str)
            .ok_or_else(|| ProviderError::Malformed("message has no kind".into()))?;
        if !matches!(
            kind,
            "server_confirm"
                | "client_confirm"
                | "manifest"
                | "manifest_ack"
                | "invoke"
                | "result"
                | "subscribe"
                | "subscription_ack"
                | "event"
                | "cancel"
                | "ping"
                | "pong"
                | "error"
        ) {
            return Err(ProviderError::Malformed(format!(
                "unknown provider message kind {kind}"
            )));
        }
        serde_json::from_value(value).map_err(ProviderError::from)
    }

    /// Next outbound sequence number.
    pub fn outbound_seq(&self) -> u64 {
        self.framed.outbound_seq()
    }

    /// Next expected inbound sequence number.
    pub fn inbound_seq(&self) -> u64 {
        self.framed.inbound_seq()
    }
}
