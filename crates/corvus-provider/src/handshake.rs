//! Server-side provider handshake and post-confirmation session state.

use ring::agreement::EphemeralPrivateKey;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::crypto::{
    base64_decode, base64_encode, base64url_decode, base64url_encode, derive_keys, hmac_sha256,
    hmac_verify, p256_agree, p256_keypair, random_array, transcript,
};
use crate::error::ProviderError;
use crate::message::{Manifest, ProviderMessage, ProviderResult};
use crate::session::{FramedSession, ProviderSession};
use crate::MAX_HANDSHAKE_BYTES;

/// First message sent by the gateway on /provider/v2.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerHello {
    /// Wire kind, always "server_hello".
    pub kind: String,
    /// Protocol version, always 2.
    pub protocol: u16,
    /// 16-byte gateway identifier, unpadded base64url.
    pub server_id: String,
    /// 32-byte server nonce, standard base64.
    pub server_nonce: String,
    /// 65-byte uncompressed P-256 server ephemeral key, standard base64.
    pub server_ephemeral: String,
    /// 32-byte HKDF salt, standard base64.
    pub salt: String,
    /// P1 always requires pairing.
    pub pairing_required: bool,
}

/// Client response to ServerHello.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientHello {
    /// Wire kind, always "client_hello".
    pub kind: String,
    /// Protocol version, must be 2.
    pub protocol: u16,
    /// Persistent 16..64 byte client identifier, unpadded base64url.
    pub client_id: String,
    /// 65-byte uncompressed P-256 client identity key, standard base64.
    pub client_public_key: String,
    /// 32-byte client nonce, standard base64.
    pub client_nonce: String,
    /// 65-byte uncompressed P-256 client ephemeral key, standard base64.
    pub client_ephemeral: String,
    /// HMAC-SHA256 pairing proof over the transcript, standard base64.
    pub pairing_proof: String,
}

/// Gateway half of an in-progress handshake.
pub struct ServerHandshake {
    token: [u8; 32],
    server_hello: ServerHello,
    server_nonce: [u8; 32],
    server_ephemeral: Vec<u8>,
    salt: [u8; 32],
    ephemeral: EphemeralPrivateKey,
}

impl ServerHandshake {
    /// Create a fresh handshake and the ServerHello to send.
    pub fn new(token: &[u8; 32]) -> Result<(Self, ServerHello), ProviderError> {
        let server_id = random_array::<16>()?;
        let server_nonce = random_array::<32>()?;
        let salt = random_array::<32>()?;
        let (ephemeral, server_ephemeral) = p256_keypair()?;
        let server_hello = ServerHello {
            kind: "server_hello".into(),
            protocol: 2,
            server_id: base64url_encode(&server_id),
            server_nonce: base64_encode(&server_nonce),
            server_ephemeral: base64_encode(&server_ephemeral),
            salt: base64_encode(&salt),
            pairing_required: true,
        };
        Ok((
            Self {
                token: *token,
                server_hello: server_hello.clone(),
                server_nonce,
                server_ephemeral,
                salt,
                ephemeral,
            },
            server_hello,
        ))
    }

    /// The ServerHello associated with this handshake.
    pub fn server_hello(&self) -> &ServerHello {
        &self.server_hello
    }

    /// Parse and accept one raw client_hello JSON document.
    pub fn accept_raw(self, raw: &[u8]) -> Result<PendingProvider, ProviderError> {
        if raw.len() > MAX_HANDSHAKE_BYTES {
            return Err(ProviderError::TooLarge);
        }
        let hello: ClientHello = serde_json::from_slice(raw)?;
        self.accept(&hello)
    }

