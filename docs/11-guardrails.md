# Guardrails policy

[English](11-guardrails.md) | [中文](11-guardrails.zh.md)

Status: the v1.6.0 guardrails implementation is present in the tree and passed formal QA on the working tree at HEAD `61ca85f` (237 workspace tests, 137 conformance checks, 55 Vitest tests in ten files and Playwright 1/1; one Low `browser_type` channel finding, no High or Medium). No v1.6.0 tag, CI, published package or published asset exists; 1.5.0 remains the current published release. Audience: operators and MCP/REST client authors.

## 1. Overview

Corvus can load one optional local policy file at daemon start. The policy restricts which tools may run, restricts navigation to selected hosts, blocks literal private-network targets, redacts cookie values, and marks page content as untrusted. It is opt-in: with no policy file, the daemon behaves as before.

The policy is enforced by `corvusd` before a tool call is dispatched and again on navigation results. It does not replace authentication, network egress controls or operating-system isolation.

## 2. Policy file and schema

The file must be valid JSON, at most 65,536 bytes, and an object with no unknown fields. `version` is required and must be exactly 1. All other fields are optional.

| Field | Type | Default | Meaning |
|---|---|---|---|
| `version` | integer | required | Must be 1. |
| `read_only` | boolean | `false` | Deny every tool whose catalogue entry is not read-only. |
| `allow_tools` | string array | `[]` | When non-empty, only these tool names are allowed unless the read-only rule or deny list decides first. |
| `deny_tools` | string array | `[]` | These tool names are always denied. |
| `allowed_hosts` | string array | `[]` | When non-empty, navigation must target an http or https host matching an entry. |
| `block_private_network` | boolean | `false` | Deny literal loopback, private, link-local and local-name hosts. |
| `redact_secrets` | boolean | `false` | Redact cookie values in `browser_cookies` results. |
| `content_boundary` | boolean | `false` | Wrap untrusted page text and HTML in marker lines. |

An empty `allow_tools` means no allowlist restriction. An empty `allowed_hosts` means no host restriction. Tool names must exist in the 43-tool catalogue; unknown names reject startup. A name may not appear in both `allow_tools` and `deny_tools`.

A host entry is either an exact lowercase host such as `example.com` or a leading wildcard suffix such as `*.example.com`. Patterns reject whitespace, `/`, `:`, `?`, `#`, `@`, an embedded `*`, a leading or trailing dot, and double dots; characters are limited to ASCII letters, digits, dot, hyphen and underscore. Ports are ignored. `*.example.com` matches the apex `example.com` and its subdomains, but not `api.example.com.evil.test`.

## 3. Startup and validation

Pass the file with `--guardrails <PATH>` or set `CORVUS_GUARDRAILS=<PATH>`. The command-line option wins when both are present.

```bash
corvusd serve --guardrails /etc/corvus/guardrails.json
```

Any load or validation error is fatal: the daemon refuses to start and reports an invalid-input error. The exact failure classes are:

- unreadable file or I/O error;
- file larger than 65,536 bytes;
- invalid JSON, an unknown field, or a wrong `version`;
- an unknown tool name, the same tool in both allow and deny lists, or an invalid host pattern.

At startup the daemon logs whether guardrails are enabled, together with list counts and the switch states. It never logs the policy body.

## 4. Precedence

For a tool call the checks run in this order, and the first match returns the denial:

1. **Read-only.** If `read_only` is true and the tool's catalogue entry is not read-only, deny with reason `read_only`.
2. **Allow list.** If `allow_tools` is non-empty and the tool is absent, deny with reason `allowlist`.
3. **Deny list.** If `deny_tools` contains the tool, deny with reason `deny_list`.

Because startup rejects allow/deny overlap, the order only matters when a non-read-only tool is allowlisted while `read_only` is also true: the read-only rule wins.

The URL gate is separate from tool precedence. It runs on arguments before dispatch for `browser_navigate` and `tab_new`, and on the returned `url` after a successful `browser_navigate`, `tab_new` or `browser_history`.

## 5. Worked examples

### 5.1 Read-only

A read-only daemon for observation and testing:

```json
{
  "version": 1,
  "read_only": true
}
```

Every action tool is denied before dispatch with reason `read_only`; tools that the catalogue classifies as read-only, such as snapshots and page text reads, remain available. The policy does not make a read-only tool's result safe to trust.

### 5.2 Host allowlist and private-network block

Limit navigation to two domains and reject local targets:

```json
{
  "version": 1,
  "allowed_hosts": ["example.com", "*.example.org"],
  "block_private_network": true
}
```

`https://example.com/a` and `https://docs.example.org/b` are allowed. `https://api.example.com.evil.test/` is denied with `host_not_allowed` because the suffix trick does not match `example.com`. `http://127.0.0.1:8080/`, `http://localhost/`, `http://[::1]/` and private IPv4 or IPv6 literals are denied with `private_network`. `about:blank` is always allowed.

When `allowed_hosts` is non-empty, a non-http(s) URL is denied. A navigation that redirects to a denied host is caught by the post-dispatch URL check and returns `POLICY_DENIED` with reason `redirect_not_allowed`; the browser may already have followed the redirect.

### 5.3 Untrusted content and redaction

Mark page output as data and hide cookie values:

