'use strict';

/**
 * Corvus options page.
 *
 * The daemon URL lives in chrome.storage.local under CorvusDaemonUrl.STORAGE_KEY.
 * This page validates it, saves it and asks the service worker to reconnect.
 * It owns no protocol state; the popup remains read-only.
 */

const DAEMON = self.CorvusDaemonUrl;
const DEFAULT_DAEMON_URL = DAEMON.DEFAULT_DAEMON_URL;
const CONNECTION_KEY = 'corvus.connection';

const form = document.getElementById('daemon-form');
const urlInput = document.getElementById('daemon-url');
const saveButton = document.getElementById('save');
const resetButton = document.getElementById('reset');
const validationElement = document.getElementById('validation');
const statusDot = document.getElementById('status-dot');
const statusText = document.getElementById('status-text');

let initialized = false;
let busy = false;

function text(value, fallback) {
  return typeof value === 'string' && value.length > 0 ? value : fallback;
}

function setValidation(message, kind) {
  validationElement.textContent = message || '';
  validationElement.className = kind ? 'hint ' + kind : 'hint';
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

  if (initialized) return;
  const current = uiState && typeof uiState.wsUrl === 'string'
    ? uiState.wsUrl
    : await DAEMON.readDaemonUrl();
  urlInput.value = current;
  initialized = true;
}

function setBusy(value) {
  busy = value;
  saveButton.disabled = value;
  resetButton.disabled = value;
  urlInput.disabled = value;
}

async function requestReconnect() {
  try {
    await chrome.runtime.sendMessage({ type: 'corvus.reconnect' });
  } catch (error) {
    // The worker's storage listener also reconnects after the write.
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
    setValidation('Could not save the URL: ' + (error && error.message ? error.message : String(error)), 'error');
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
    setValidation('Could not reset the URL: ' + (error && error.message ? error.message : String(error)), 'error');
  } finally {
    setBusy(false);
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

  if (changes[CONNECTION_KEY]) void refreshStatus();
});

void refreshStatus();
