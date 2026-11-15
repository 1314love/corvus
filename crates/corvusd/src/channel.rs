//! Browser channel selection between the extension transport and CDP.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use corvus_cdp::{CdpClient, CDP_TOOL_NAMES};
use corvus_proto::CdpDescriptor;
use corvus_transport::Registry;
use serde_json::{json, Value};

/// Which transport carries browser tools.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ChannelMode {
    /// Prefer CDP when a connection exists, otherwise use the extension.
    #[default]
    Auto,
    /// Require CDP and never fall back to the extension.
    Cdp,
    /// Ignore CDP and always use the extension.
    Extension,
}

impl ChannelMode {
    /// Stable lowercase name used by the CLI and tools.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Cdp => "cdp",
            Self::Extension => "extension",
        }
    }

    /// Parse a mode name.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "cdp" => Some(Self::Cdp),
            "extension" => Some(Self::Extension),
            _ => None,
        }
    }
}

/// Shared channel selection and the transports it chooses between.
pub struct ChannelState {
    mode: RwLock<ChannelMode>,
    cdp: Arc<CdpClient>,
    registry: Arc<Registry>,
    cdp_enabled: bool,
    descriptor_error: RwLock<Option<String>>,
    mismatch_logged: AtomicBool,
}

impl ChannelState {
    /// Build the state around a CDP client and the browser registry.
    pub fn new(
        mode: ChannelMode,
        cdp: Arc<CdpClient>,
        registry: Arc<Registry>,
        cdp_enabled: bool,
    ) -> Self {
        Self {
            mode: RwLock::new(mode),
            cdp,
            registry,
            cdp_enabled,
            descriptor_error: RwLock::new(None),
            mismatch_logged: AtomicBool::new(false),
        }
    }

    /// Current channel mode.
    pub fn mode(&self) -> ChannelMode {
        self.mode.read().map(|guard| *guard).unwrap_or_default()
    }

    /// Change the mode.
    pub fn set_mode(&self, mode: ChannelMode) -> Result<(), String> {
        if mode == ChannelMode::Cdp && !self.cdp_enabled {
            return Err("CDP is disabled for this daemon".into());
        }
        *self
            .mode
            .write()
            .map_err(|_| "channel state is poisoned".to_string())? = mode;
        Ok(())
    }

    /// The CDP client.
    pub fn cdp(&self) -> &Arc<CdpClient> {
        &self.cdp
    }

    /// Record why the latest advertised CDP descriptor was rejected.
    pub fn note_descriptor_error(&self, message: Option<String>) {
        if let Ok(mut guard) = self.descriptor_error.write() {
            *guard = message;
        }
    }

    /// Latest descriptor rejection, if any.
    pub fn descriptor_error(&self) -> Option<String> {
        self.descriptor_error
            .read()
            .ok()
            .and_then(|guard| guard.clone())
    }

    /// Most recent CDP descriptor advertised by any connected extension.
    pub fn descriptor(&self) -> Option<CdpDescriptor> {
        self.descriptor_with_family()
            .map(|(descriptor, _)| descriptor)
    }

    /// Descriptor together with the browser family of the extension that
    /// advertised it. The profile token is intentionally not part of this
    /// comparison: it is a random identifier, not a browser family.
    pub fn descriptor_with_family(&self) -> Option<(CdpDescriptor, Option<String>)> {
        self.registry.snapshot().into_iter().find_map(|browser| {
            browser
                .cdp
                .map(|descriptor| (descriptor, browser.browser_family))
        })
    }

    /// Channel, CDP state and extension state in one document.
    pub fn status(&self) -> Value {
        let browsers = self.registry.snapshot();
        let browser_ids: Vec<String> = browsers.iter().map(|browser| browser.id.clone()).collect();
        let mut document = json!({
            "channel": self.mode().as_str(),
            "cdp_enabled": self.cdp_enabled,
            "cdp": self.cdp.status(),
            "extension": {
                "connected": !browsers.is_empty(),
                "browsers": browser_ids,
            },
            "cdp_tools": CDP_TOOL_NAMES,
            "descriptor": self.descriptor(),
        });
        if let Some(error) = self.descriptor_error() {
            document["descriptor_error"] = json!(error);
        }
        if let Some((descriptor, family)) = self.descriptor_with_family() {
            if family_mismatch(&descriptor, family.as_deref()) {
                document["browser_mismatch"] = json!(true);
                if !self.mismatch_logged.swap(true, Ordering::SeqCst) {
                    tracing::warn!(
                        extension_family = family.as_deref().unwrap_or("unknown"),
                        descriptor_family = ?descriptor.browser_family,
                        "explicit CDP endpoint belongs to a different browser family"
                    );
                }
            }
        }
        document
    }

