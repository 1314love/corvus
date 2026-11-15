/** Service-worker entry: legacy runtime owner plus the typed UI Port RPC. */

import { handleUiMessage } from './runtime';
import { UI_PORT_NAME, type UiPortRequest, type UiReply } from '../shared/contracts/ui';
import { describeError } from '../shared/errors';

const UI_REQUEST_TYPES = new Set<string>([
  'corvus.get_state',
  'corvus.cdp_refresh',
  'corvus.cdp_save',
  'corvus.cdp_reset',
  'corvus.daemon_save',
  'corvus.daemon_reset',
  'corvus.reconnect',
]);

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function isUiRequest(value: unknown): value is UiPortRequest {
  if (!isRecord(value)) return false;
  if (typeof value.id !== 'string' || !value.id) return false;
  if (typeof value.type !== 'string' || !UI_REQUEST_TYPES.has(value.type)) return false;
  return value.payload === undefined || isRecord(value.payload);
}

function dispatch(request: UiPortRequest): Promise<UiReply> {
  const payload = isRecord(request.payload) ? request.payload : {};
  return handleUiMessage({ ...payload, type: request.type }) as Promise<UiReply>;
}

chrome.runtime.onConnect.addListener((port) => {
  if (port.name !== UI_PORT_NAME) return;

  port.onMessage.addListener((message: unknown) => {
    if (!isUiRequest(message)) {
      port.postMessage({ id: '', ok: false, error: 'malformed ui request' });
      return;
    }

    void dispatch(message)
      .then((reply) => {
        port.postMessage({ id: message.id, ...reply });
      })
      .catch((error: unknown) => {
        port.postMessage({ id: message.id, ok: false, error: describeError(error) });
      });
  });
});
