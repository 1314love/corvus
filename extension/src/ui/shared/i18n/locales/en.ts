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
    refreshStatus: 'Refresh status',
    save: 'Save',
    connect: 'Connect',
    disconnect: 'Disconnect',
    saveAndConnect: 'Save and connect',
    language: 'Language',
    english: 'English',
    chinese: '中文',
    notConnected: 'not connected',
    none: 'none',
    unknown: 'unknown',
  },
  status: {
    connected: 'connected',
    connecting: 'connecting',
    disconnected: 'disconnected',
    pairing_required: 'pairing required',
    error: 'error',
    unknown: 'unknown',
  },
  options: {
    pairing: 'Local pairing',
    pairingHelp: 'Enter the loopback address of the local service. The default is ',
    baseUrl: 'Local address',
    browser: 'Browser',
    token: 'Pairing token',
    tokenPresent: 'saved',
    tokenMissing: 'not saved',
    tokenSaved: 'Token saved. Enter a new token to replace it.',
    tokenPlaceholder: 'Paste the 43-character pairing token',
    status: 'Status',
    service: 'Service',
    lastError: 'Last error',
  },
  notice: {
    saved: 'Saved. Connecting...',
    connecting: 'Connecting...',
    disconnected: 'Disconnected.',
  },
  validation: {
    base: 'Enter a loopback ws:// URL, for example ws://127.0.0.1:7330.',
    token: 'Enter the 43-character pairing token.',
  },
};

export default en;
