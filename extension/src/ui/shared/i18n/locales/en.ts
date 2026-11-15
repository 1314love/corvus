/**
 * English UI message catalog.
 *
 * Every visible popup/options string lives here. Keep the key set in sync with
 * zh-CN.ts; tests/contract/i18n-catalogs.test.ts enforces parity.
 */

export type MessageCatalog = Record<string, unknown>;

const en: MessageCatalog = {
  app: {
    title: 'Corvus',
    popupTitle: 'Corvus',
    optionsTitle: 'Corvus settings',
    version: 'Corvus {version}',
  },
  common: {
    settings: 'Settings',
    refresh: 'Refresh',
    save: 'Save',
    resetToDefault: 'Reset to default',
    resetToAuto: 'Reset to auto',
    reconnect: 'Reconnect',
    detectNow: 'Detect now',
    refreshStatus: 'Refresh status',
    daemon: 'Daemon',
    browser: 'Browser',
    cdp: 'CDP',
    language: 'Language',
    english: 'English',
    chinese: '中文',
    notConnected: 'not connected',
    notDetected: 'not detected',
    available: 'Available',
    unavailable: 'Unavailable',
    yes: 'yes',
    no: 'no',
    none: 'none',
    unknown: 'unknown',
    expected: 'expected',
  },
  status: {
    connected: 'connected',
    connecting: 'connecting',
    disconnected: 'disconnected',
    degraded: 'degraded',
    unknown: 'unknown',
  },
  options: {
    daemonConnection: 'Daemon connection',
    daemonHelpBefore: 'The offscreen document connects to this WebSocket URL. The local default is ',
    daemonHelpAfter: '.',
    browserCdpEndpoint: 'Browser CDP endpoint',
    cdpHelpBefore: 'Leave empty or use ',
    cdpHelpMid: ' to probe the local ports 9222, 9223 and 9333. Detection only reads ',
    cdpHelpAfter: '; it never attaches the debugger.',
    endpoint: 'Endpoint',
    protocol: 'Protocol',
    source: 'Source',
    browserFamily: 'Browser family',
    reason: 'Reason',
    status: 'Status',
    daemonUrl: 'Daemon URL',
    browserId: 'Browser id',
    daemonVersion: 'Daemon version',
    capabilities: 'Capabilities',
    lastError: 'Last error',
  },
  family: {
    match: 'match',
    mismatch: 'mismatch',
    unknown: 'unknown',
  },
  source: {
    auto: 'auto',
    configured: 'configured',
    unknown: 'unknown',
  },
  notice: {
    daemonSaved: 'Saved. Reconnecting...',
    daemonReset: 'Reset to the local default. Reconnecting...',
    cdpSaved: 'CDP setting saved. Detection refreshed.',
    cdpReset: 'CDP setting reset to auto.',
    cdpRefreshed: 'CDP detection refreshed.',
    reconnecting: 'Reconnecting...',
  },
  errors: {
    policyDenied: 'Blocked by daemon policy',
    policyDeniedWithReason: 'Blocked by daemon policy: {reason}',
  },
  validation: {
    daemon: 'Enter a valid daemon URL.',
    cdp: 'Enter auto or a loopback http:// URL.',
  },
};

export default en;
