'use strict';

/**
 * The only Corvus interface: one switch and one status dot.
 *
 * The service worker owns all state; this page only mirrors it from
 * chrome.storage and asks the service worker to change the debugger channel.
 */

const CONNECTION_KEY = 'corvus.connection';
const DEBUGGER_KEY = 'corvus.debugger';

const switchElement = document.getElementById('debugger-switch');
const dotElement = document.getElementById('status-dot');
const statusElement = document.getElementById('status-text');

function render(connection, debuggerEnabled) {
  const status = connection && typeof connection.status === 'string' ? connection.status : 'disconnected';
  const connected = status === 'connected';
  dotElement.className = connected ? 'dot dot-on' : 'dot dot-off';
  dotElement.title = connected ? 'Connected' : 'Disconnected';
  statusElement.textContent = connected ? 'Connected' : (status === 'connecting' ? 'Connecting' : 'Disconnected');
  statusElement.title = connection && connection.lastError ? String(connection.lastError) : '';
  switchElement.checked = debuggerEnabled === true;
}

async function refresh() {
  try {
    const stored = await chrome.storage.local.get([CONNECTION_KEY, DEBUGGER_KEY]);
    render(stored[CONNECTION_KEY], stored[DEBUGGER_KEY]);
  } catch (error) {
    render({ status: 'disconnected', lastError: String(error && error.message ? error.message : error) }, switchElement.checked);
  }
}

async function commitDebuggerSwitch() {
  const requested = switchElement.checked;
  switchElement.disabled = true;
  try {
    const reply = await chrome.runtime.sendMessage({ type: 'corvus.set_debugger', enabled: requested });
    if (reply && reply.ok === false) switchElement.checked = !requested;
  } catch (error) {
    switchElement.checked = !requested;
  } finally {
    switchElement.disabled = false;
    await refresh();
  }
}

switchElement.addEventListener('change', () => { void commitDebuggerSwitch(); });
chrome.storage.onChanged.addListener((changes, areaName) => {
  if (areaName !== 'local') return;
  if (changes[CONNECTION_KEY] || changes[DEBUGGER_KEY]) void refresh();
});

void refresh();