    /// Validate the client hello and prepare the server confirmation frame.
    pub fn accept(self, client_hello: &ClientHello) -> Result<PendingProvider, ProviderError> {
        if client_hello.kind != "client_hello" {
            return Err(ProviderError::Malformed("expected client_hello".into()));
        }
        if client_hello.protocol != 2 {
            return Err(ProviderError::Version(client_hello.protocol));
        }
        if serde_json::to_vec(client_hello)?.len() > MAX_HANDSHAKE_BYTES {
            return Err(ProviderError::TooLarge);
        }
        let client_nonce = decode_fixed("client_nonce", &client_hello.client_nonce, 32)?;
        let client_ephemeral =
            decode_fixed("client_ephemeral", &client_hello.client_ephemeral, 65)?;
        if client_ephemeral[0] != 0x04 {
            return Err(ProviderError::Malformed(
                "client_ephemeral must be uncompressed P-256".into(),
            ));
        }
        let client_public_key =
            decode_fixed("client_public_key", &client_hello.client_public_key, 65)?;
        if client_public_key[0] != 0x04 {
            return Err(ProviderError::Malformed(
                "client_public_key must be uncompressed P-256".into(),
            ));
        }
        let client_id = base64url_decode(&client_hello.client_id)?;
        if !(16..=64).contains(&client_id.len()) {
            return Err(ProviderError::Malformed(
                "client_id must be 16..64 bytes".into(),
            ));
        }
        let pairing_proof = decode_fixed("pairing_proof", &client_hello.pairing_proof, 32)?;
        let transcript = transcript(
            &self.server_nonce,
            &client_nonce,
            &self.server_ephemeral,
            &client_ephemeral,
            &client_id,
            &client_public_key,
        );
        if !hmac_verify(&self.token, &transcript, &pairing_proof) {
            return Err(ProviderError::Authentication);
        }
        let shared = p256_agree(self.ephemeral, &client_ephemeral)?;
        let keys = derive_keys(&shared, &self.salt, &self.token);
        let session_id = format!("s-{}", Uuid::new_v4());
        let server_proof = hmac_sha256(&keys.server_confirm, &transcript);
        let mut session = ProviderSession::new(FramedSession::server(keys.c2s, keys.s2c));
        let server_confirm_frame = session.send_message(&ProviderMessage::ServerConfirm {
            protocol: 2,
            session_id: session_id.clone(),
            proof: base64_encode(&server_proof),
        })?;
        Ok(PendingProvider {
            session,
            transcript,
            client_confirm_key: zeroize::Zeroizing::new(keys.client_confirm),
            session_id,
            server_confirm_frame,
        })
    }
}

fn decode_fixed(field: &str, raw: &str, expected: usize) -> Result<Vec<u8>, ProviderError> {
    let decoded = base64_decode(raw)?;
    if decoded.len() != expected {
        return Err(ProviderError::Malformed(format!(
            "{field} must be {expected} bytes"
        )));
    }
    Ok(decoded)
}

/// Handshake state after the server confirmation has been queued.
pub struct PendingProvider {
    session: ProviderSession,
    transcript: [u8; 32],
    client_confirm_key: zeroize::Zeroizing<[u8; 32]>,
    session_id: String,
    server_confirm_frame: Vec<u8>,
}

impl PendingProvider {
    /// Opaque session identifier included in the server confirmation.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Handshake transcript used for both confirmation proofs.
    pub fn transcript(&self) -> &[u8; 32] {
        &self.transcript
    }

    /// Encrypted server_confirm frame at s2c sequence zero.
    pub fn server_confirm_frame(&self) -> &[u8] {
        &self.server_confirm_frame
    }

    /// Verify the encrypted client confirmation and return the established session.
    pub fn accept_client_confirm(
        mut self,
        frame: &[u8],
    ) -> Result<EstablishedProvider, ProviderError> {
        let message = self.session.receive_message(frame)?;
        let proof = match message {
            ProviderMessage::ClientConfirm { proof } => proof,
            _ => return Err(ProviderError::Message("expected client_confirm".into())),
        };
        let proof = decode_fixed("client_confirm proof", &proof, 32)?;
        if !hmac_verify(&self.client_confirm_key[..], &self.transcript, &proof) {
            return Err(ProviderError::Authentication);
        }
        Ok(EstablishedProvider {
            session: self.session,
            transcript: self.transcript,
            manifest: None,
            pending_invoke: None,
        })
    }
}

/// Authenticated provider session that can accept a manifest and one invoke.
pub struct EstablishedProvider {
    session: ProviderSession,
    transcript: [u8; 32],
    manifest: Option<Manifest>,
    pending_invoke: Option<String>,
}

impl EstablishedProvider {
    /// Handshake transcript.
    pub fn transcript(&self) -> &[u8; 32] {
        &self.transcript
    }

    /// Accepted manifest, if any.
    pub fn manifest(&self) -> Option<&Manifest> {
        self.manifest.as_ref()
    }

