/** Typed UI Port RPC shared by popup, options and the background worker. */

import type { CdpProbeResult } from '../cdp-endpoint';

export const UI_PORT_NAME = 'corvus.ui';

export interface CdpUiState {
  available: boolean;
  endpoint: string | null;
  webSocketDebuggerUrl: string | null;
  browser: string | null;
  browserFamily: string | null;
  expectedBrowserFamily: string | null;
  familyMatch: boolean | null;
  protocolVersion: string | null;
  source: 'auto' | 'configured';
  reason: string | null;
  detected_at: number | null;
}

export interface DebuggerChannelState {
  managed: true;
  attached_tabs: string[];
}

export interface PolicyDeniedUiState {
  code: 'POLICY_DENIED';
  reason: string | null;
}

export interface UiState {
  status: string;
  connected: boolean;
  browserId: string | null;
  daemonVersion: string | null;
  lastError: string | null;
  lastPolicyDenied?: PolicyDeniedUiState | null;
  wsUrl: string;
  cdpEndpoint: string;
  cdp: CdpUiState | null;
  version: string;
  capabilities: string[];
  debuggerChannel: DebuggerChannelState;
}

export interface UiRequestPayloads {
  'corvus.get_state': Record<string, never>;
  'corvus.cdp_refresh': Record<string, never>;
  'corvus.cdp_save': { endpoint: string };
  'corvus.cdp_reset': Record<string, never>;
  'corvus.daemon_save': { url: string };
  'corvus.daemon_reset': Record<string, never>;
  'corvus.reconnect': Record<string, never>;
}

export type UiRequestType = keyof UiRequestPayloads;

export interface UiPortRequest<K extends UiRequestType = UiRequestType> {
  id: string;
  type: K;
  payload: UiRequestPayloads[K];
}

export type UiReply = { ok: true; state: UiState } | { ok: false; error: string };

export type UiPortResponse = ({ id: string } & UiReply) | { id: string; ok: false; error: string };

export type UiMethod = keyof UiRequestPayloads;

export type CdpDescriptor = Pick<CdpProbeResult, 'available' | 'endpoint' | 'browser' | 'source' | 'reason'> & {
  webSocketDebuggerUrl: string | null;
  familyMatch: boolean | null;
};
