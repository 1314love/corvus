import type { Worker } from '@playwright/test';

const READY_POLL_MS = 150;
const CONNECT_RETRY_MS = 1500;

export interface OffscreenStatus {
  reachable: boolean;
  hasDocument: boolean;
  contextUrls: string[];
  status: string | null;
  lastError: string | null;
  attempt: number | null;
  url: string | null;
  error: string | null;
}

export interface OffscreenCreateResult {
  created: boolean;
  error: string | null;
}

export interface OffscreenConnectResult {
  ok: boolean;
  error: string | null;
}

function errorMessage(error: unknown): string {
  if (error instanceof Error && error.message) return error.message;
  return String(error);
}

export async function readOffscreenStatus(worker: Worker): Promise<OffscreenStatus> {
  return worker.evaluate(async () => {
    const runtime = chrome.runtime as unknown as {
      getContexts?: (filter: { contextTypes: string[] }) => Promise<Array<Record<string, unknown>>>;
      sendMessage: (message: unknown) => Promise<unknown>;
    };
    const offscreen = (chrome as unknown as { offscreen?: { hasDocument?: () => Promise<boolean> } }).offscreen;

    let hasDocument = false;
    let contextUrls: string[] = [];
    let contextError: string | null = null;

    try {
      if (offscreen && typeof offscreen.hasDocument === 'function') {
        hasDocument = Boolean(await offscreen.hasDocument());
      }
    } catch (error) {
      contextError = error instanceof Error && error.message ? error.message : String(error);
    }

    try {
      if (typeof runtime.getContexts === 'function') {
        const contexts = await runtime.getContexts({ contextTypes: ['OFFSCREEN_DOCUMENT'] });
        contextUrls = (contexts || []).map((context) =>
          String(context.documentUrl || context.contextType || context.tabId || ''),
        );
      }
    } catch (error) {
      if (!contextError) contextError = error instanceof Error && error.message ? error.message : String(error);
    }

    let response: Record<string, unknown> | null = null;
    let messageError: string | null = null;
    try {
      const raw = await Promise.race([
        runtime.sendMessage({ target: 'offscreen', type: 'corvus.status_request' }),
        new Promise<null>((resolve) => setTimeout(() => resolve(null), 2000)),
      ]);
      if (raw === null) messageError = 'status_request timed out after 2000 ms';
      response = raw && typeof raw === 'object' ? (raw as Record<string, unknown>) : null;
    } catch (error) {
      messageError = error instanceof Error && error.message ? error.message : String(error);
    }

    return {
      reachable: Boolean(response && response.ok !== false),
      hasDocument,
      contextUrls,
      status: response && typeof response.status === 'string' ? response.status : null,
      lastError: response && typeof response.lastError === 'string' ? response.lastError : null,
      attempt: response && typeof response.attempt === 'number' ? response.attempt : null,
      url: response && typeof response.url === 'string' ? response.url : null,
      error: messageError || contextError,
    };
  });
}

export async function ensureOffscreenDocument(worker: Worker): Promise<OffscreenCreateResult> {
  return worker.evaluate(async () => {
    const offscreen = (
      chrome as unknown as {
        offscreen?: {
          hasDocument?: () => Promise<boolean>;
          createDocument?: (options: Record<string, unknown>) => Promise<void>;
        };
      }
    ).offscreen;

    if (!offscreen) return { created: false, error: 'chrome.offscreen is unavailable' };

    try {
      if (typeof offscreen.hasDocument === 'function' && (await offscreen.hasDocument())) {
        return { created: false, error: null };
      }
    } catch {
      // Fall through to createDocument; a missing hasDocument is not fatal here.
    }

    if (typeof offscreen.createDocument !== 'function') {
      return { created: false, error: 'chrome.offscreen.createDocument is unavailable' };
    }

    try {
      await offscreen.createDocument({
        url: 'offscreen.html',
        reasons: ['BLOBS'],
        justification:
          'E2E readiness: the offscreen document owns the isolated loopback WebSocket and must exist before the handshake.',
      });
      return { created: true, error: null };
    } catch (error) {
      const message = error instanceof Error && error.message ? error.message : String(error);
      if (/single offscreen|already exists/i.test(message)) return { created: false, error: null };
      return { created: false, error: message };
    }
  });
}

export async function connectOffscreen(worker: Worker, daemonUrl: string): Promise<OffscreenConnectResult> {
  return worker.evaluate(async (url: string) => {
    try {
      const response = (await Promise.race([
        chrome.runtime.sendMessage({
          target: 'offscreen',
          type: 'corvus.connect',
          url,
          reason: 'e2e_readiness',
        }),
        new Promise<null>((resolve) => setTimeout(() => resolve(null), 2000)),
      ])) as Record<string, unknown> | null;
      if (response === null) return { ok: false, error: 'connect request timed out after 2000 ms' };
      return { ok: Boolean(response.ok !== false), error: null };
    } catch (error) {
      return { ok: false, error: error instanceof Error && error.message ? error.message : String(error) };
    }
  }, daemonUrl);
}

export interface OffscreenReadyResult {
  status: OffscreenStatus;
  created: OffscreenCreateResult | null;
  attempts: number;
}

export async function waitForOffscreenConnected(
  worker: Worker,
  daemonUrl: string,
  timeoutMs = 20000,
): Promise<OffscreenReadyResult> {
  const deadline = Date.now() + timeoutMs;
  let created: OffscreenCreateResult | null = null;
  let attempts = 0;
  let lastConnectAt = 0;
  let status = await readOffscreenStatus(worker);

  for (;;) {
    attempts += 1;

    if (!status.hasDocument && status.contextUrls.length === 0) {
      created = await ensureOffscreenDocument(worker);
      lastConnectAt = 0;
    }

    if (status.status === 'connected' && status.url === daemonUrl) {
      return { status, created, attempts };
    }

    if (Date.now() >= deadline) {
      throw new Error(
        'offscreen did not reach connected state within ' +
          String(timeoutMs) +
          ' ms: ' +
          JSON.stringify({ created, attempts, status }),
      );
    }

    if (Date.now() - lastConnectAt >= CONNECT_RETRY_MS) {
      await connectOffscreen(worker, daemonUrl);
      lastConnectAt = Date.now();
    }

    await new Promise((resolve) => setTimeout(resolve, READY_POLL_MS));
    status = await readOffscreenStatus(worker);
  }
}

export interface ExtensionDiagnostics {
  workerUrl: string;
  offscreen: OffscreenStatus | null;
  offscreenError: string | null;
  daemon: unknown;
  capturedAt: string;
}

export async function collectExtensionDiagnostics(
  worker: Worker,
  daemon: { snapshot: () => unknown },
): Promise<ExtensionDiagnostics> {
  let offscreen: OffscreenStatus | null = null;
  let offscreenError: string | null = null;
  try {
    offscreen = await readOffscreenStatus(worker);
  } catch (error) {
    offscreenError = errorMessage(error);
  }

  return {
    workerUrl: worker.url(),
    offscreen,
    offscreenError,
    daemon: daemon.snapshot(),
    capturedAt: new Date().toISOString(),
  };
}

export function firstErrorLine(error: unknown): string {
  return errorMessage(error).split('\n')[0];
}
