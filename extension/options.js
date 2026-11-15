'use strict';

/**
 * @fileoverview Corvus options page.
 *
 * The daemon URL lives in chrome.storage.local under CorvusDaemonUrl.STORAGE_KEY.
 * The CDP endpoint lives under CorvusCdpEndpoint.STORAGE_KEY. This page validates
 * both, saves them and asks the service worker to reconnect or redetect.
 * It owns no protocol state; the popup remains read-only.
 */

const DAEMON = self.CorvusDaemonUrl;
const CDP = self.CorvusCdpEndpoint;
const DEFAULT_DAEMON_URL = DAEMON.DEFAULT_DAEMON_URL;
const CONNECTION_KEY = 'corvus.connection';

const form = document.getElementById('daemon-form');
const urlInput = document.getElementById('daemon-url');
const saveButton = document.getElementById('save');
const resetButton = document.getElementById('reset');
const validationElement = document.getElementById('validation');
const statusDot = document.getElementById('status-dot');
const statusText = document.getElementById('status-text');
const daemonUrlStatus = document.getElementById('daemon-url-status');

const cdpForm = document.getElementById('cdp-form');
const cdpInput = document.getElementById('cdp-endpoint');
const saveCdpButton = document.getElementById('save-cdp');
const resetCdpButton = document.getElementById('reset-cdp');
const detectCdpButton = document.getElementById('detect-cdp');
const cdpValidationElement = document.getElementById('cdp-validation');
const cdpStatusElement = document.getElementById('cdp-status');

let initialized = false;
let cdpInitialized = false;
let busy = false;
let cdpBusy = false;

function text(value, fallback) {
  return typeof value === 'string' && value.length > 0 ? value : fallback;
}

function errorMessage(error) {
  return error && error.message ? error.message : String(error);
}

function setValidation(message, kind) {
  validationElement.textContent = message || '';
  validationElement.className = kind ? 'hint ' + kind : 'hint';
}

function setCdpValidation(message, kind) {
  cdpValidationElement.textContent = message || '';
  cdpValidationElement.className = kind ? 'hint ' + kind : 'hint';
}

function renderStatus(uiState, connection) {
  const state = uiState && typeof uiState === 'object' ? uiState : {};
  const stored = connection && typeof connection === 'object' ? connection : {};
  const status = text(state.status, text(stored.status, 'disconnected'));
  const connected = status === 'connected';
  const connecting = status === 'connecting';

  statusDot.className = connected ? 'dot dot-on' : 'dot dot-off';
  statusDot.title = connected ? 'Connected' : (connecting ? 'Connecting' : 'Disconnected');
  statusText.textContent = connected ? 'Connected' : (connecting ? 'Connecting' : 'Disconnected');
  statusText.title = text(state.lastError, text(stored.lastError, ''));
}

function renderDaemonUrl(uiState) {
  const state = uiState && typeof uiState === 'object' ? uiState : {};
  const wsUrl = text(state.wsUrl, urlInput.value || DEFAULT_DAEMON_URL);
  const status = text(state.status, 'disconnected');
  daemonUrlStatus.textContent = 'Daemon URL: ' + wsUrl + ' (' + status + ')';
}