    /// Handle the browser_channel tool.
    pub fn tool(&self, arguments: &Value) -> Result<Value, corvus_proto::ErrorBody> {
        let action = arguments
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or("status");
        match action {
            "status" => Ok(self.status()),
            "set" => {
                let requested = arguments
                    .get("channel")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        corvus_proto::ErrorBody::new(
                            corvus_proto::ErrorCode::BadArgs,
                            "browser_channel set requires a channel argument",
                        )
                    })?;
                let mode = ChannelMode::parse(requested).ok_or_else(|| {
                    corvus_proto::ErrorBody::new(
                        corvus_proto::ErrorCode::BadArgs,
                        format!("unknown channel {requested}"),
                    )
                    .with_details(json!({ "channel": requested }))
                })?;
                self.set_mode(mode).map_err(|message| {
                    corvus_proto::ErrorBody::new(corvus_proto::ErrorCode::BadArgs, message)
                })?;
                Ok(self.status())
            }
            other => Err(corvus_proto::ErrorBody::new(
                corvus_proto::ErrorCode::BadArgs,
                format!("unknown browser_channel action {other}"),
            )),
        }
    }
}

/// Whether two browser family names share one brand family.
///
/// Chrome and Chromium are one Chromium brand family. Every branded Chromium
/// browser (Edge, Brave, Opera, Vivaldi) is distinct. Unknown or empty names
/// carry no claim and therefore never conflict.
fn families_match(expected: &str, actual: &str) -> bool {
    let expected = expected.to_ascii_lowercase();
    let actual = actual.to_ascii_lowercase();
    if expected.is_empty()
        || actual.is_empty()
        || expected == "unknown"
        || actual == "unknown"
        || expected == "none"
        || actual == "none"
    {
        return true;
    }
    if expected == actual {
        return true;
    }
    matches!(expected.as_str(), "chrome" | "chromium")
        && matches!(actual.as_str(), "chrome" | "chromium")
}

/// Whether the descriptor names a different browser family than its extension.
///
/// `extension_family` comes from the hello handshake, never from the profile
/// token. Missing or unknown families never claim a conflict.
pub fn family_mismatch(descriptor: &CdpDescriptor, extension_family: Option<&str>) -> bool {
    let expected = extension_family.unwrap_or("unknown");
    descriptor
        .browser_family
        .as_deref()
        .is_some_and(|family| !families_match(expected, family))
}

/// Validate an extension-advertised CDP descriptor before the daemon uses it.
///
/// Both URLs must be loopback and the advertised browser family must match the
/// extension that reported it. A mismatch never takes over extension tools.
pub fn validate_descriptor(
    descriptor: &CdpDescriptor,
    extension_family: Option<&str>,
) -> Result<String, String> {
    if !descriptor.available {
        return Err(descriptor
            .reason
            .clone()
            .unwrap_or_else(|| "extension reports no CDP endpoint".to_string()));
    }
    if let Some(endpoint) = descriptor.endpoint.as_deref() {
        validate_url(endpoint, &["http", "https", "ws", "wss"])?;
    }
    if let Some(ws) = descriptor.web_socket_debugger_url.as_deref() {
        validate_url(ws, &["ws", "wss"])?;
    }
    let expected = extension_family.unwrap_or("unknown").to_ascii_lowercase();
    if descriptor.source.as_deref() == Some("auto") && family_mismatch(descriptor, extension_family)
    {
        let family = descriptor.browser_family.as_deref().unwrap_or("unknown");
        return Err(format!(
            "descriptor browser_family {family} does not match extension browser family {expected}"
        ));
    }
    if let Some(ws) = descriptor.web_socket_debugger_url.as_deref() {
        return Ok(ws.to_string());
    }
    if let Some(endpoint) = descriptor.endpoint.as_deref() {
        if endpoint.starts_with("ws://") || endpoint.starts_with("wss://") {
            return Ok(endpoint.to_string());
        }
    }
    Err("descriptor has no WebSocket debugger URL".to_string())
}

