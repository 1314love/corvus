/** Typed chrome.runtime.Port client for the UI projection RPC. */

import { UI_PORT_NAME, type UiPortResponse, type UiReply, type UiRequestType } from '../../../shared/contracts/ui';

const REQUEST_TIMEOUT_MS = 8000;

interface PendingRequest {
  resolve: (reply: UiReply) => void;
  reject: (error: Error) => void;
  timer: ReturnType<typeof setTimeout>;
}

function asError(value: unknown): Error {
  if (value instanceof Error) return value;
  return new Error(typeof value === 'string' ? value : 'UI request failed');
}

class UiPortClient {
  private port: chrome.runtime.Port | null = null;
  private pending = new Map<string, PendingRequest>();
  private nextId = 1;

  private ensurePort(): chrome.runtime.Port {
    if (this.port) return this.port;

    const port = chrome.runtime.connect({ name: UI_PORT_NAME });
    this.port = port;

    port.onMessage.addListener((message: unknown) => {
      this.onMessage(message);
    });

    port.onDisconnect.addListener(() => {
      if (this.port === port) this.port = null;
      const detail = chrome.runtime.lastError?.message;
      this.rejectAll(detail ? 'UI channel closed: ' + detail : 'UI channel closed');
    });

    return port;
  }

  private onMessage(message: unknown): void {
    if (!message || typeof message !== 'object') return;
    const response = message as UiPortResponse;
    if (typeof response.id !== 'string' || !response.id) return;

    const pending = this.pending.get(response.id);
    if (!pending) return;
    this.pending.delete(response.id);
    clearTimeout(pending.timer);

    if (response.ok) {
      pending.resolve({ ok: true, state: response.state });
    } else {
      pending.reject(new Error(response.error));
    }
  }

  private rejectAll(message: string): void {
    const pending = Array.from(this.pending.values());
    this.pending.clear();
    for (const item of pending) {
      clearTimeout(item.timer);
      item.reject(new Error(message));
    }
  }

  request(type: UiRequestType, payload: Record<string, unknown>): Promise<UiReply> {
    return new Promise((resolve, reject) => {
      let port: chrome.runtime.Port;
      try {
        port = this.ensurePort();
      } catch (error) {
        reject(asError(error));
        return;
      }

      const id = String(this.nextId++);
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error('UI request timed out'));
      }, REQUEST_TIMEOUT_MS);

      this.pending.set(id, { resolve, reject, timer });

      try {
        port.postMessage({ id, type, payload });
      } catch (error) {
        this.pending.delete(id);
        clearTimeout(timer);
        this.port = null;
        reject(asError(error));
      }
    });
  }
}

export const uiPortClient = new UiPortClient();
