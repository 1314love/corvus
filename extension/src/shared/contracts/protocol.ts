/** Wire contracts for daemon, offscreen document and content-script messages. */

import type { ToolErrorPayload } from '../errors';

export const PROTOCOL_VERSION = 1;
export const OFFSCREEN_TARGET = 'offscreen';

export interface HelloArgs {
  profile: string;
  version: string;
  capabilities: string[];
  cdp?: WireCdpDescriptor;
}

export interface WireCdpDescriptor {
  available: boolean;
  endpoint: string | null;
  web_socket_debugger_url: string | null;
  browser: string | null;
  browser_family: string | null;
  protocol_version: string | null;
  source: 'auto' | 'configured';
  reason: string | null;
}

export interface DaemonHelloFrame {
  v: number;
  id: string;
  kind: 'hello';
  browser: string;
  args: HelloArgs;
}

export interface DaemonCommandFrame {
  v: number;
  id: string;
  kind: 'command';
  browser?: string;
  session?: string;
  tool: string;
  args: Record<string, unknown>;
  meta?: { trace_id?: string };
  timeout_ms?: number;
}

export interface DaemonResponseFrame {
  v: number;
  id: string;
  kind: 'response';
  browser?: string;
  session?: string;
  tool?: string;
  args: unknown;
  meta?: { trace_id?: string };
  error?: ToolErrorPayload;
}

export interface DaemonEventFrame {
  v: number;
  id: string;
  kind: 'event';
  browser?: string;
  session?: string;
  tool?: string;
  args: Record<string, unknown>;
}

export interface DaemonHelloAckFrame {
  v: number;
  id: string;
  kind: 'hello_ack';
  browser?: string;
  args?: Record<string, unknown>;
}

export interface DaemonPingFrame {
  v: number;
  id: string;
  kind: 'ping' | 'pong';
  browser?: string;
}

export type DaemonFrame =
  | DaemonHelloFrame
  | DaemonCommandFrame
  | DaemonResponseFrame
  | DaemonEventFrame
  | DaemonHelloAckFrame
  | DaemonPingFrame;

export interface OffscreenConnectMessage {
  target: typeof OFFSCREEN_TARGET;
  type: 'corvus.connect';
  url: string;
  reason?: string;
}

export interface OffscreenSendMessage {
  target: typeof OFFSCREEN_TARGET;
  type: 'corvus.send';
  frame: DaemonFrame;
}

export interface OffscreenStatusRequest {
  target: typeof OFFSCREEN_TARGET;
  type: 'corvus.status_request';
}

export type BackgroundToOffscreen = OffscreenConnectMessage | OffscreenSendMessage | OffscreenStatusRequest;

export interface OffscreenStatusMessage {
  type: 'status';
  status: 'connecting' | 'connected' | 'disconnected';
  url?: string;
  lastError?: string | null;
}

export interface OffscreenSocketOpenMessage {
  type: 'socket_open';
  url?: string;
}

export interface OffscreenKeepaliveMessage {
  type: 'keepalive';
}

export interface OffscreenFrameMessage {
  type: 'frame';
  frame: DaemonFrame;
}

export type OffscreenToBackground =
  OffscreenStatusMessage | OffscreenSocketOpenMessage | OffscreenKeepaliveMessage | OffscreenFrameMessage;

export interface ContentCommandMessage {
  type: 'corvus.command';
  id: string;
  tool: string;
  args: Record<string, unknown>;
  meta?: { trace_id?: string };
  timeout_ms?: number;
}

export interface ContentUploadTargetMessage {
  type: 'corvus.upload_target';
  id: string;
  target: string;
}

export type BackgroundToContent = ContentCommandMessage | ContentUploadTargetMessage;

export type ContentReply = { ok: true; result: unknown } | { ok: false; error: ToolErrorPayload };