fn validate_url(url: &str, schemes: &[&str]) -> Result<(), String> {
    let (scheme, rest) = url
        .split_once("://")
        .ok_or_else(|| format!("CDP URL {url} has no scheme"))?;
    if !schemes.contains(&scheme.to_ascii_lowercase().as_str()) {
        return Err(format!("CDP URL {url} uses an unsupported scheme"));
    }
    let authority = rest.split('/').next().unwrap_or(rest);
    let host_part = authority.rsplit('@').next().unwrap_or(authority);
    let host = if let Some(stripped) = host_part.strip_prefix('[') {
        stripped
            .split_once(']')
            .map(|(host, _)| host)
            .ok_or_else(|| format!("CDP URL {url} has an invalid host"))?
    } else {
        host_part.split(':').next().unwrap_or(host_part)
    };
    if !matches!(
        host.to_ascii_lowercase().as_str(),
        "127.0.0.1" | "::1" | "localhost"
    ) {
        return Err(format!("CDP URL {url} is not a loopback endpoint"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_validation_rejects_remote_and_mismatched_endpoints() {
        let mut descriptor = CdpDescriptor {
            available: true,
            web_socket_debugger_url: Some("ws://127.0.0.1:9222/devtools/browser/1".into()),
            browser_family: Some("edge".into()),
            ..Default::default()
        };
        assert_eq!(
            validate_descriptor(&descriptor, Some("edge")).unwrap(),
            "ws://127.0.0.1:9222/devtools/browser/1"
        );
        descriptor.browser_family = Some("chrome".into());
        assert!(validate_descriptor(&descriptor, Some("edge")).is_ok());
        descriptor.source = Some("auto".into());
        assert!(validate_descriptor(&descriptor, Some("edge")).is_err());
        descriptor.source = None;
        descriptor.browser_family = Some("edge".into());
        descriptor.web_socket_debugger_url = Some("ws://192.0.2.10:9222/devtools/browser/1".into());
        assert!(validate_descriptor(&descriptor, Some("edge")).is_err());
        descriptor.web_socket_debugger_url = Some("ws://[::1]:9222/devtools/browser/1".into());
        assert!(validate_descriptor(&descriptor, Some("edge")).is_ok());
        descriptor.available = false;
        assert!(validate_descriptor(&descriptor, Some("edge")).is_err());

        descriptor.available = true;
        descriptor.web_socket_debugger_url = None;
        descriptor.endpoint = Some("http://127.0.0.1:9222".into());
        assert!(validate_descriptor(&descriptor, Some("edge")).is_err());
        descriptor.web_socket_debugger_url = Some("ws://127.0.0.1:9222/devtools/browser/9".into());
        assert_eq!(
            validate_descriptor(&descriptor, Some("edge")).unwrap(),
            "ws://127.0.0.1:9222/devtools/browser/9"
        );
        descriptor.endpoint = Some("http://127.0.0.1:9222".into());
        assert_eq!(
            validate_descriptor(&descriptor, Some("edge")).unwrap(),
            "ws://127.0.0.1:9222/devtools/browser/9"
        );
    }

    #[test]
    fn family_comparison_treats_chrome_and_chromium_as_one_brand() {
        let mut descriptor = CdpDescriptor {
            available: true,
            web_socket_debugger_url: Some("ws://127.0.0.1:9222/devtools/browser/1".into()),
            browser_family: Some("chrome".into()),
            source: Some("auto".into()),
            ..Default::default()
        };
        assert_eq!(
            validate_descriptor(&descriptor, Some("chromium")).unwrap(),
            "ws://127.0.0.1:9222/devtools/browser/1"
        );
        assert!(!family_mismatch(&descriptor, Some("chromium")));
        descriptor.browser_family = Some("chromium".into());
        assert!(validate_descriptor(&descriptor, Some("chrome")).is_ok());

        descriptor.browser_family = Some("edge".into());
        assert!(validate_descriptor(&descriptor, Some("chrome")).is_err());
        assert!(validate_descriptor(&descriptor, Some("chromium")).is_err());
        descriptor.browser_family = Some("unknown".into());
        assert!(validate_descriptor(&descriptor, Some("edge")).is_ok());
    }

    #[test]
    fn auto_family_uses_the_handshake_family_not_the_profile_token() {
        let mut descriptor = CdpDescriptor {
            available: true,
            web_socket_debugger_url: Some("ws://127.0.0.1:9222/devtools/browser/1".into()),
            browser_family: Some("chrome".into()),
            source: Some("auto".into()),
            ..Default::default()
        };
        // A random profile token must not make a same-family endpoint fail.
        assert!(!family_mismatch(&descriptor, Some("chrome")));
        assert!(validate_descriptor(&descriptor, Some("chrome")).is_ok());

        assert!(family_mismatch(&descriptor, Some("edge")));
        assert!(validate_descriptor(&descriptor, Some("edge")).is_err());

        // Configured endpoints stay usable across families but are reported.
        descriptor.source = Some("configured".into());
        assert!(validate_descriptor(&descriptor, Some("edge")).is_ok());
        assert!(family_mismatch(&descriptor, Some("edge")));
    }

    #[test]
    fn mode_names_round_trip() {
        for mode in [ChannelMode::Auto, ChannelMode::Cdp, ChannelMode::Extension] {
            assert_eq!(ChannelMode::parse(mode.as_str()), Some(mode));
        }
        assert_eq!(ChannelMode::parse("CDP"), Some(ChannelMode::Cdp));
        assert_eq!(ChannelMode::parse("other"), None);
    }
}
