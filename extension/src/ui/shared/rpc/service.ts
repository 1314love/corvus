/** Service layer: the only place UI code talks to the background worker. */

import type { UiRequestPayloads, UiRequestType, UiReply } from '../../../shared/contracts/ui';
import { uiPortClient } from './client';

export function request<K extends UiRequestType>(type: K, payload: UiRequestPayloads[K]): Promise<UiReply> {
  return uiPortClient.request(type, payload as unknown as Record<string, unknown>);
}

export const uiService = {
  getState: () => request('corvus.get_state', {}),
  refreshCdp: () => request('corvus.cdp_refresh', {}),
  saveCdp: (endpoint: string) => request('corvus.cdp_save', { endpoint }),
  resetCdp: () => request('corvus.cdp_reset', {}),
  saveDaemon: (url: string) => request('corvus.daemon_save', { url }),
  resetDaemon: () => request('corvus.daemon_reset', {}),
  reconnect: () => request('corvus.reconnect', {}),
};

export function errorMessage(error: unknown): string {
  if (error instanceof Error && error.message) return error.message;
  if (typeof error === 'string') return error;
  return 'Request failed';
}
