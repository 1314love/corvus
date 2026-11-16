/** Service layer: the only place UI code talks to the background worker. */

import type { UiRequestPayloads, UiRequestType, UiReply } from '../../../shared/contracts/ui';
import { uiPortClient } from './client';

export function request<K extends UiRequestType>(type: K, payload: UiRequestPayloads[K]): Promise<UiReply> {
  return uiPortClient.request(type, payload as unknown as Record<string, unknown>);
}

export const uiService = {
  getState: () => request('corvus.get_state', {}),
  saveAndConnect: (baseUrl: string, token?: string) =>
    request('corvus.save_and_connect', token ? { base_url: baseUrl, token } : { base_url: baseUrl }),
  connect: () => request('corvus.connect', {}),
  disconnect: () => request('corvus.disconnect', {}),
};

export function errorMessage(error: unknown): string {
  if (error instanceof Error && error.message) return error.message;
  if (typeof error === 'string') return error;
  return 'Request failed';
}