```json
{
  "version": 1,
  "redact_secrets": true,
  "content_boundary": true
}
```

With `redact_secrets`, every object in the top-level `cookies` array of a `browser_cookies` result that has a `value` field gets `value: "[redacted]"` and `redacted: true`. Objects without `value` are unchanged. No other tool's values and no generic secret scanning are covered.

With `content_boundary`, `page_text` output under `text`, `page_html` output under `html`, and compact `page_snapshot` output under `text` are wrapped in marker lines. JSON snapshots are untouched. The markers are applied after navigation-result checks and cookie redaction.

## 6. Enforcement and errors

`RouterBackend::call` in `crates/corvusd/src/lib.rs` checks the tool policy and then the navigation arguments before dispatch. A denied call is recorded and returns an error without reaching the browser. `browser_batch` checks each step, applies each result, and stops on the first denied step by default; a batch cannot bypass the policy for a nested step. `session_replay` checks each planned or live step and records a denied replay step as skipped with reason `policy_denied`.

A denial is the 11th error code, `POLICY_DENIED`. It is not retryable. MCP returns one error text line with `isError: true` and no `structuredContent`; details are embedded as compact JSON:

```text
POLICY_DENIED: tool browser_navigate denied by guardrails policy (allowlist) {"reason":"allowlist","tool":"browser_navigate","policy":"/etc/corvus/guardrails.json"}
```

REST returns HTTP 403 with the same error body nested under `error`:

```json
{"error":{"code":"POLICY_DENIED","message":"navigation denied by guardrails policy (host_not_allowed)","retryable":false,"details":{"reason":"host_not_allowed","url":"https://example.net/","policy":"/etc/corvus/guardrails.json"}}}
```

URL denials use the message `navigation denied by guardrails policy (<reason>)` and carry `details.reason`, `details.url` and `details.policy`. Tool denials use `tool <name> denied by guardrails policy (<reason>)` and carry `details.reason`, `details.tool` and `details.policy`. A post-dispatch denial still returns `POLICY_DENIED`, but the navigation has already happened.

## 7. Redaction and content boundaries

Cookie redaction is deliberately narrow. Only `browser_cookies` is processed, only the top-level `cookies` array is inspected, and only objects that already contain `value` are changed. Other tools, nested values, headers and page text are not redacted.

The content boundary uses these exact markers:

```text
<<<CORVUS UNTRUSTED PAGE CONTENT>>>
<<<END CORVUS UNTRUSTED PAGE CONTENT>>>
```

If the payload already contains either marker, it is first escaped to `<<<CORVUS_UNTRUSTED_PAGE_CONTENT_ESCAPED>>>` or `<<<END_CORVUS_UNTRUSTED_PAGE_CONTENT_ESCAPED>>>`. The final value has exactly one leading start marker and one trailing end marker, with the payload between them.

Password masking is independent of guardrails and always on for snapshots: password input values render as `***`. `browser_type` differs by channel: the extension returns `masked: true` for password fields and a truncated `value` otherwise, while CDP omits `value` and does not emit `masked`, so no typed value is returned. JSON snapshots are still not boundary-wrapped by design.

## 8. Health

When enabled, the daemon `/health` document contains a `guardrails` object under `health`. REST `/api/v1/health` returns `{ok, version, health: {..., guardrails: {...}}}`. The object has fixed keys and counts, never tool names or page data:

```json
"guardrails": {
  "enabled": true,
  "read_only": false,
  "allow_tools": 0,
  "deny_tools": 3,
  "allowed_hosts": 2,
  "block_private_network": true,
  "redact_secrets": true,
  "content_boundary": true
}
```

When no policy is configured, `enabled` is false and the three list-count fields are 0; the four switches are false. The health object is safe to expose to a local monitoring system.

## 9. Limits

- The navigation gate is not a network sandbox. After an allowed page loads, page scripts can still fetch denied origins, and subresources are not filtered by `allowed_hosts`. Use egress controls for network isolation.
- DNS is never resolved by the gate. A hostname that passes the pattern can resolve to a private address, and DNS rebinding is not detected.
- `browser_eval`, `page_html` and `browser_history` can expose secrets even when `redact_secrets` is on. Redaction covers only the cookie values described above.
- `page_html` returns pasted attributes as they are; pasted HTML attributes are not scrubbed.
- JSON snapshots are not boundary-wrapped; only compact snapshot text, `page_text` text and `page_html` html are.
- REST auth accounts and refresh tokens are in memory; guardrails do not add persistent identity or authentication.
- `details.policy` exposes the local policy path in daemon responses. The extension UI hides it, but direct MCP/REST clients receive it.
- A denied `browser_batch` step is an error and stops the batch by default. `session_replay` records a denied step as skipped instead of executing it.
- Read-only is a catalogue classification. Check [09-tools.md](09-tools.md) for a tool's current classification before relying on it.
- No live v1.6.0 deployment is claimed, and NFR-3 remains unpassed.

## 10. Related documents

- [09-tools.md](09-tools.md) for tool inputs, results and error codes.
- [02-architecture.md](02-architecture.md) for the enforcement point and daemon structure.
- [05-deployment.md](05-deployment.md) for service configuration and health checks.
- [release-notes/v1.6.0.md](release-notes/v1.6.0.md) for the pending release evidence.
