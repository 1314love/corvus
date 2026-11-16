//! Application messages carried inside the encrypted provider session.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::error::ProviderError;

/// Browser metadata supplied in a provider manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserInfo {
    /// Browser family, for example "edge".
    pub family: String,
    /// Full browser version string.
    pub version: String,
    /// Profile label chosen by the extension.
    pub profile: String,
}

/// One capability advertised by a provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capability {
    /// Capability identifier such as "system.info".
    pub id: String,
    /// Capability schema version.
    pub version: u32,
    /// Actions implemented for this capability.
    pub actions: Vec<String>,
}

/// Provider limits applied to one invoke.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    /// Maximum serialized invoke arguments in bytes.
    pub max_invoke_bytes: u64,
    /// Maximum serialized result value in bytes.
    pub max_result_bytes: u64,
}

/// Provider manifest accepted after confirmation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Stable provider identifier of the form provider-&lt;uuid&gt;.
    pub provider_id: String,
    /// Provider implementation version.
    pub version: String,
    /// Browser metadata.
    pub browser: BrowserInfo,
    /// Advertised capabilities.
    pub capabilities: Vec<Capability>,
    /// Requested event subscriptions.
    pub subscriptions: Vec<String>,
    /// Provider limits.
    pub limits: Limits,
}

impl Manifest {
    /// Validate every field that the gateway relies on.
    pub fn validate(&self) -> Result<(), ProviderError> {
        if self.version != "2.0.0" {
            return Err(ProviderError::BadArgs(
                "manifest version must be 2.0.0".into(),
            ));
        }
        let Some(uuid) = self.provider_id.strip_prefix("provider-") else {
            return Err(ProviderError::BadArgs(
                "provider_id must start with provider-".into(),
            ));
        };
        if Uuid::parse_str(uuid).is_err() {
            return Err(ProviderError::BadArgs(
                "provider_id must contain a UUID".into(),
            ));
        }
        if self.browser.family.is_empty()
            || self.browser.family.len() > 32
            || self
                .browser
                .family
                .chars()
                .any(|character| !character.is_ascii_alphanumeric())
        {
            return Err(ProviderError::BadArgs(
                "browser.family must be a short alphanumeric name".into(),
            ));
        }
        if self.browser.version.is_empty() || self.browser.version.len() > 64 {
            return Err(ProviderError::BadArgs(
                "browser.version must be 1..64 characters".into(),
            ));
        }
        if self.browser.profile.is_empty() || self.browser.profile.len() > 64 {
            return Err(ProviderError::BadArgs(
                "browser.profile must be 1..64 characters".into(),
            ));
        }
        if self.capabilities.is_empty() || self.capabilities.len() > 32 {
            return Err(ProviderError::BadArgs(
                "manifest must advertise 1..32 capabilities".into(),
            ));
        }
        let mut capability_ids = Vec::new();
        for capability in &self.capabilities {
            if !valid_capability_id(&capability.id) {
                return Err(ProviderError::BadArgs("capability id is invalid".into()));
            }
            if capability.version == 0 {
                return Err(ProviderError::BadArgs(
                    "capability version must be positive".into(),
                ));
            }
            if capability.actions.is_empty() || capability.actions.len() > 32 {
                return Err(ProviderError::BadArgs(
                    "capability must advertise 1..32 actions".into(),
                ));
            }
            if capability_ids.contains(&capability.id) {
                return Err(ProviderError::BadArgs(
                    "capability ids must be unique".into(),
                ));
            }
            capability_ids.push(capability.id.clone());
            let mut actions = Vec::new();
            for action in &capability.actions {
                if action.is_empty()
                    || action.len() > 64
                    || !action.chars().all(|character| {
                        character.is_ascii_lowercase()
                            || character.is_ascii_digit()
                            || character == '_'
                    })
                {
                    return Err(ProviderError::BadArgs(
                        "capability action is invalid".into(),
                    ));
                }
                if actions.contains(action) {
                    return Err(ProviderError::BadArgs(
                        "capability actions must be unique".into(),
                    ));
                }
                actions.push(action.clone());
            }
        }
        if self.subscriptions.len() > 8 {
            return Err(ProviderError::BadArgs(
                "at most 8 subscriptions are allowed".into(),
            ));
        }
        let mut subscriptions = Vec::new();
        for topic in &self.subscriptions {
            if topic != "tabs.updated" {
                return Err(ProviderError::BadArgs(
                    "unknown provider subscription".into(),
                ));
            }
            if subscriptions.contains(topic) {
                return Err(ProviderError::BadArgs(
                    "subscriptions must be unique".into(),
                ));
            }
            subscriptions.push(topic.clone());
        }
        if self.limits.max_invoke_bytes == 0 || self.limits.max_invoke_bytes > 16 * 1024 * 1024 {
            return Err(ProviderError::BadArgs(
                "max_invoke_bytes must be 1..16777216".into(),
            ));
        }
        if self.limits.max_result_bytes == 0 || self.limits.max_result_bytes > 24 * 1024 * 1024 {
            return Err(ProviderError::BadArgs(
                "max_result_bytes must be 1..25165824".into(),
            ));
        }
        Ok(())
    }