function renderCdp(uiState) {
  const state = uiState && typeof uiState === 'object' ? uiState : {};
  const cdp = state.cdp && typeof state.cdp === 'object' ? state.cdp : null;
  if (!cdp) {
    cdpStatusElement.textContent = 'Detection has not run yet.';
    cdpStatusElement.className = 'hint mono';
    return;
  }

  const source = text(cdp.source, 'auto');
  const endpoint = text(cdp.endpoint, source === 'configured' ? 'configured endpoint' : 'auto');
  const browserFamily = text(cdp.browserFamily, 'unknown browser family');
  const expectedFamily = text(cdp.expectedBrowserFamily, 'unknown browser family');
  if (cdp.available === true) {
    const browser = text(cdp.browser, 'unknown browser');
    const protocol = text(cdp.protocolVersion, 'unknown protocol');
    const family = cdp.browserFamily ? ' (' + browserFamily + ')' : '';
    cdpStatusElement.textContent = 'Detected ' + browser + family + ' at ' + endpoint
      + ' (CDP ' + protocol + ', ' + source + ').';
    if (cdp.familyMatch === false) {
      cdpStatusElement.textContent += ' Warning: this endpoint reports ' + browserFamily
        + ', but the extension is ' + expectedFamily
        + '; a configured endpoint is used even when the browser family differs.';
      cdpStatusElement.className = 'hint mono warn';
      return;
    }
    cdpStatusElement.className = 'hint mono ok';
    return;
  }

  const reason = text(cdp.reason, 'unavailable');
  cdpStatusElement.textContent = 'No CDP endpoint' + (cdp.endpoint ? ' at ' + cdp.endpoint : '') + ': ' + reason + ' (' + source + ').';
  cdpStatusElement.className = 'hint mono error';
}

async function getUiState() {
  try {
    const reply = await chrome.runtime.sendMessage({ type: 'corvus.get_state' });
    if (reply && reply.ok === true && reply.state && typeof reply.state === 'object') return reply.state;
  } catch (error) {
    // The service worker can be unavailable for a moment after a reload.
  }
  return null;
}

async function getStoredConnection() {
  try {
    const stored = await chrome.storage.local.get(CONNECTION_KEY);
    return stored[CONNECTION_KEY] || null;
  } catch (error) {
    return null;
  }
}

async function refreshStatus() {
  const uiState = await getUiState();
  const connection = await getStoredConnection();
  renderStatus(uiState, connection);
  renderDaemonUrl(uiState);
  renderCdp(uiState);

  if (!initialized) {
    const current = uiState && typeof uiState.wsUrl === 'string'
      ? uiState.wsUrl
      : await DAEMON.readDaemonUrl();
    urlInput.value = current;
    initialized = true;
  }

  if (!cdpInitialized) {
    const current = uiState && typeof uiState.cdpEndpoint === 'string'
      ? uiState.cdpEndpoint
      : await CDP.readCdpEndpoint();
    cdpInput.value = current;
    cdpInitialized = true;
  }
}

function setBusy(value) {
  busy = value;
  saveButton.disabled = value;
  resetButton.disabled = value;
  urlInput.disabled = value;
}

function setCdpBusy(value) {
  cdpBusy = value;
  saveCdpButton.disabled = value;
  resetCdpButton.disabled = value;
  detectCdpButton.disabled = value;
  cdpInput.disabled = value;
}

async function requestReconnect() {
  try {
    await chrome.runtime.sendMessage({ type: 'corvus.reconnect' });
  } catch (error) {
    // The worker's storage listener also reconnects after the write.
  }
}

async function requestCdp(message) {
  try {
    const reply = await chrome.runtime.sendMessage(message);
    if (reply && reply.ok === true && reply.state && typeof reply.state === 'object') return reply;
    return {
      ok: false,
      error: reply && reply.error ? reply.error : 'the service worker did not accept the change',
    };
  } catch (error) {
    return { ok: false, error: errorMessage(error) };
  }
}

async function save(event) {
  event.preventDefault();
  if (busy) return;

  const validation = DAEMON.validate(urlInput.value);
  if (!validation.ok) {
    setValidation(validation.error, 'error');
    urlInput.focus();
    return;
  }

  setBusy(true);
  try {
    const saved = await DAEMON.writeDaemonUrl(validation.value);
    urlInput.value = saved;
    setValidation('Saved. Reconnecting...', 'ok');
    await requestReconnect();
    await refreshStatus();
  } catch (error) {
    setValidation('Could not save the URL: ' + errorMessage(error), 'error');
  } finally {
    setBusy(false);
  }
}

