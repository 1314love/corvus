import { z } from 'zod';

export const PROVIDER_ACTION_CODES = [
  'BAD_ARGS',
  'NOT_FOUND',
  'NAVIGATION_FAILED',
  'TIMEOUT',
  'PROVIDER_LOST',
  'CAPABILITY_UNAVAILABLE',
  'AMBIGUOUS_BROWSER',
  'INTERNAL',
] as const;

export type ProviderActionCode = (typeof PROVIDER_ACTION_CODES)[number];

export class ProviderActionError extends Error {
  readonly code: ProviderActionCode;

  constructor(code: ProviderActionCode, message: string) {
    super(message);
    this.name = 'ProviderActionError';
    this.code = code;
  }
}

export interface ProviderTabInfo {
  tab_id: string;
  window_id: number;
  title: string;
  url: string;
  active: boolean;
}

export interface ProviderNavigated {
  tab_id: string;
  url: string;
  title?: string;
  status: 'loaded';
}

export interface ProviderCaptured {
  tab_id: string;
  url: string;
  format: 'png' | 'jpeg';
  mime_type: 'image/png' | 'image/jpeg';
  width: number;
  height: number;
  data: string;
}

export interface ProviderSystemInfoValue {
  browser_family: string;
  browser_version: string;
  profile?: string;
  capabilities: Array<{ group: string; actions: string[] }>;
  limits: { max_invoke_bytes: number; max_result_bytes: number };
}

const tabIdSchema = z.string().min(1).max(128);
const refSchema = z.string().min(1).max(256);
const urlSchema = z.string().min(1).max(4096);

const emptyArgs = z.object({}).strict();
const tabsOpenArgs = z.object({ url: urlSchema, active: z.boolean().optional() }).strict();
const tabsCloseArgs = z.object({ tab_id: tabIdSchema }).strict();
const navigateArgs = z
  .object({
    url: urlSchema,
    tab_id: tabIdSchema.optional(),
    wait_until: z.enum(['load', 'domcontentloaded']).optional(),
    timeout_ms: z.number().int().min(1000).max(60000).optional(),
  })
  .strict();
const readArgs = z
  .object({
    tab_id: tabIdSchema.optional(),
    mode: z.enum(['text', 'html']).optional(),
    selector: z.string().min(1).max(512).optional(),
    max_chars: z.number().int().min(1000).max(200000).optional(),
  })
  .strict();
const snapshotArgs = z
  .object({
    tab_id: tabIdSchema.optional(),
    max_nodes: z.number().int().min(50).max(2000).optional(),
    include_hidden: z.boolean().optional(),
  })
  .strict();
const clickArgs = z.object({ ref: refSchema, tab_id: tabIdSchema.optional() }).strict();
const typeArgs = z
  .object({
    ref: refSchema,
    text: z.string().max(20000),
    clear: z.boolean().optional(),
    submit: z.boolean().optional(),
    tab_id: tabIdSchema.optional(),
  })
  .strict();
const captureArgs = z
  .object({
    tab_id: tabIdSchema.optional(),
    format: z.enum(['png', 'jpeg']).optional(),
    quality: z.number().int().min(1).max(100).optional(),
  })
  .strict();

export type ParsedProviderAction =
  | { action: 'system.info.info'; args: Record<string, never> }
  | { action: 'tabs.list'; args: Record<string, never> }
  | { action: 'tabs.open'; args: { url: string; active: boolean } }
  | { action: 'tabs.close'; args: { tab_id: string } }
  | {
      action: 'page.navigate';
      args: { url: string; tab_id?: string; wait_until: 'load' | 'domcontentloaded'; timeout_ms: number };
    }
  | { action: 'page.read'; args: { tab_id?: string; mode: 'text' | 'html'; selector?: string; max_chars: number } }
  | { action: 'page.snapshot'; args: { tab_id?: string; max_nodes: number; include_hidden: boolean } }
  | { action: 'input.click'; args: { ref: string; tab_id?: string } }
  | { action: 'input.type'; args: { ref: string; text: string; clear: boolean; submit: boolean; tab_id?: string } }
  | { action: 'capture.viewport'; args: { tab_id?: string; format: 'png' | 'jpeg'; quality?: number } };

