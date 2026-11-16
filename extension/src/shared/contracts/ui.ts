/** Typed UI Port RPC shared by popup, options and the provider-only service worker. */

export const UI_PORT_NAME = 'corvus.ui';

export interface ProviderUiCapability {
  id: string;
  version: number;
  actions: string[];
}

export interface ProviderUiBrowser {
  family: string;
  version: string;
  profile: string;
}

export interface ProviderUiState {
  status: 'pairing_required' | 'connecting' | 'connected' | 'disconnected' | 'error';
  connected: boolean;
  base_url: string;
  token_saved: boolean;
  provider_id: string | null;
  server_id: string | null;
  session_id: string | null;
  browser: ProviderUiBrowser | null;
  last_error: { code: string; message: string } | null;
  version: string;
  capabilities: ProviderUiCapability[];
}

export interface UiRequestPayloads {
  'corvus.get_state': Record<string, never>;
  'corvus.save_and_connect': { base_url: string; token?: string };
  'corvus.connect': Record<string, never>;
  'corvus.disconnect': Record<string, never>;
}

export type UiRequestType = keyof UiRequestPayloads;

export interface UiPortRequest<K extends UiRequestType = UiRequestType> {
  id: string;
  type: K;
  payload: UiRequestPayloads[K];
}

export type UiReply = { ok: true; state: ProviderUiState } | { ok: false; error: string };

export type UiPortResponse = ({ id: string } & UiReply) | { id: string; ok: false; error: string };

export type UiMethod = keyof UiRequestPayloads;