async function reset() {
  if (busy) return;

  setBusy(true);
  try {
    const url = await DAEMON.resetDaemonUrl();
    urlInput.value = url;
    setValidation('Reset to default. Reconnecting...', 'ok');
    await requestReconnect();
    await refreshStatus();
  } catch (error) {
    setValidation('Could not reset the URL: ' + errorMessage(error), 'error');
  } finally {
    setBusy(false);
  }
}

async function saveCdp(event) {
  event.preventDefault();
  if (cdpBusy) return;

  const validation = CDP.validate(cdpInput.value);
  if (!validation.ok) {
    setCdpValidation(validation.error, 'error');
    cdpInput.focus();
    return;
  }

  setCdpBusy(true);
  try {
    const saved = await CDP.writeCdpEndpoint(validation.value);
    cdpInput.value = saved;
    setCdpValidation('Saved. Detecting...', 'ok');
    const reply = await requestCdp({ type: 'corvus.cdp_save', endpoint: saved });
    if (!reply.ok) throw new Error(reply.error);
    await refreshStatus();
    setCdpValidation('Saved.', 'ok');
  } catch (error) {
    setCdpValidation('Could not save the CDP endpoint: ' + errorMessage(error), 'error');
  } finally {
    setCdpBusy(false);
  }
}

async function resetCdp() {
  if (cdpBusy) return;

  setCdpBusy(true);
  try {
    const endpoint = await CDP.resetCdpEndpoint();
    cdpInput.value = endpoint;
    setCdpValidation('Reset to auto. Detecting...', 'ok');
    const reply = await requestCdp({ type: 'corvus.cdp_reset' });
    if (!reply.ok) throw new Error(reply.error);
    await refreshStatus();
    setCdpValidation('Reset to auto.', 'ok');
  } catch (error) {
    setCdpValidation('Could not reset the CDP endpoint: ' + errorMessage(error), 'error');
  } finally {
    setCdpBusy(false);
  }
}

async function detectCdp() {
  if (cdpBusy) return;

  setCdpBusy(true);
  try {
    setCdpValidation('Detecting...', '');
    const reply = await requestCdp({ type: 'corvus.cdp_refresh' });
    if (!reply.ok) throw new Error(reply.error);
    await refreshStatus();
    setCdpValidation('Detection complete.', 'ok');
  } catch (error) {
    setCdpValidation('Could not detect a CDP endpoint: ' + errorMessage(error), 'error');
  } finally {
    setCdpBusy(false);
  }
}

form.addEventListener('submit', (event) => {
  void save(event);
});

resetButton.addEventListener('click', () => {
  void reset();
});

urlInput.addEventListener('input', () => {
  if (!busy) setValidation('', '');
});

cdpForm.addEventListener('submit', (event) => {
  void saveCdp(event);
});

resetCdpButton.addEventListener('click', () => {
  void resetCdp();
});

detectCdpButton.addEventListener('click', () => {
  void detectCdp();
});

cdpInput.addEventListener('input', () => {
  if (!cdpBusy) setCdpValidation('', '');
});

chrome.storage.onChanged.addListener((changes, areaName) => {
  if (areaName !== 'local') return;

  const daemonChange = changes[DAEMON.STORAGE_KEY];
  if (daemonChange) {
    const next = daemonChange.newValue === undefined
      ? DEFAULT_DAEMON_URL
      : DAEMON.normalize(daemonChange.newValue);
    if (next && document.activeElement !== urlInput) urlInput.value = next;
    void refreshStatus();
  }

  const cdpChange = changes[CDP.STORAGE_KEY];
  if (cdpChange) {
    const next = cdpChange.newValue === undefined
      ? CDP.DEFAULT_CDP_ENDPOINT
      : CDP.normalize(cdpChange.newValue);
    if (next && document.activeElement !== cdpInput) cdpInput.value = next;
    void refreshStatus();
  }

  if (changes[CONNECTION_KEY]) void refreshStatus();
});

void refreshStatus();