function fail(code: ProviderActionCode, message: string): never {
  throw new ProviderActionError(code, message);
}

function parseWith<T>(schema: z.ZodType<T>, args: unknown): T {
  const result = schema.safeParse(args);
  if (!result.success) fail('BAD_ARGS', 'invalid provider action arguments');
  return result.data;
}

function knownGroup(action: string): boolean {
  return /^(system\.info|tabs|page|input|capture)\./.test(action);
}

export function parseProviderAction(action: unknown, rawArgs: unknown): ParsedProviderAction {
  if (typeof action !== 'string' || action.length === 0 || action.length > 128) {
    fail('BAD_ARGS', 'invalid provider action');
  }
  const args = rawArgs === undefined ? {} : rawArgs;
  if (typeof args !== 'object' || args === null || Array.isArray(args)) {
    fail('BAD_ARGS', 'invalid provider action arguments');
  }
  switch (action) {
    case 'system.info.info':
      parseWith(emptyArgs, args);
      return { action: 'system.info.info', args: {} };
    case 'tabs.list':
      parseWith(emptyArgs, args);
      return { action: 'tabs.list', args: {} };
    case 'tabs.open': {
      const parsed = parseWith(tabsOpenArgs, args);
      return { action: 'tabs.open', args: { url: parsed.url, active: parsed.active ?? true } };
    }
    case 'tabs.close': {
      const parsed = parseWith(tabsCloseArgs, args);
      return { action: 'tabs.close', args: { tab_id: parsed.tab_id } };
    }
    case 'page.navigate': {
      const parsed = parseWith(navigateArgs, args);
      return {
        action: 'page.navigate',
        args: {
          url: parsed.url,
          tab_id: parsed.tab_id,
          wait_until: parsed.wait_until ?? 'load',
          timeout_ms: parsed.timeout_ms ?? 30000,
        },
      };
    }
    case 'page.read': {
      const parsed = parseWith(readArgs, args);
      return {
        action: 'page.read',
        args: {
          tab_id: parsed.tab_id,
          mode: parsed.mode ?? 'text',
          selector: parsed.selector,
          max_chars: parsed.max_chars ?? 20000,
        },
      };
    }
    case 'page.snapshot': {
      const parsed = parseWith(snapshotArgs, args);
      return {
        action: 'page.snapshot',
        args: {
          tab_id: parsed.tab_id,
          max_nodes: parsed.max_nodes ?? 500,
          include_hidden: parsed.include_hidden ?? false,
        },
      };
    }
    case 'input.click': {
      const parsed = parseWith(clickArgs, args);
      return { action: 'input.click', args: { ref: parsed.ref, tab_id: parsed.tab_id } };
    }
    case 'input.type': {
      const parsed = parseWith(typeArgs, args);
      return {
        action: 'input.type',
        args: {
          ref: parsed.ref,
          text: parsed.text,
          clear: parsed.clear ?? true,
          submit: parsed.submit ?? false,
          tab_id: parsed.tab_id,
        },
      };
    }
    case 'capture.viewport': {
      const parsed = parseWith(captureArgs, args);
      const format = parsed.format ?? 'png';
      return {
        action: 'capture.viewport',
        args: {
          tab_id: parsed.tab_id,
          format,
          quality: format === 'jpeg' ? parsed.quality : undefined,
        },
      };
    }
    default:
      if (!knownGroup(action)) fail('CAPABILITY_UNAVAILABLE', 'provider capability is not available');
      fail('BAD_ARGS', 'invalid provider action');
  }
}