    /// Find the action inside the advertised capabilities.
    pub fn resolve_action<'a>(
        &'a self,
        action: &'a str,
    ) -> Result<(&'a Capability, &'a str), ProviderError> {
        let mut best: Option<(&Capability, &str)> = None;
        for capability in &self.capabilities {
            let prefix = format!("{}.", capability.id);
            if let Some(rest) = action.strip_prefix(&prefix) {
                if best
                    .as_ref()
                    .map(|(current, _)| capability.id.len() > current.id.len())
                    .unwrap_or(true)
                {
                    best = Some((capability, rest));
                }
            }
        }
        let Some((capability, rest)) = best else {
            return Err(ProviderError::CapabilityUnavailable(action.to_string()));
        };
        if !capability.actions.iter().any(|known| known == rest) {
            return Err(ProviderError::BadArgs(format!(
                "unknown action {action} on {}",
                capability.id
            )));
        }
        Ok((capability, rest))
    }
}

fn valid_capability_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '.'
        })
        && !value.starts_with('.')
        && !value.ends_with('.')
        && !value.contains("..")
}

/// Error object returned inside a provider result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderErrorBody {
    /// Stable machine code.
    pub code: String,
    /// Human-readable message that must not contain secrets.
    pub message: String,
    /// Whether the invoke may be retried.
    pub retryable: bool,
}

/// Confirmation acknowledgement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestAck {
    /// Provider identifier being acknowledged.
    pub provider_id: String,
    /// Whether the manifest was accepted.
    pub accepted: bool,
}

/// One decrypted provider result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderResult {
    /// Invoke identifier this result answers.
    pub id: String,
    /// Whether the invoke succeeded.
    pub ok: bool,
    /// Result value on success.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
    /// Error object on failure.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ProviderErrorBody>,
}

/// Every message carried inside the encrypted provider session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProviderMessage {
    /// Server proof sent at s2c sequence zero.
    ServerConfirm {
        /// Provider protocol version.
        protocol: u16,
        /// Opaque session identifier.
        session_id: String,
        /// HMAC proof over the transcript.
        proof: String,
    },
    /// Client proof sent at c2s sequence zero.
    ClientConfirm {
        /// HMAC proof over the transcript.
        proof: String,
    },
    /// Provider manifest.
    Manifest(Manifest),
    /// Manifest acknowledgement.
    ManifestAck {
        /// Provider identifier being acknowledged.
        provider_id: String,
        /// Whether the manifest was accepted.
        accepted: bool,
    },
    /// One invoke request.
    Invoke {
        /// Unique invoke identifier.
        id: String,
        /// Action in capability.action form.
        action: String,
        /// Action arguments.
        args: Value,
        /// Caller deadline in milliseconds.
        deadline_ms: u64,
    },
    /// One invoke result.
    Result {
        /// Invoke identifier this result answers.
        id: String,
        /// Whether the invoke succeeded.
        ok: bool,
        /// Result value on success.
        #[serde(skip_serializing_if = "Option::is_none")]
        value: Option<Value>,
        /// Error object on failure.
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<ProviderErrorBody>,
    },
    /// Subscription request.
    Subscribe {
        /// Request identifier.
        id: String,
        /// Requested topics.
        topics: Vec<String>,
    },
    /// Subscription acknowledgement.
    SubscriptionAck {
        /// Request identifier.
        id: String,
        /// Accepted topics.
        topics: Vec<String>,
    },
    /// One provider event.
    Event {
        /// Event topic.
        topic: String,
        /// Event sequence number.
        seq: u64,
        /// Event payload.
        payload: Value,
    },
    /// Cancel an in-flight invoke.
    Cancel {
        /// Invoke identifier.
        id: String,
    },
    /// Liveness request.
    Ping {
        /// Opaque nonce echoed by pong.
        nonce: String,
    },
    /// Liveness response.
    Pong {
        /// Opaque nonce from ping.
        nonce: String,
    },
    /// Protocol-level error.
    Error {
        /// Optional invoke identifier.
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// Stable machine code.
        code: String,
        /// Human-readable message.
        message: String,
    },
}

impl ProviderMessage {
    /// Stable wire kind.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::ServerConfirm { .. } => "server_confirm",
            Self::ClientConfirm { .. } => "client_confirm",
            Self::Manifest(_) => "manifest",
            Self::ManifestAck { .. } => "manifest_ack",
            Self::Invoke { .. } => "invoke",
            Self::Result { .. } => "result",
            Self::Subscribe { .. } => "subscribe",
            Self::SubscriptionAck { .. } => "subscription_ack",
            Self::Event { .. } => "event",
            Self::Cancel { .. } => "cancel",
            Self::Ping { .. } => "ping",
            Self::Pong { .. } => "pong",
            Self::Error { .. } => "error",
        }
    }
}
