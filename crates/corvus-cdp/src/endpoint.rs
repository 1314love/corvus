//! Loopback-only endpoint validation.

use corvus_proto::CdpDescriptor;

use crate::CdpError;

/// Whether the host part of an endpoint is a loopback name or address.
pub fn is_loopback_host(host: &str) -> bool {
    matches!(
        host.trim_matches(['[', ']']).to_ascii_lowercase().as_str(),
        "127.0.0.1" | "::1" | "0:0:0:0:0:0:0:1" | "localhost"
    )
}

/// Validate a browser-level WebSocket debugger URL and return it unchanged.
///
/// Only ws:// and wss:// URLs whose host is loopback are accepted. A remote
/// endpoint is rejected even when it would otherwise parse.
pub fn validate_ws_endpoint(raw: &str) -> Result<String, CdpError> {
    let raw = raw.trim();
    let (scheme, rest) = raw
        .split_once("://")
        .ok_or_else(|| CdpError::InvalidEndpoint(format!("{raw} has no scheme")))?;
    if !matches!(scheme.to_ascii_lowercase().as_str(), "ws" | "wss") {
        return Err(CdpError::InvalidEndpoint(format!(
            "{raw} must use ws:// or wss://"
        )));
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.is_empty() || authority.contains('@') {
        return Err(CdpError::InvalidEndpoint(format!(
            "{raw} has no usable authority"
        )));
    }
    let host = if let Some(stripped) = authority.strip_prefix('[') {
        let end = stripped
            .find(']')
            .ok_or_else(|| CdpError::InvalidEndpoint(format!("{raw} has a malformed IPv6 host")))?;
        &stripped[..end]
    } else {
        authority.split(':').next().unwrap_or_default()
    };
    if !is_loopback_host(host) {
        return Err(CdpError::InvalidEndpoint(format!(
            "{raw} is not a loopback endpoint"
        )));
    }
    Ok(raw.to_string())
}

/// Validate the discovery endpoint when one is present.
pub fn validate_http_endpoint(raw: &str) -> Result<(), CdpError> {
    let raw = raw.trim();
    let (scheme, rest) = raw
        .split_once("://")
        .ok_or_else(|| CdpError::InvalidEndpoint(format!("{raw} has no scheme")))?;
    if !matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https") {
        return Err(CdpError::InvalidEndpoint(format!(
            "{raw} must use http:// or https://"
        )));
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = if let Some(stripped) = authority.strip_prefix('[') {
        let end = stripped
            .find(']')
            .ok_or_else(|| CdpError::InvalidEndpoint(format!("{raw} has a malformed IPv6 host")))?;
        &stripped[..end]
    } else {
        authority.split(':').next().unwrap_or_default()
    };
    if !is_loopback_host(host) {
        return Err(CdpError::InvalidEndpoint(format!(
            "{raw} is not a loopback endpoint"
        )));
    }
    Ok(())
}

/// Validate a descriptor advertised during the extension handshake.
pub fn validate_descriptor(descriptor: &CdpDescriptor) -> Result<(), CdpError> {
    if !descriptor.available {
        return Ok(());
    }
    let websocket = descriptor
        .web_socket_debugger_url
        .as_deref()
        .ok_or_else(|| {
            CdpError::InvalidEndpoint("a CDP descriptor needs a websocket URL".into())
        })?;
    validate_ws_endpoint(websocket)?;
    if let Some(endpoint) = descriptor.endpoint.as_deref() {
        validate_http_endpoint(endpoint)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_websocket_endpoints_are_accepted() {
        for endpoint in [
            "ws://127.0.0.1:9222/devtools/browser/abc",
            "wss://localhost:9222/devtools/browser/abc",
            "ws://[::1]:9222/devtools/browser/abc",
        ] {
            assert!(validate_ws_endpoint(endpoint).is_ok(), "{endpoint}");
        }
    }

    #[test]
    fn remote_or_non_websocket_endpoints_are_rejected() {
        for endpoint in [
            "ws://192.168.1.2:9222/devtools/browser/abc",
            "ws://example.test:9222/devtools/browser/abc",
            "http://127.0.0.1:9222/json/version",
            "127.0.0.1:9222",
            "ws://127.0.0.1:9222@evil.test/x",
        ] {
            assert!(validate_ws_endpoint(endpoint).is_err(), "{endpoint}");
        }
    }

    #[test]
    fn descriptors_require_a_loopback_websocket_url() {
        let valid = CdpDescriptor {
            available: true,
            endpoint: Some("http://127.0.0.1:9222".into()),
            web_socket_debugger_url: Some("ws://127.0.0.1:9222/devtools/browser/abc".into()),
            ..CdpDescriptor::default()
        };
        assert!(validate_descriptor(&valid).is_ok());
        let mut remote = valid.clone();
        remote.web_socket_debugger_url = Some("ws://10.0.0.5:9222/x".into());
        assert!(validate_descriptor(&remote).is_err());
        let unavailable = CdpDescriptor::default();
        assert!(validate_descriptor(&unavailable).is_ok());
    }
}