    /// Identifier of the one in-flight invoke, if any.
    pub fn pending_invoke(&self) -> Option<&str> {
        self.pending_invoke.as_deref()
    }

    /// Accept and validate the provider's manifest.
    pub fn accept_manifest(&mut self, frame: &[u8]) -> Result<&Manifest, ProviderError> {
        if self.manifest.is_some() {
            return Err(ProviderError::Message("manifest already accepted".into()));
        }
        let message = self.session.receive_message(frame)?;
        let manifest = match message {
            ProviderMessage::Manifest(manifest) => manifest,
            _ => return Err(ProviderError::Message("expected manifest".into())),
        };
        manifest.validate()?;
        self.manifest = Some(manifest);
        Ok(self.manifest.as_ref().expect("manifest just stored"))
    }

    /// Build the encrypted manifest_ack frame for the accepted manifest.
    pub fn manifest_ack_frame(&mut self) -> Result<Vec<u8>, ProviderError> {
        let provider_id = self
            .manifest
            .as_ref()
            .ok_or_else(|| ProviderError::Message("manifest has not been accepted".into()))?
            .provider_id
            .clone();
        self.session.send_message(&ProviderMessage::ManifestAck {
            provider_id,
            accepted: true,
        })
    }

    /// Send one invoke to the provider and remember its identifier.
    pub fn start_invoke(
        &mut self,
        action: &str,
        args: &Value,
        deadline_ms: u64,
    ) -> Result<Vec<u8>, ProviderError> {
        if let Some(pending) = &self.pending_invoke {
            return Err(ProviderError::BadArgs(format!(
                "invoke {pending} is already in flight"
            )));
        }
        if !(1..=300_000).contains(&deadline_ms) {
            return Err(ProviderError::BadArgs(
                "deadline_ms must be 1..300000".into(),
            ));
        }
        let (resolved_action, max_invoke_bytes) = {
            let manifest = self
                .manifest
                .as_ref()
                .ok_or_else(|| ProviderError::Message("manifest has not been accepted".into()))?;
            manifest.resolve_action(action)?;
            (action.to_string(), manifest.limits.max_invoke_bytes)
        };
        let args_size = serde_json::to_vec(args)?.len() as u64;
        if args_size > max_invoke_bytes {
            return Err(ProviderError::BadArgs(
                "invoke args exceed max_invoke_bytes".into(),
            ));
        }
        let id = Uuid::new_v4().to_string();
        let frame = self.session.send_message(&ProviderMessage::Invoke {
            id: id.clone(),
            action: resolved_action,
            args: args.clone(),
            deadline_ms,
        })?;
        self.pending_invoke = Some(id);
        Ok(frame)
    }

