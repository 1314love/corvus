'use strict';

/**
 * Read-only Corvus status card.
 *
 * The service worker owns all state. This page only renders a snapshot: the
 * connection dot and state, the assigned browser id, the extension version, the
 * advertised capabilities and one line explaining the debugger channel. It has
 * no controls and never changes state.
 */

const CONNECTION_KEY = 'corvus.connection';
const DEFAULT_CAPABILITIES = 'dom, scripting, debugger';

const dotElement = document.getElementById('status-dot');
const statusElement = document.getElementById('status-text');
const browserIdElement = document.getElementById('browser-id');
const versionElement = document.getElementById('extension-version');
const capabilitiesElement = document.getElementById('capabilities');
const debuggerNoteElement = document.getElementById('debugger-note');

function text(value, fallback) {
  return typeof value === 'string' && value.length > 0 ? value : fallback;
}

function render(uiState, connection) {
  const state = uiState && typeof uiState === 'object' ? uiState : {};
  const stored = connection && typeof connection === 'object' ? connection : {};
  const status = text(state.status, text(stored.status, 'disconnected'));
  const connected = status === 'connected';
  const connecting = status === 'connecting';

  dotElement.className = connected ? 'dot dot-on' : 'dot dot-off';
  dotElement.title = connected ? 'Connected' : (connecting ? 'Connecting' : 'Disconnected');
  statusElement.textContent = connected ? 'Connected' : (connecting ? 'Connecting' : 'Disconnected');
  statusElement.title = text(state.lastError, text(stored.lastError, ''));

  browserIdElement.textContent = text(state.browserId, text(stored.browserId, 'not assigned'));
  versionElement.textContent = text(state.version, 'unknown');

  const capabilities = Array.isArray(state.capabilities) && state.capabilities.length > 0
    ? state.capabilities.join(', ')
    : DEFAULT_CAPABILITIES;
  capabilitiesElement.textContent = capabilities;
  capabilitiesElement.title = capabilities;

  debuggerNoteElement.textContent = 'The debugger channel is managed automatically by the service worker.';
}

async function refresh() {
  let uiState = null;
  try {
    const reply = await chrome.runtime.sendMessage({ type: 'corvus.get_state' });
    if (reply && reply.ok === true && reply.state && typeof reply.state === 'object') uiState = reply.state;
  } catch (error) {
    // The service worker can be unavailable during an extension reload; the
    // stored connection snapshot still gives the popup something to show.
  }

  let connection = null;
  try {
    const stored = await chrome.storage.local.get(CONNECTION_KEY);
    connection = stored[CONNECTION_KEY] || null;
  } catch (error) {
    connection = null;
  }

  render(uiState, connection);
}

chrome.storage.onChanged.addListener((changes, areaName) => {
  if (areaName !== 'local') return;
  if (changes[CONNECTION_KEY]) void refresh();
});

void refresh();