export interface ProviderActionDeps {
  listTabs(): Promise<ProviderTabInfo[]>;
  openTab(url: string, active: boolean): Promise<ProviderTabInfo>;
  closeTab(tabId: string): Promise<void>;
  navigate(
    tabId: string | undefined,
    url: string,
    waitUntil: 'load' | 'domcontentloaded',
    timeoutMs: number,
  ): Promise<ProviderNavigated>;
  contentAction(tabId: string | undefined, action: string, args: Record<string, unknown>): Promise<unknown>;
  captureViewport(tabId: string | undefined, format: 'png' | 'jpeg', quality?: number): Promise<ProviderCaptured>;
  systemInfo(): ProviderSystemInfoValue;
}

export async function dispatchProviderAction(parsed: ParsedProviderAction, deps: ProviderActionDeps): Promise<unknown> {
  switch (parsed.action) {
    case 'system.info.info':
      return deps.systemInfo();
    case 'tabs.list': {
      const tabs = (await deps.listTabs()).slice(0, 200);
      return { tabs, count: tabs.length };
    }
    case 'tabs.open':
      return deps.openTab(parsed.args.url, parsed.args.active);
    case 'tabs.close':
      await deps.closeTab(parsed.args.tab_id);
      return { tab_id: parsed.args.tab_id, closed: true };
    case 'page.navigate':
      return deps.navigate(parsed.args.tab_id, parsed.args.url, parsed.args.wait_until, parsed.args.timeout_ms);
    case 'page.read':
      return deps.contentAction(parsed.args.tab_id, 'page.read', {
        mode: parsed.args.mode,
        selector: parsed.args.selector,
        max_chars: parsed.args.max_chars,
      });
    case 'page.snapshot':
      return deps.contentAction(parsed.args.tab_id, 'page.snapshot', {
        max_nodes: parsed.args.max_nodes,
        include_hidden: parsed.args.include_hidden,
      });
    case 'input.click':
      return deps.contentAction(parsed.args.tab_id, 'input.click', { ref: parsed.args.ref });
    case 'input.type':
      return deps.contentAction(parsed.args.tab_id, 'input.type', {
        ref: parsed.args.ref,
        text: parsed.args.text,
        clear: parsed.args.clear,
        submit: parsed.args.submit,
      });
    case 'capture.viewport':
      return deps.captureViewport(parsed.args.tab_id, parsed.args.format, parsed.args.quality);
    default:
      fail('CAPABILITY_UNAVAILABLE', 'provider capability is not available');
  }
}

export function providerCapabilities(): Array<{ id: string; version: number; actions: string[] }> {
  return [
    { id: 'system.info', version: 1, actions: ['info'] },
    { id: 'tabs', version: 1, actions: ['list', 'open', 'close'] },
    { id: 'page', version: 1, actions: ['navigate', 'read', 'snapshot'] },
    { id: 'input', version: 1, actions: ['click', 'type'] },
    { id: 'capture', version: 1, actions: ['viewport'] },
  ];
}

export function providerSystemInfo(): ProviderSystemInfoValue {
  return {
    browser_family: detectBrowserFamily(),
    browser_version: detectBrowserVersion(),
    capabilities: [
      { group: 'tabs', actions: ['list', 'open', 'close'] },
      { group: 'page', actions: ['navigate', 'read', 'snapshot'] },
      { group: 'input', actions: ['click', 'type'] },
      { group: 'capture', actions: ['viewport'] },
    ],
    limits: { max_invoke_bytes: 1048576, max_result_bytes: 8388608 },
  };
}

function detectBrowserFamily(): string {
  const ua = typeof navigator === 'undefined' ? '' : navigator.userAgent;
  if (/Edg\//.test(ua)) return 'Edge';
  if (/Chrome\//.test(ua)) return 'Chrome';
  if (/Firefox\//.test(ua)) return 'Firefox';
  return 'unknown';
}

function detectBrowserVersion(): string {
  const ua = typeof navigator === 'undefined' ? '' : navigator.userAgent;
  const match = /(?:Chrome|Edg|Firefox)\/([0-9.]+)/.exec(ua);
  return match ? match[1] : 'unknown';
}
