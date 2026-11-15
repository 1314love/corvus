'use strict';

/**
 * @fileoverview Shared daemon WebSocket URL helper.
 *
 * Used by the service worker (via importScripts), the offscreen document, the
 * popup and the options page. Plain JavaScript, no build step.
 */

(() => {
  const DEFAULT_DAEMON_URL = 'ws://127.0.0.1:7330';
  const STORAGE_KEY = 'corvus.daemonUrl';

  /**
   * Validate a daemon URL and return a normalized value when it is usable.
   * Only ws:// and wss:// URLs with a host, an optional port and no path,
   * query string or fragment are accepted. A single trailing slash is allowed
   * and removed.
   */
  function validate(value) {
    if (typeof value !== 'string') {
      return { ok: false, error: 'Enter a WebSocket URL.' };
    }

    const text = value.trim();
    if (!text) {
      return { ok: false, error: 'Enter a WebSocket URL.' };
    }
    if (text.indexOf('?') !== -1 || text.indexOf('#') !== -1) {
      return { ok: false, error: 'Remove the query string and fragment; only ws://host[:port] is supported.' };
    }

    const authority = text.match(/^[a-z][a-z0-9+.-]*:\/\/([^/?#]*)/i);
    if (authority && authority[1].endsWith(':')) {
      return { ok: false, error: 'Enter a port number after the colon, or remove the colon.' };
    }

    let url;
    try {
      url = new URL(text);
    } catch (error) {
      return { ok: false, error: 'Enter a valid WebSocket URL.' };
    }

    if (url.protocol !== 'ws:' && url.protocol !== 'wss:') {
      return { ok: false, error: 'Use ws:// or wss://.' };
    }
    if (!url.hostname) {
      return { ok: false, error: 'The URL must include a host.' };
    }
    if (url.username || url.password) {
      return { ok: false, error: 'Remove the username and password from the URL.' };
    }
    if (url.pathname !== '/') {
      return { ok: false, error: 'Use ws://host[:port]; paths are not supported.' };
    }
    if (url.search || url.hash) {
      return { ok: false, error: 'Remove the query string and fragment; only ws://host[:port] is supported.' };
    }

    return { ok: true, value: url.protocol + '//' + url.host };
  }

  /** Normalize a URL, returning null when it is not acceptable. */
  function normalize(value) {
    const result = validate(value);
    return result.ok ? result.value : null;
  }

  function requireValid(value) {
    const result = validate(value);
    if (!result.ok) {
      const error = new Error(result.error);
      error.code = 'INVALID_DAEMON_URL';
      throw error;
    }
    return result.value;
  }

  /** Return the stored URL, or the local default when absent or invalid. */
  async function readDaemonUrl() {
    // Offscreen documents may not expose chrome.storage; treat a missing API
    // as no override and use the local default.
    if (typeof chrome === 'undefined' || !chrome.storage || !chrome.storage.local) {
      return DEFAULT_DAEMON_URL;
    }
    try {
      const stored = await chrome.storage.local.get(STORAGE_KEY);
      const normalized = normalize(stored && stored[STORAGE_KEY]);
      return normalized || DEFAULT_DAEMON_URL;
    } catch (error) {
      return DEFAULT_DAEMON_URL;
    }
  }

  /** Persist a normalized URL and return it. Throws when validation or storage fails. */
  async function writeDaemonUrl(value) {
    const normalized = requireValid(value);
    await chrome.storage.local.set({ [STORAGE_KEY]: normalized });
    return normalized;
  }

  /** Remove the stored override so reads fall back to the local default. */
  async function resetDaemonUrl() {
    await chrome.storage.local.remove(STORAGE_KEY);
    return DEFAULT_DAEMON_URL;
  }

  const api = Object.freeze({
    DEFAULT_DAEMON_URL,
    STORAGE_KEY,
    validate,
    normalize,
    readDaemonUrl,
    writeDaemonUrl,
    resetDaemonUrl,
  });

  const root = typeof self !== 'undefined' ? self : globalThis;
  root.CorvusDaemonUrl = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})();