    /// Verify and return the result for the one in-flight invoke.
    pub fn accept_result(&mut self, frame: &[u8]) -> Result<ProviderResult, ProviderError> {
        let message = self.session.receive_message(frame)?;
        let (id, ok, value, error) = match message {
            ProviderMessage::Result {
                id,
                ok,
                value,
                error,
            } => (id, ok, value, error),
            _ => return Err(ProviderError::Message("expected result".into())),
        };
        if self.pending_invoke.as_deref() != Some(id.as_str()) {
            return Err(ProviderError::BadArgs(
                "result id does not match invoke".into(),
            ));
        }
        if ok && value.is_none() {
            return Err(ProviderError::Message(
                "successful result has no value".into(),
            ));
        }
        if !ok && error.is_none() {
            return Err(ProviderError::Message(
                "failed result has no error body".into(),
            ));
        }
        if let Some(manifest) = &self.manifest {
            let size = if ok {
                serde_json::to_vec(value.as_ref().expect("ok result has a value"))?.len()
            } else {
                serde_json::to_vec(error.as_ref().expect("failed result has an error"))?.len()
            } as u64;
            if size > manifest.limits.max_result_bytes {
                return Err(ProviderError::TooLarge);
            }
        }
        self.pending_invoke = None;
        Ok(ProviderResult {
            id,
            ok,
            value,
            error,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::{base64_decode, base64_encode, base64url_encode, random_bytes};
    use crate::message::{BrowserInfo, Capability, Limits, Manifest};
    use crate::session::Direction;
    use crate::MAX_PLAINTEXT_BYTES;
    use serde_json::json;

    fn sample_manifest() -> Manifest {
        Manifest {
            provider_id: format!("provider-{}", Uuid::new_v4()),
            version: "2.0.0".into(),
            browser: BrowserInfo {
                family: "edge".into(),
                version: "157.0.0.0".into(),
                profile: "default".into(),
            },
            capabilities: vec![
                Capability {
                    id: "system.info".into(),
                    version: 1,
                    actions: vec!["info".into()],
                },
                Capability {
                    id: "tabs.read".into(),
                    version: 1,
                    actions: vec!["list".into()],
                },
            ],
            subscriptions: vec!["tabs.updated".into()],
            limits: Limits {
                max_invoke_bytes: 1_048_576,
                max_result_bytes: 8_388_608,
            },
        }
    }

    struct TestClient {
        token: [u8; 32],
        ephemeral: Option<EphemeralPrivateKey>,
        ephemeral_public: Vec<u8>,
        client_public_key: Vec<u8>,
        client_nonce: [u8; 32],
        client_id: Vec<u8>,
    }

    impl TestClient {
        fn new(token: [u8; 32]) -> Self {
            let (ephemeral, ephemeral_public) = p256_keypair().expect("client ephemeral");
            let (_identity_private, client_public_key) = p256_keypair().expect("client identity");
            Self {
                token,
                ephemeral: Some(ephemeral),
                ephemeral_public,
                client_public_key,
                client_nonce: random_array().expect("client nonce"),
                client_id: random_bytes(16).expect("client id"),
            }
        }

        fn hello(&self, server: &ServerHello) -> ClientHello {
            self.hello_with(server, self.token, 2)
        }

        fn hello_with(&self, server: &ServerHello, token: [u8; 32], protocol: u16) -> ClientHello {
            let server_nonce = base64_decode(&server.server_nonce).expect("server nonce");
            let server_ephemeral =
                base64_decode(&server.server_ephemeral).expect("server ephemeral");
            let transcript = transcript(
                &server_nonce,
                &self.client_nonce,
                &server_ephemeral,
                &self.ephemeral_public,
                &self.client_id,
                &self.client_public_key,
            );
            ClientHello {
                kind: "client_hello".into(),
                protocol,
                client_id: base64url_encode(&self.client_id),
                client_public_key: base64_encode(&self.client_public_key),
                client_nonce: base64_encode(&self.client_nonce),
                client_ephemeral: base64_encode(&self.ephemeral_public),
                pairing_proof: base64_encode(&hmac_sha256(&token, &transcript)),
            }
        }

        fn finish(
            self,
            server: &ServerHello,
            pending: PendingProvider,
        ) -> (EstablishedProvider, FramedSession) {
            self.finish_with_proof(server, pending, None)
                .expect("handshake confirmation")
        }

        fn finish_with_proof(
            self,
            server: &ServerHello,
            pending: PendingProvider,
            proof_override: Option<[u8; 32]>,
        ) -> Result<(EstablishedProvider, FramedSession), ProviderError> {
            let salt = base64_decode(&server.salt)?;
            let mut salt_array = [0_u8; 32];
            salt_array.copy_from_slice(&salt);
            let server_ephemeral = base64_decode(&server.server_ephemeral)?;
            let shared = p256_agree(
                self.ephemeral.expect("client ephemeral consumed once"),
                &server_ephemeral,
            )?;
            let keys = derive_keys(&shared, &salt_array, &self.token);
            let mut client = FramedSession::client(keys.c2s, keys.s2c);
            let confirm: ProviderMessage = client.decode_json(pending.server_confirm_frame())?;
            match confirm {
                ProviderMessage::ServerConfirm {
                    protocol,
                    session_id,
                    proof,
                } => {
                    assert_eq!(protocol, 2);
                    assert_eq!(session_id, pending.session_id());
                    assert_eq!(
                        proof,
                        base64_encode(&hmac_sha256(&keys.server_confirm, pending.transcript()))
                    );
                }
                other => panic!("expected server_confirm, got {other:?}"),
            }
            let proof = proof_override
                .unwrap_or_else(|| hmac_sha256(&keys.client_confirm, pending.transcript()));
            let frame = client.encode_json(&ProviderMessage::ClientConfirm {
                proof: base64_encode(&proof),
            })?;
            let established = pending.accept_client_confirm(&frame)?;
            Ok((established, client))
        }
    }

    #[test]
    fn full_handshake_and_encrypted_round_trip() {
        let token = [7_u8; 32];
        let (server, hello) = ServerHandshake::new(&token).expect("server handshake");
        let client = TestClient::new(token);
        let client_hello = client.hello(&hello);
        let pending = server.accept(&client_hello).expect("accept client");
        let (mut provider, mut client_session) = client.finish(&hello, pending);
        let manifest = sample_manifest();
        let manifest_frame = client_session
            .encode_json(&ProviderMessage::Manifest(manifest.clone()))
            .expect("manifest frame");
        let accepted = provider.accept_manifest(&manifest_frame).expect("manifest");
        assert_eq!(accepted.provider_id, manifest.provider_id);
        let ack_frame = provider.manifest_ack_frame().expect("ack frame");
        let ack: ProviderMessage = client_session.decode_json(&ack_frame).expect("ack");
        assert_eq!(
            ack,
            ProviderMessage::ManifestAck {
                provider_id: manifest.provider_id.clone(),
                accepted: true,
            }
        );
        let invoke_frame = provider
            .start_invoke("tabs.read.list", &json!({}), 30_000)
            .expect("invoke");
        let invoke: ProviderMessage = client_session.decode_json(&invoke_frame).expect("invoke");
        let (invoke_id, invoke_action) = match invoke {
            ProviderMessage::Invoke {
                id, action, args, ..
            } => {
                assert_eq!(args, json!({}));
                (id, action)
            }
            other => panic!("expected invoke, got {other:?}"),
        };
        assert_eq!(invoke_action, "tabs.read.list");
        let result_frame = client_session
            .encode_json(&ProviderMessage::Result {
                id: invoke_id.clone(),
                ok: true,
                value: Some(json!({"tabs": [{"tab_id": "t1"}]})),
                error: None,
            })
            .expect("result frame");
        let result = provider.accept_result(&result_frame).expect("result");
        assert_eq!(result.id, invoke_id);
        assert!(result.ok);
        assert_eq!(result.value, Some(json!({"tabs": [{"tab_id": "t1"}]})));
    }

    #[test]
    fn wrong_token_is_rejected_before_a_manifest() {
        let (server, hello) = ServerHandshake::new(&[1_u8; 32]).expect("server handshake");
        let client = TestClient::new([2_u8; 32]);
        let error = server
            .accept(&client.hello(&hello))
            .err()
            .expect("wrong token");
        assert_eq!(error, ProviderError::Authentication);
        assert_eq!(error.close_code(), 4401);
        assert!(!error.to_string().contains(&base64_encode(&[1_u8; 32])));
    }

    #[test]
    fn wrong_protocol_version_is_rejected() {
        let token = [3_u8; 32];
        let (server, hello) = ServerHandshake::new(&token).expect("server handshake");
        let client = TestClient::new(token);
        let error = server
            .accept(&client.hello_with(&hello, token, 1))
            .err()
            .expect("wrong version");
        assert_eq!(error, ProviderError::Version(1));
        assert_eq!(error.close_code(), 4403);
    }

    #[test]
    fn oversized_handshake_is_rejected_before_parsing() {
        let token = [4_u8; 32];
        let (server, _hello) = ServerHandshake::new(&token).expect("server handshake");
        let error = server
            .accept_raw(&vec![b' '; crate::MAX_HANDSHAKE_BYTES + 1])
            .err()
            .expect("oversized handshake");
        assert_eq!(error, ProviderError::TooLarge);
        assert_eq!(error.close_code(), 1013);
    }

    #[test]
    fn tampered_ciphertext_is_rejected() {
        let token = [5_u8; 32];
        let (server, hello) = ServerHandshake::new(&token).expect("server handshake");
        let client = TestClient::new(token);
        let pending = server.accept(&client.hello(&hello)).expect("accept client");
        let (mut provider, mut client_session) = client.finish(&hello, pending);
        let frame = client_session
            .encode_json(&ProviderMessage::Manifest(sample_manifest()))
            .expect("manifest frame");
        let mut value: Value = serde_json::from_slice(&frame).expect("outer frame");
        let ciphertext = value
            .get("ciphertext")
            .and_then(Value::as_str)
            .expect("ciphertext")
            .to_string();
        let mut bytes = base64_decode(&ciphertext).expect("ciphertext bytes");
        bytes[0] ^= 0x01;
        value["ciphertext"] = Value::String(base64_encode(&bytes));
        let tampered = serde_json::to_vec(&value).expect("tampered frame");
        let error = provider
            .accept_manifest(&tampered)
            .expect_err("tampered manifest");
        assert_eq!(error, ProviderError::Authentication);
        assert_eq!(error.close_code(), 4401);
    }

    fn direct_sessions() -> (FramedSession, FramedSession) {
        let keys = derive_keys(&[9_u8; 32], &[8_u8; 32], &[7_u8; 32]);
        (
            FramedSession::client(keys.c2s, keys.s2c),
            FramedSession::server(keys.c2s, keys.s2c),
        )
    }

    #[test]
    fn replayed_sequence_is_rejected() {
        let (mut client, mut server) = direct_sessions();
        let frame = client
            .encode_json(&ProviderMessage::Ping {
                nonce: "one".into(),
            })
            .expect("frame");
        let _: ProviderMessage = server.decode_json(&frame).expect("first frame");
        let error = server
            .decode_json::<ProviderMessage>(&frame)
            .expect_err("replayed frame");
        assert_eq!(error, ProviderError::Sequence);
        assert_eq!(error.close_code(), 4402);
    }

    #[test]
    fn sequence_gap_is_rejected() {
        let (mut client, mut server) = direct_sessions();
        let _first = client
            .encode_json(&ProviderMessage::Ping {
                nonce: "one".into(),
            })
            .expect("first frame");
        let second = client
            .encode_json(&ProviderMessage::Ping {
                nonce: "two".into(),
            })
            .expect("second frame");
        let error = server
            .decode_json::<ProviderMessage>(&second)
            .expect_err("gap");
        assert_eq!(error, ProviderError::Sequence);
    }

    #[test]
    fn oversized_plaintext_is_rejected() {
        let (mut client, _server) = direct_sessions();
        let value = json!({
            "kind": "ping",
            "nonce": "a".repeat(MAX_PLAINTEXT_BYTES),
        });
        let error = client
            .encode_value(&value)
            .expect_err("oversized plaintext");
        assert_eq!(error, ProviderError::TooLarge);
        assert_eq!(error.close_code(), 1013);
    }

    #[test]
    fn unknown_inner_kind_is_rejected() {
        let keys = derive_keys(&[1_u8; 32], &[2_u8; 32], &[3_u8; 32]);
        let mut client = FramedSession::client(keys.c2s, keys.s2c);
        let frame = client
            .encode_value(&json!({"kind": "bogus"}))
            .expect("frame");
        let mut server = ProviderSession::new(FramedSession::server(keys.c2s, keys.s2c));
        let error = server.receive_message(&frame).expect_err("unknown kind");
        assert_eq!(error.close_code(), 4404);
        assert!(error.to_string().contains("unknown provider message kind"));
    }

    #[test]
    fn confirmation_proof_mismatch_is_rejected() {
        let token = [6_u8; 32];
        let (server, hello) = ServerHandshake::new(&token).expect("server handshake");
        let client = TestClient::new(token);
        let pending = server.accept(&client.hello(&hello)).expect("accept client");
        let error = client
            .finish_with_proof(&hello, pending, Some([0_u8; 32]))
            .err()
            .expect("wrong confirmation proof");
        assert_eq!(error, ProviderError::Authentication);
        assert_eq!(error.close_code(), 4401);
    }

    #[test]
    fn frame_aad_direction_cannot_be_swapped() {
        let keys = derive_keys(&[4_u8; 32], &[5_u8; 32], &[6_u8; 32]);
        let mut client = FramedSession::client(keys.c2s, keys.s2c);
        let frame = client
            .encode_json(&ProviderMessage::Ping {
                nonce: "direction".into(),
            })
            .expect("frame");
        let mut wrong = FramedSession::new(
            keys.s2c,
            keys.c2s,
            Direction::ServerToClient,
            Direction::ServerToClient,
        );
        let error = wrong
            .decode_json::<ProviderMessage>(&frame)
            .expect_err("direction swap must fail");
        assert_eq!(error, ProviderError::Authentication);
    }
}
