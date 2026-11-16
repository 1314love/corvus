/** Service-worker entry: provider-only runtime owner plus the typed UI Port RPC. */

import { handleProviderUiRequest, installProviderRuntime, type ProviderUiReply } from './provider-runtime';
import { UI_PORT_NAME, type UiPortRequest } from '../shared/contracts/ui';
import { describeError } from '../shared/errors';

const UI_REQUEST_TYPES = new Set<string>([
  'corvus.get_state',
  'corvus.save_and_connect',
  'corvus.connect',
  'corvus.disconnect',
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

function dispatch(request: UiPortRequest): Promise<ProviderUiReply> {
  const payload = isRecord(request.payload) ? request.payload : {};
  return handleProviderUiRequest({ kind: request.type.replace('corvus.', ''), ...payload });
}

installProviderRuntime();

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
