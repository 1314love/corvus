import { ToolError, describeError } from '../shared/errors';

/**
 * v1.7.0 extension-only tool handlers.
 *
 * Handlers that need a page document delegate to the content script through the
 * existing corvus.command channel. Accessibility and PDF use the already
 * managed debugger channel. Cookies also use CDP so no permission beyond
 * tabGroups is required.
 */

export interface ExpandedDeps {
  resolveTab(args: any): Promise<any>;
  runInTab(tool: string, args: any, frame: any, options?: any): Promise<any>;
  requestContentResult(tab: any, message: any): Promise<any>;
  sendCdpCommand(tabId: any, method: string, params?: any): Promise<any>;
  tabIdString(tabId: any): string;
}

const TAB_GROUP_COLORS = ['grey', 'blue', 'red', 'yellow', 'green', 'pink', 'purple', 'cyan', 'orange'];
const SAME_SITE_VALUES = ['strict', 'lax', 'none'];
const STORAGE_AREAS = ['local', 'session'];
const MAX_STORE_BYTES = 64 * 1024;

function badArgs(message: string, details?: any): ToolError {
  return new ToolError('BAD_ARGS', message, details);
}

function ensureObject(args: any): Record<string, any> {
  if (!args || typeof args !== 'object' || Array.isArray(args)) throw badArgs('arguments must be an object');
  return args as Record<string, any>;
}

function checkKeys(args: Record<string, any>, allowed: string[]): void {
  for (const key of Object.keys(args)) {
    if (allowed.indexOf(key) === -1) throw badArgs('unknown argument: ' + key, { argument: key });
  }
}

function optionalString(args: Record<string, any>, key: string, max: number): string | undefined {
  const value = args[key];
  if (value === undefined || value === null) return undefined;
  if (typeof value !== 'string') throw badArgs('args.' + key + ' must be a string', { argument: key });
  if (value.length > max) throw badArgs('args.' + key + ' is too long', { argument: key, max });
  return value;
}

function requiredString(args: Record<string, any>, key: string, max: number, allowEmpty?: boolean): string {
  const value = optionalString(args, key, max);
  if (value === undefined) throw badArgs('args.' + key + ' is required', { argument: key });
  if (!allowEmpty && value.trim() === '') throw badArgs('args.' + key + ' must not be empty', { argument: key });
  return value;
}

function optionalInteger(args: Record<string, any>, key: string, min: number, max: number): number | undefined {
  const value = args[key];
  if (value === undefined || value === null) return undefined;
  if (typeof value !== 'number' || !Number.isInteger(value) || value < min || value > max) {
    throw badArgs('args.' + key + ' must be an integer between ' + min + ' and ' + max, { argument: key });
  }
  return value;
}

function optionalNumber(args: Record<string, any>, key: string, min: number, max: number): number | undefined {
  const value = args[key];
  if (value === undefined || value === null) return undefined;
  if (typeof value !== 'number' || !Number.isFinite(value) || value < min || value > max) {
    throw badArgs('args.' + key + ' must be a number between ' + min + ' and ' + max, { argument: key });
  }
  return value;
}

function optionalBoolean(args: Record<string, any>, key: string, fallback: boolean): boolean {
  const value = args[key];
  if (value === undefined || value === null) return fallback;
  if (typeof value !== 'boolean') throw badArgs('args.' + key + ' must be a boolean', { argument: key });
  return value;
}

function optionalEnum<T extends string>(args: Record<string, any>, key: string, values: T[], fallback: T): T {
  const value = args[key];
  if (value === undefined || value === null) return fallback;
  if (typeof value !== 'string' || values.indexOf(value as T) === -1) {
    throw badArgs('args.' + key + ' must be one of: ' + values.join(', '), { argument: key });
  }
  return value as T;
}

function optionalStringArray(
  args: Record<string, any>,
  key: string,
  maxItems: number,
  itemMax: number,
): string[] | undefined {
  const value = args[key];
  if (value === undefined || value === null) return undefined;
  if (!Array.isArray(value)) throw badArgs('args.' + key + ' must be an array', { argument: key });
  if (value.length > maxItems) throw badArgs('args.' + key + ' has too many items', { argument: key, max: maxItems });
  return value.map((item: any) => {
    if (typeof item !== 'string') throw badArgs('args.' + key + ' must contain strings only', { argument: key });
    if (item.length === 0 || item.length > itemMax) {
      throw badArgs('args.' + key + ' contains an invalid string', { argument: key });
    }
    return item;
  });
}

function round2(value: number): number {
  return Math.round(value * 100) / 100;
}

// ---------------------------------------------------------------------------
// Argument parsers
// ---------------------------------------------------------------------------

export interface PagePdfArgs {
  paper_width: number;
  paper_height: number;
  landscape: boolean;
  print_background: boolean;
  scale: number;
  tab_id?: string;
  browser_id?: string;
}

export function parsePagePdfArgs(raw: any): PagePdfArgs {
  const args = ensureObject(raw);
  checkKeys(args, ['paper_width', 'paper_height', 'landscape', 'print_background', 'scale', 'tab_id', 'browser_id']);
  return {
    paper_width: optionalNumber(args, 'paper_width', 1, 100) ?? 8.5,
    paper_height: optionalNumber(args, 'paper_height', 1, 100) ?? 11,
    landscape: optionalBoolean(args, 'landscape', false),
    print_background: optionalBoolean(args, 'print_background', true),
    scale: optionalNumber(args, 'scale', 0.1, 2) ?? 1,
    tab_id: optionalString(args, 'tab_id', 128),
    browser_id: optionalString(args, 'browser_id', 128),
  };
}

export interface PageLinksArgs {
  selector?: string;
  max_results: number;
  include_hidden: boolean;
  tab_id?: string;
  browser_id?: string;
}

export function parsePageLinksArgs(raw: any): PageLinksArgs {
  const args = ensureObject(raw);
  checkKeys(args, ['selector', 'max_results', 'include_hidden', 'tab_id', 'browser_id']);
  const selector = optionalString(args, 'selector', 512);
  if (selector !== undefined && selector.trim() === '') throw badArgs('args.selector must not be empty');
  return {
    selector,
    max_results: optionalInteger(args, 'max_results', 1, 1000) ?? 200,
    include_hidden: optionalBoolean(args, 'include_hidden', false),
    tab_id: optionalString(args, 'tab_id', 128),
    browser_id: optionalString(args, 'browser_id', 128),
  };
}

export interface PageFormsArgs {
  selector?: string;
  include_values: boolean;
  max_forms: number;
  tab_id?: string;
  browser_id?: string;
}

export function parsePageFormsArgs(raw: any): PageFormsArgs {
  const args = ensureObject(raw);
  checkKeys(args, ['selector', 'include_values', 'max_forms', 'tab_id', 'browser_id']);
  const selector = optionalString(args, 'selector', 512);
  if (selector !== undefined && selector.trim() === '') throw badArgs('args.selector must not be empty');
  return {
    selector,
    include_values: optionalBoolean(args, 'include_values', false),
    max_forms: optionalInteger(args, 'max_forms', 1, 100) ?? 20,
    tab_id: optionalString(args, 'tab_id', 128),
    browser_id: optionalString(args, 'browser_id', 128),
  };
}

export interface PageAccessibilityArgs {
  root?: string;
  max_nodes: number;
  max_depth: number;
  tab_id?: string;
  browser_id?: string;
}

export function parsePageAccessibilityArgs(raw: any): PageAccessibilityArgs {
  const args = ensureObject(raw);
  checkKeys(args, ['root', 'max_nodes', 'max_depth', 'tab_id', 'browser_id']);
  const root = optionalString(args, 'root', 512);
  if (root !== undefined && root.trim() === '') throw badArgs('args.root must not be empty');
  return {
    root,
    max_nodes: optionalInteger(args, 'max_nodes', 1, 2000) ?? 500,
    max_depth: optionalInteger(args, 'max_depth', 1, 50) ?? 12,
    tab_id: optionalString(args, 'tab_id', 128),
    browser_id: optionalString(args, 'browser_id', 128),
  };
}

export interface BrowserDragArgs {
  from_ref: string;
  to_ref: string;
  steps: number;
  tab_id?: string;
  browser_id?: string;
}

export function parseBrowserDragArgs(raw: any): BrowserDragArgs {
  const args = ensureObject(raw);
  checkKeys(args, ['from_ref', 'to_ref', 'steps', 'tab_id', 'browser_id']);
  const fromRef = requiredString(args, 'from_ref', 64);
  const toRef = requiredString(args, 'to_ref', 64);
  if (fromRef === toRef) throw badArgs('from_ref and to_ref must be different');
  return {
    from_ref: fromRef,
    to_ref: toRef,
    steps: optionalInteger(args, 'steps', 1, 50) ?? 10,
    tab_id: optionalString(args, 'tab_id', 128),
    browser_id: optionalString(args, 'browser_id', 128),
  };
}

export interface BrowserFocusArgs {
  ref: string;
  tab_id?: string;
  browser_id?: string;
}

export function parseBrowserFocusArgs(raw: any): BrowserFocusArgs {
  const args = ensureObject(raw);
  checkKeys(args, ['ref', 'tab_id', 'browser_id']);
  return {
    ref: requiredString(args, 'ref', 64),
    tab_id: optionalString(args, 'tab_id', 128),
    browser_id: optionalString(args, 'browser_id', 128),
  };
}

export interface BrowserHighlightArgs {
  ref?: string;
  selector?: string;
  duration_ms: number;
  color: string;
  tab_id?: string;
  browser_id?: string;
}

export function parseBrowserHighlightArgs(raw: any): BrowserHighlightArgs {
  const args = ensureObject(raw);
  checkKeys(args, ['ref', 'selector', 'duration_ms', 'color', 'tab_id', 'browser_id']);
  const ref = optionalString(args, 'ref', 64);
  const selector = optionalString(args, 'selector', 512);
  const hasRef = ref !== undefined && ref !== '';
  const hasSelector = selector !== undefined && selector !== '';
  if (hasRef === hasSelector) throw badArgs('exactly one of ref or selector is required');
  if (selector !== undefined && selector.trim() === '') throw badArgs('args.selector must not be empty');
  const color = typeof args.color === 'string' ? args.color : '#ffd400';
  if (!/^#[0-9a-fA-F]{6}$/.test(color)) throw badArgs('args.color must be a #rrggbb color');
  return {
    ref: hasRef ? ref : undefined,
    selector: hasSelector ? selector : undefined,
    duration_ms: optionalInteger(args, 'duration_ms', 0, 10000) ?? 1200,
    color,
    tab_id: optionalString(args, 'tab_id', 128),
    browser_id: optionalString(args, 'browser_id', 128),
  };
}

export interface TabGroupArgs {
  tab_ids: string[];
  title?: string;
  color: string;
  browser_id?: string;
}

export function parseTabGroupArgs(raw: any): TabGroupArgs {
  const args = ensureObject(raw);
  checkKeys(args, ['tab_ids', 'title', 'color', 'browser_id']);
  const tabIds = optionalStringArray(args, 'tab_ids', 50, 128);
  if (!tabIds || tabIds.length === 0) throw badArgs('args.tab_ids must contain at least one tab id');
  const color = optionalEnum(args, 'color', TAB_GROUP_COLORS as any, 'blue');
  return {
    tab_ids: tabIds,
    title: optionalString(args, 'title', 64),
    color,
    browser_id: optionalString(args, 'browser_id', 128),
  };
}

export interface TabDuplicateArgs {
  tab_id: string;
  browser_id?: string;
}

export function parseTabDuplicateArgs(raw: any): TabDuplicateArgs {
  const args = ensureObject(raw);
  checkKeys(args, ['tab_id', 'browser_id']);
  return {
    tab_id: requiredString(args, 'tab_id', 128),
    browser_id: optionalString(args, 'browser_id', 128),
  };
}

export interface WindowListArgs {
  include_tabs: boolean;
  max_windows: number;
  browser_id?: string;
}

export function parseWindowListArgs(raw: any): WindowListArgs {
  const args = ensureObject(raw);
  checkKeys(args, ['include_tabs', 'max_windows', 'browser_id']);
  return {
    include_tabs: optionalBoolean(args, 'include_tabs', true),
    max_windows: optionalInteger(args, 'max_windows', 1, 50) ?? 20,
    browser_id: optionalString(args, 'browser_id', 128),
  };
}

export interface WindowNewArgs {
  url?: string;
  incognito: boolean;
  focused: boolean;
  width?: number;
  height?: number;
  browser_id?: string;
}

export function parseWindowNewArgs(raw: any): WindowNewArgs {
  const args = ensureObject(raw);
  checkKeys(args, ['url', 'incognito', 'focused', 'width', 'height', 'browser_id']);
  const url = optionalString(args, 'url', 4096);
  if (url !== undefined) validateWindowUrl(url);
  return {
    url,
    incognito: optionalBoolean(args, 'incognito', false),
    focused: optionalBoolean(args, 'focused', true),
    width: optionalInteger(args, 'width', 200, 10000),
    height: optionalInteger(args, 'height', 200, 10000),
    browser_id: optionalString(args, 'browser_id', 128),
  };
}

function validateWindowUrl(url: string): string {
  if (url === 'about:blank') return url;
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    throw badArgs('args.url must be an absolute http, https or about:blank URL');
  }
  if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') {
    throw badArgs('args.url must be an absolute http, https or about:blank URL');
  }
  return parsed.href;
}

export interface CookieSetArgs {
  name: string;
  value: string;
  url?: string;
  domain?: string;
  path: string;
  secure: boolean;
  http_only: boolean;
  same_site: string;
  expires?: number;
  tab_id?: string;
  browser_id?: string;
}

export function parseCookieSetArgs(raw: any): CookieSetArgs {
  const args = ensureObject(raw);
  checkKeys(args, [
    'name',
    'value',
    'url',
    'domain',
    'path',
    'secure',
    'http_only',
    'same_site',
    'expires',
    'tab_id',
    'browser_id',
  ]);
  return {
    name: requiredString(args, 'name', 256),
    value: requiredString(args, 'value', 4096, true),
    url: optionalString(args, 'url', 4096),
    domain: optionalString(args, 'domain', 256),
    path: optionalString(args, 'path', 512) ?? '/',
    secure: optionalBoolean(args, 'secure', false),
    http_only: optionalBoolean(args, 'http_only', false),
    same_site: optionalEnum(args, 'same_site', SAME_SITE_VALUES as any, 'lax'),
    expires: optionalInteger(args, 'expires', 0, Number.MAX_SAFE_INTEGER),
    tab_id: optionalString(args, 'tab_id', 128),
    browser_id: optionalString(args, 'browser_id', 128),
  };
}

export interface CookieClearArgs {
  url?: string;
  domain?: string;
  name?: string;
  tab_id?: string;
  browser_id?: string;
}

export function parseCookieClearArgs(raw: any): CookieClearArgs {
  const args = ensureObject(raw);
  checkKeys(args, ['url', 'domain', 'name', 'tab_id', 'browser_id']);
  return {
    url: optionalString(args, 'url', 4096),
    domain: optionalString(args, 'domain', 256),
    name: optionalString(args, 'name', 256),
    tab_id: optionalString(args, 'tab_id', 128),
    browser_id: optionalString(args, 'browser_id', 128),
  };
}

export interface StorageArgs {
  area: 'local' | 'session';
  keys?: string[];
  entries?: Record<string, string>;
  tab_id?: string;
  browser_id?: string;
}

export function parseStorageGetArgs(raw: any): StorageArgs {
  const args = ensureObject(raw);
  checkKeys(args, ['area', 'keys', 'tab_id', 'browser_id']);
  const keys = optionalStringArray(args, 'keys', 100, 256);
  return {
    area: optionalEnum(args, 'area', STORAGE_AREAS as any, 'local') as 'local' | 'session',
    keys,
    tab_id: optionalString(args, 'tab_id', 128),
    browser_id: optionalString(args, 'browser_id', 128),
  };
}

export function parseStorageSetArgs(raw: any): StorageArgs {
  const args = ensureObject(raw);
  checkKeys(args, ['area', 'entries', 'tab_id', 'browser_id']);
  const entries = args.entries;
  if (!entries || typeof entries !== 'object' || Array.isArray(entries)) {
    throw badArgs('args.entries must be an object of string values');
  }
  const keys = Object.keys(entries);
  if (keys.length < 1 || keys.length > 100) throw badArgs('args.entries must contain between 1 and 100 keys');
  let total = 0;
  const clean: Record<string, string> = {};
  for (const key of keys) {
    const value = entries[key];
    if (typeof value !== 'string') throw badArgs('args.entries values must be strings', { key });
    if (key.length === 0 || key.length > 256) throw badArgs('args.entries keys must be 1..256 characters', { key });
    if (value.length > 4096) throw badArgs('args.entries values must be at most 4096 characters', { key });
    total += new TextEncoder().encode(key + value).length;
    if (total > MAX_STORE_BYTES) throw badArgs('args.entries exceed the 64 KiB limit');
    clean[key] = value;
  }
  return {
    area: optionalEnum(args, 'area', STORAGE_AREAS as any, 'local') as 'local' | 'session',
    entries: clean,
    tab_id: optionalString(args, 'tab_id', 128),
    browser_id: optionalString(args, 'browser_id', 128),
  };
}

export function parseStorageClearArgs(raw: any): StorageArgs {
  const args = ensureObject(raw);
  checkKeys(args, ['area', 'keys', 'tab_id', 'browser_id']);
  return {
    area: optionalEnum(args, 'area', STORAGE_AREAS as any, 'local') as 'local' | 'session',
    keys: optionalStringArray(args, 'keys', 100, 256),
    tab_id: optionalString(args, 'tab_id', 128),
    browser_id: optionalString(args, 'browser_id', 128),
  };
}

// ---------------------------------------------------------------------------
// Accessibility tree normalization
// ---------------------------------------------------------------------------

function axText(value: any): string {
  if (value === undefined || value === null) return '';
  if (typeof value === 'string') return value;
  if (typeof value === 'number' || typeof value === 'boolean') return String(value);
  if (typeof value === 'object' && 'value' in value) return String((value as any).value ?? '');
  return '';
}

function capped(value: any, max: number): string {
  const text = String(value === undefined || value === null ? '' : value);
  return text.length > max ? text.slice(0, max) : text;
}

function axProperty(node: any, name: string): any {
  const properties = Array.isArray(node && node.properties) ? node.properties : [];
  for (const property of properties) {
    if (property && property.name === name) {
      const value = property.value;
      return value && typeof value === 'object' && 'value' in value ? value.value : value;
    }
  }
  return undefined;
}

export interface NormalizedAxNode {
  role: string;
  name: string;
  value?: string;
  description?: string;
  disabled?: boolean;
  checked?: boolean | string;
  expanded?: boolean;
  selected?: boolean;
  level?: number;
  depth: number;
}

export function normalizeAxTree(
  rawNodes: any[],
  rootBackendNodeId: number | null,
  maxNodes: number,
  maxDepth: number,
): { nodes: NormalizedAxNode[]; truncated: boolean } {
  const byId = new Map<string, any>();
  for (const node of rawNodes || []) {
    if (node && node.nodeId !== undefined) byId.set(String(node.nodeId), node);
  }
  const nodes: NormalizedAxNode[] = [];
  let truncated = false;

  const visit = (node: any, depth: number): void => {
    if (!node || nodes.length >= maxNodes) {
      if (node) truncated = true;
      return;
    }
    const role = String(axText(node.role) || '').toLowerCase();
    const ignored = node.ignored === true || role === 'none' || role === 'presentation';
    if (!ignored) {
      const output: NormalizedAxNode = {
        role: role || 'generic',
        name: capped(axText(node.name), 200),
        depth,
      };
      const value = capped(axText(node.value), 200);
      if (value) output.value = value;
      const description = capped(axText(node.description), 200);
      if (description) output.description = description;
      const disabled = axProperty(node, 'disabled');
      if (disabled === true || disabled === false) output.disabled = disabled;
      const checked = axProperty(node, 'checked');
      if (checked === true || checked === false) output.checked = checked;
      else if (checked === 'mixed' || checked === 'true' || checked === 'false') {
        output.checked = checked === 'mixed' ? 'mixed' : checked === 'true';
      }
      const expanded = axProperty(node, 'expanded');
      if (expanded === true || expanded === false) output.expanded = expanded;
      const selected = axProperty(node, 'selected');
      if (selected === true || selected === false) output.selected = selected;
      const level = Number(axProperty(node, 'level'));
      if (Number.isFinite(level) && level > 0) output.level = level;
      nodes.push(output);
      if (nodes.length >= maxNodes) {
        const childIds = Array.isArray(node.childIds) ? node.childIds : [];
        if (childIds.length > 0) truncated = true;
        return;
      }
      depth += 1;
    }

    if (depth >= maxDepth) {
      const pending = Array.isArray(node.childIds) ? node.childIds.slice() : [];
      while (pending.length > 0) {
        const child = byId.get(String(pending.shift()));
        if (!child) continue;
        const childRole = String(axText(child.role) || '').toLowerCase();
        const childIgnored = child.ignored === true || childRole === 'none' || childRole === 'presentation';
        if (!childIgnored) {
          truncated = true;
          break;
        }
        if (Array.isArray(child.childIds)) pending.push(...child.childIds);
      }
      return;
    }

    const childIds = Array.isArray(node.childIds) ? node.childIds : [];
    for (const childId of childIds) {
      visit(byId.get(String(childId)), depth);
      if (nodes.length >= maxNodes) break;
    }
  };

  if (rootBackendNodeId !== null) {
    const root = (rawNodes || []).find((node) => node && node.backendDOMNodeId === rootBackendNodeId);
    if (root) visit(root, 0);
    return { nodes, truncated };
  }

  const roots = (rawNodes || []).filter((node) => node && (node.parentId === undefined || node.parentId === null));
  const ordered = roots.length > 0 ? roots : (rawNodes || []).slice(0, 1);
  for (const root of ordered) {
    visit(root, 0);
    if (nodes.length >= maxNodes) break;
  }
  return { nodes, truncated };
}

// ---------------------------------------------------------------------------
// Result helpers and handlers
// ---------------------------------------------------------------------------

function requireResolvedTab(tab: any): any {
  if (!tab || typeof tab.id !== 'number') throw new ToolError('NOT_FOUND', 'no active tab is available');
  return tab;
}

function asCapabilityUnavailable(error: any, message: string): any {
  if (error instanceof ToolError) {
    if (error.code === 'CAPABILITY_UNAVAILABLE' || error.code === 'BAD_ARGS' || error.code === 'NOT_FOUND')
      return error;
    if (
      error.code === 'INTERNAL' &&
      /not supported|method not found|not allowed|not attached|detached/i.test(error.message)
    ) {
      return new ToolError('CAPABILITY_UNAVAILABLE', message, { cause: error.message });
    }
    return error;
  }
  const text = describeError(error);
  if (/not supported|method not found|not allowed|not attached|detached/i.test(text)) {
    return new ToolError('CAPABILITY_UNAVAILABLE', message, { cause: text });
  }
  return new ToolError('INTERNAL', message + ': ' + text);
}

async function pagePdf(args: any, deps: ExpandedDeps): Promise<any> {
  const parsed = parsePagePdfArgs(args);
  const tab = requireResolvedTab(await deps.resolveTab(parsed));
  let raw: any;
  try {
    raw = await deps.sendCdpCommand(tab.id, 'Page.printToPDF', {
      printBackground: parsed.print_background,
      landscape: parsed.landscape,
      paperWidth: parsed.paper_width,
      paperHeight: parsed.paper_height,
      scale: parsed.scale,
      marginTop: 0.4,
      marginBottom: 0.4,
      marginLeft: 0.4,
      marginRight: 0.4,
      preferCSSPageSize: false,
      transferMode: 'ReturnAsBase64',
    });
  } catch (error: any) {
    throw asCapabilityUnavailable(error, 'this page cannot be printed');
  }
  const data = raw && typeof raw.data === 'string' ? raw.data : '';
  if (!data) throw new ToolError('INTERNAL', 'Page.printToPDF returned no PDF data');
  return {
    format: 'pdf',
    mime_type: 'application/pdf',
    tab_id: deps.tabIdString(tab.id),
    url: tab.url || '',
    data,
    paper_width: parsed.paper_width,
    paper_height: parsed.paper_height,
    landscape: parsed.landscape,
    scale: parsed.scale,
  };
}

async function pageLinks(args: any, frame: any, deps: ExpandedDeps): Promise<any> {
  const parsed = parsePageLinksArgs(args);
  return deps.runInTab('page_links', parsed, frame);
}

async function pageForms(args: any, frame: any, deps: ExpandedDeps): Promise<any> {
  const parsed = parsePageFormsArgs(args);
  return deps.runInTab('page_forms', parsed, frame);
}

function readCdpNodeId(result: any): number {
  if (!result || typeof result !== 'object') return 0;
  if (typeof result.nodeId === 'number' && result.nodeId > 0) return result.nodeId;
  if (result.root && typeof result.root.nodeId === 'number' && result.root.nodeId > 0) return result.root.nodeId;
  return 0;
}

function readCdpNode(result: any): any {
  if (!result || typeof result !== 'object') return null;
  return result.node && typeof result.node === 'object' ? result.node : result;
}

async function resolveAxRootBackendId(tab: any, root: string, deps: ExpandedDeps): Promise<number> {
  const path = await deps.requestContentResult(tab, { type: 'corvus.ax_root', selector: root });
  const segments = path && Array.isArray(path.segments) ? path.segments : [];
  if (segments.length === 0) throw new ToolError('NOT_FOUND', 'the root selector matched no element', { root });
  let document: any;
  try {
    document = await deps.sendCdpCommand(tab.id, 'DOM.getDocument', { depth: 0, pierce: true });
  } catch (error: any) {
    throw asCapabilityUnavailable(error, 'the accessibility tree is unavailable for this tab');
  }
  let nodeId = readCdpNodeId(document);
  if (!nodeId) throw new ToolError('NOT_FOUND', 'the root selector matched no element', { root });
  for (let index = 0; index < segments.length; index += 1) {
    const selector = String(segments[index] || '');
    let found: any;
    try {
      found = await deps.sendCdpCommand(tab.id, 'DOM.querySelector', { nodeId, selector });
    } catch (error: any) {
      throw asCapabilityUnavailable(error, 'the accessibility tree is unavailable for this tab');
    }
    nodeId = readCdpNodeId(found);
    if (!nodeId) throw new ToolError('NOT_FOUND', 'the root selector matched no element', { root });
    if (index < segments.length - 1) {
      let described: any;
      try {
        described = await deps.sendCdpCommand(tab.id, 'DOM.describeNode', { nodeId, depth: 0 });
      } catch (error: any) {
        throw asCapabilityUnavailable(error, 'the accessibility tree is unavailable for this tab');
      }
      const describedNode = readCdpNode(described);
      const shadowRoots = describedNode && Array.isArray(describedNode.shadowRoots) ? describedNode.shadowRoots : [];
      nodeId = shadowRoots.length > 0 && typeof shadowRoots[0].nodeId === 'number' ? shadowRoots[0].nodeId : 0;
      if (!nodeId) throw new ToolError('NOT_FOUND', 'the root selector matched no element', { root });
    }
  }
  let finalNode: any;
  try {
    finalNode = await deps.sendCdpCommand(tab.id, 'DOM.describeNode', { nodeId, depth: 0 });
  } catch (error: any) {
    throw asCapabilityUnavailable(error, 'the accessibility tree is unavailable for this tab');
  }
  const finalNodeData = readCdpNode(finalNode);
  const backendNodeId = finalNodeData && finalNodeData.backendNodeId;
  if (typeof backendNodeId !== 'number')
    throw new ToolError('NOT_FOUND', 'the root selector matched no element', { root });
  return backendNodeId;
}

async function pageAccessibility(args: any, deps: ExpandedDeps): Promise<any> {
  const parsed = parsePageAccessibilityArgs(args);
  const tab = requireResolvedTab(await deps.resolveTab(parsed));
  let rootBackend: number | null = null;
  if (parsed.root !== undefined) {
    rootBackend = await resolveAxRootBackendId(tab, parsed.root, deps);
  }
  let raw: any;
  try {
    raw = await deps.sendCdpCommand(tab.id, 'Accessibility.getFullAXTree', {});
  } catch (error: any) {
    throw asCapabilityUnavailable(error, 'the accessibility tree is unavailable for this tab');
  }
  const normalized = normalizeAxTree(
    raw && Array.isArray(raw.nodes) ? raw.nodes : [],
    rootBackend,
    parsed.max_nodes,
    parsed.max_depth,
  );
  return {
    url: tab.url || '',
    count: normalized.nodes.length,
    truncated: normalized.truncated,
    nodes: normalized.nodes,
  };
}

async function browserDrag(args: any, deps: ExpandedDeps): Promise<any> {
  const parsed = parseBrowserDragArgs(args);
  const tab = requireResolvedTab(await deps.resolveTab(parsed));
  const from = await deps.requestContentResult(tab, { type: 'corvus.ref_point', ref: parsed.from_ref });
  const to = await deps.requestContentResult(tab, { type: 'corvus.ref_point', ref: parsed.to_ref });
  const fromX = Number(from && from.x);
  const fromY = Number(from && from.y);
  const toX = Number(to && to.x);
  const toY = Number(to && to.y);
  if (![fromX, fromY, toX, toY].every((value) => Number.isFinite(value))) {
    throw new ToolError('INTERNAL', 'the content script returned invalid element coordinates');
  }
  const dispatch = (params: any) => deps.sendCdpCommand(tab.id, 'Input.dispatchMouseEvent', params);
  await dispatch({ type: 'mouseMoved', x: fromX, y: fromY, button: 'none', buttons: 0 });
  await dispatch({ type: 'mousePressed', x: fromX, y: fromY, button: 'left', buttons: 1, clickCount: 1 });
  for (let index = 1; index <= parsed.steps; index += 1) {
    const ratio = index / parsed.steps;
    await dispatch({
      type: 'mouseMoved',
      x: fromX + (toX - fromX) * ratio,
      y: fromY + (toY - fromY) * ratio,
      button: 'left',
      buttons: 1,
    });
  }
  await dispatch({ type: 'mouseReleased', x: toX, y: toY, button: 'left', buttons: 0, clickCount: 1 });
  return {
    dragged: true,
    from: { ref: parsed.from_ref, x: round2(Number(from.pageX ?? fromX)), y: round2(Number(from.pageY ?? fromY)) },
    to: { ref: parsed.to_ref, x: round2(Number(to.pageX ?? toX)), y: round2(Number(to.pageY ?? toY)) },
    tab_id: deps.tabIdString(tab.id),
    url: tab.url || '',
  };
}

async function browserFocus(args: any, frame: any, deps: ExpandedDeps): Promise<any> {
  const parsed = parseBrowserFocusArgs(args);
  return deps.runInTab('browser_focus', parsed, frame, { withTabId: true });
}

async function browserHighlight(args: any, frame: any, deps: ExpandedDeps): Promise<any> {
  const parsed = parseBrowserHighlightArgs(args);
  return deps.runInTab('browser_highlight', parsed, frame, { withTabId: true });
}

function chromeApi(): any {
  return (globalThis as any).chrome;
}

async function resolveTabById(tabId: string): Promise<any> {
  try {
    return await chromeApi().tabs.get(Number(tabId));
  } catch {
    throw new ToolError('NOT_FOUND', 'tab ' + tabId + ' does not exist', { tab_id: tabId });
  }
}

async function tabGroup(args: any): Promise<any> {
  const parsed = parseTabGroupArgs(args);
  const tabs = [];
  for (const tabId of parsed.tab_ids) {
    tabs.push(await resolveTabById(tabId));
  }
  let groupId: any;
  try {
    groupId = await chromeApi().tabs.group({ tabIds: tabs.map((tab) => tab.id) });
  } catch (error: any) {
    throw new ToolError('INTERNAL', 'the tabs could not be grouped: ' + describeError(error));
  }
  if (groupId === undefined || groupId === null)
    throw new ToolError('INTERNAL', 'chrome.tabs.group returned no group id');
  try {
    const update: any = { color: parsed.color };
    if (parsed.title !== undefined) update.title = parsed.title;
    await chromeApi().tabGroups.update(groupId, update);
  } catch (error: any) {
    throw new ToolError('INTERNAL', 'the tab group could not be updated: ' + describeError(error));
  }
  return {
    group_id: String(groupId),
    tab_ids: tabs.map((tab) => String(tab.id)),
    title: parsed.title ?? '',
    color: parsed.color,
    window_id: String(tabs[0].windowId),
  };
}

async function tabDuplicate(args: any): Promise<any> {
  const parsed = parseTabDuplicateArgs(args);
  const tab = await resolveTabById(parsed.tab_id);
  let duplicated: any;
  try {
    duplicated = await chromeApi().tabs.duplicate(tab.id);
  } catch (error: any) {
    throw new ToolError('INTERNAL', 'the tab could not be duplicated: ' + describeError(error));
  }
  if (!duplicated || typeof duplicated.id !== 'number') {
    throw new ToolError('INTERNAL', 'chrome.tabs.duplicate returned no tab');
  }
  return {
    duplicated: true,
    source_tab_id: String(tab.id),
    tab: {
      tab_id: String(duplicated.id),
      index: duplicated.index,
      active: duplicated.active === true,
      url: duplicated.url || '',
      title: duplicated.title || '',
    },
  };
}

async function windowList(args: any): Promise<any> {
  const parsed = parseWindowListArgs(args);
  let windows: any[];
  try {
    windows = await chromeApi().windows.getAll({ populate: parsed.include_tabs });
  } catch (error: any) {
    throw new ToolError('INTERNAL', 'windows could not be listed: ' + describeError(error));
  }
  const allTabs = await chromeApi().tabs.query({});
  const counts = new Map<number, number>();
  for (const tab of allTabs || []) counts.set(tab.windowId, (counts.get(tab.windowId) || 0) + 1);
  const selected = (windows || []).slice(0, parsed.max_windows);
  let truncated = (windows || []).length > selected.length;
  let tabsLeft = 200;
  const output: any[] = [];
  for (const window of selected) {
    const entry: any = {
      window_id: String(window.id),
      focused: window.focused === true,
      state: window.state || 'normal',
      type: window.type || 'normal',
      incognito: window.incognito === true,
      left: Number.isFinite(window.left) ? window.left : 0,
      top: Number.isFinite(window.top) ? window.top : 0,
      width: Number.isFinite(window.width) ? window.width : 0,
      height: Number.isFinite(window.height) ? window.height : 0,
      tab_count: counts.get(window.id) ?? (Array.isArray(window.tabs) ? window.tabs.length : 0),
    };
    if (parsed.include_tabs) {
      const tabs = Array.isArray(window.tabs) ? window.tabs : [];
      const visible = tabs.slice(0, Math.max(0, tabsLeft));
      tabsLeft -= visible.length;
      if (visible.length < tabs.length || tabsLeft <= 0) truncated = true;
      entry.tabs = visible.map((tab: any) => ({
        tab_id: String(tab.id),
        index: tab.index,
        active: tab.active === true,
        url: tab.url || '',
        title: tab.title || '',
      }));
    }
    output.push(entry);
  }
  return { count: output.length, truncated, windows: output };
}

async function windowNew(args: any): Promise<any> {
  const parsed = parseWindowNewArgs(args);
  const createProperties: any = { incognito: parsed.incognito, focused: parsed.focused };
  if (parsed.url !== undefined) createProperties.url = parsed.url;
  if (parsed.width !== undefined) createProperties.width = parsed.width;
  if (parsed.height !== undefined) createProperties.height = parsed.height;
  let created: any;
  try {
    created = await chromeApi().windows.create(createProperties);
  } catch (error: any) {
    throw new ToolError('INTERNAL', 'the window could not be created: ' + describeError(error));
  }
  if (!created || typeof created.id !== 'number')
    throw new ToolError('INTERNAL', 'chrome.windows.create returned no window');
  const tab = Array.isArray(created.tabs) && created.tabs.length > 0 ? created.tabs[0] : null;
  return {
    created: true,
    window_id: String(created.id),
    tab_id: tab && typeof tab.id === 'number' ? String(tab.id) : '',
    url: tab && tab.url ? tab.url : parsed.url || '',
    incognito: created.incognito === true,
    width: Number.isFinite(created.width) ? created.width : parsed.width || 0,
    height: Number.isFinite(created.height) ? created.height : parsed.height || 0,
  };
}

function normalizeWebUrl(raw: string): string {
  let parsed: URL;
  try {
    parsed = new URL(raw);
  } catch {
    throw badArgs('a valid http or https URL is required');
  }
  if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') {
    throw badArgs('a valid http or https URL is required');
  }
  return parsed.href;
}

function cookieDomainFromUrl(url: string): string {
  return new URL(url).hostname;
}

function sameSiteForCdp(value: string): string {
  if (value === 'strict') return 'Strict';
  if (value === 'none') return 'None';
  return 'Lax';
}

async function cookieSet(args: any, deps: ExpandedDeps): Promise<any> {
  const parsed = parseCookieSetArgs(args);
  let tab: any = null;
  if (parsed.tab_id !== undefined) tab = await deps.resolveTab(parsed);
  let targetUrl: string;
  if (parsed.url !== undefined) {
    targetUrl = normalizeWebUrl(parsed.url);
  } else if (parsed.domain !== undefined) {
    if (tab && tab.url) {
      let scheme = 'https';
      try {
        scheme = new URL(tab.url).protocol === 'http:' ? 'http' : 'https';
      } catch {
        scheme = 'https';
      }
      targetUrl = scheme + '://' + parsed.domain.replace(/^\./, '') + '/';
    } else {
      targetUrl = 'https://' + parsed.domain.replace(/^\./, '') + '/';
    }
  } else {
    tab = tab || (await deps.resolveTab(parsed));
    if (!tab || typeof tab.url !== 'string' || !/^https?:/i.test(tab.url)) {
      throw badArgs('cookie_set needs an http(s) tab URL or an explicit url');
    }
    targetUrl = normalizeWebUrl(tab.url);
  }
  tab = tab || requireResolvedTab(await deps.resolveTab(parsed));
  const params: any = {
    name: parsed.name,
    value: parsed.value,
    url: targetUrl,
    path: parsed.path,
    secure: parsed.secure,
    httpOnly: parsed.http_only,
    sameSite: sameSiteForCdp(parsed.same_site),
  };
  if (parsed.domain !== undefined) params.domain = parsed.domain;
  if (parsed.expires !== undefined && parsed.expires > 0) params.expires = parsed.expires;
  let outcome: any;
  try {
    outcome = await deps.sendCdpCommand(tab.id, 'Network.setCookie', params);
  } catch (error: any) {
    throw asCapabilityUnavailable(error, 'the cookie could not be set');
  }
  if (outcome && outcome.success === false) throw new ToolError('INTERNAL', 'the browser rejected the cookie');
  const result: any = {
    set: true,
    name: parsed.name,
    domain: parsed.domain ?? cookieDomainFromUrl(targetUrl),
    path: parsed.path,
    secure: parsed.secure,
    http_only: parsed.http_only,
    same_site: parsed.same_site,
    expires: parsed.expires ?? 0,
    url: targetUrl,
  };
  if (parsed.tab_id !== undefined && tab && typeof tab.id === 'number') result.tab_id = deps.tabIdString(tab.id);
  return result;
}

function cookieMatchesDomain(cookieDomain: any, domain: string): boolean {
  const cookie = String(cookieDomain || '')
    .replace(/^\./, '')
    .toLowerCase();
  const target = String(domain || '')
    .replace(/^\./, '')
    .toLowerCase();
  return cookie === target || cookie.endsWith('.' + target);
}

async function cookieClear(args: any, deps: ExpandedDeps): Promise<any> {
  const parsed = parseCookieClearArgs(args);
  let tab: any = null;
  let targetUrl: string | null = null;
  let targetDomain: string | null = null;
  if (parsed.url !== undefined) {
    targetUrl = normalizeWebUrl(parsed.url);
    targetDomain = cookieDomainFromUrl(targetUrl);
  } else if (parsed.domain !== undefined) {
    targetDomain = parsed.domain.replace(/^\./, '');
    targetUrl = 'https://' + targetDomain + '/';
  } else {
    tab = await deps.resolveTab(parsed);
    if (!tab || typeof tab.url !== 'string' || !/^https?:/i.test(tab.url)) {
      throw badArgs('cookie_clear needs an http(s) tab URL, url, or domain');
    }
    targetUrl = normalizeWebUrl(tab.url);
    targetDomain = cookieDomainFromUrl(targetUrl);
  }
  const cdpTab = tab || requireResolvedTab(await deps.resolveTab(parsed));
  let all: any[];
  try {
    const response = await deps.sendCdpCommand(cdpTab.id, 'Storage.getCookies', {});
    all = response && Array.isArray(response.cookies) ? response.cookies : [];
  } catch (error: any) {
    throw asCapabilityUnavailable(error, 'cookies could not be read');
  }
  const urlPath = targetUrl ? new URL(targetUrl).pathname || '/' : '/';
  const matched = all.filter((cookie: any) => {
    if (parsed.name !== undefined && cookie.name !== parsed.name) return false;
    if (!cookieMatchesDomain(cookie.domain, targetDomain || '')) return false;
    if (parsed.url !== undefined && cookie.path && urlPath.indexOf(cookie.path) !== 0) return false;
    return true;
  });
  for (const cookie of matched) {
    try {
      await deps.sendCdpCommand(cdpTab.id, 'Network.deleteCookies', {
        name: cookie.name,
        domain: cookie.domain,
        path: cookie.path,
      });
    } catch (error: any) {
      throw asCapabilityUnavailable(error, 'cookies could not be cleared');
    }
  }
  const result: any = { cleared: matched.length, names: matched.slice(0, 100).map((cookie: any) => cookie.name) };
  if (parsed.url !== undefined) result.url = targetUrl;
  if (parsed.domain !== undefined) result.domain = parsed.domain;
  if (parsed.name !== undefined) result.name = parsed.name;
  return result;
}

async function storageGet(args: any, frame: any, deps: ExpandedDeps): Promise<any> {
  return deps.runInTab('storage_get', parseStorageGetArgs(args), frame);
}

async function storageSet(args: any, frame: any, deps: ExpandedDeps): Promise<any> {
  return deps.runInTab('storage_set', parseStorageSetArgs(args), frame);
}

async function storageClear(args: any, frame: any, deps: ExpandedDeps): Promise<any> {
  return deps.runInTab('storage_clear', parseStorageClearArgs(args), frame);
}

export function createExpandedHandlers(deps: ExpandedDeps): Record<string, (args: any, frame?: any) => Promise<any>> {
  return {
    page_pdf: (args) => pagePdf(args, deps),
    page_links: (args, frame) => pageLinks(args, frame, deps),
    page_forms: (args, frame) => pageForms(args, frame, deps),
    page_accessibility: (args) => pageAccessibility(args, deps),
    browser_drag: (args) => browserDrag(args, deps),
    browser_focus: (args, frame) => browserFocus(args, frame, deps),
    browser_highlight: (args, frame) => browserHighlight(args, frame, deps),
    tab_group: (args) => tabGroup(args),
    tab_duplicate: (args) => tabDuplicate(args),
    window_list: (args) => windowList(args),
    window_new: (args) => windowNew(args),
    cookie_set: (args) => cookieSet(args, deps),
    cookie_clear: (args) => cookieClear(args, deps),
    storage_get: (args, frame) => storageGet(args, frame, deps),
    storage_set: (args, frame) => storageSet(args, frame, deps),
    storage_clear: (args, frame) => storageClear(args, frame, deps),
  };
}

export const EXPANDED_TOOL_NAMES = [
  'page_pdf',
  'page_links',
  'page_forms',
  'page_accessibility',
  'browser_drag',
  'browser_focus',
  'browser_highlight',
  'tab_group',
  'tab_duplicate',
  'window_list',
  'window_new',
  'cookie_set',
  'cookie_clear',
  'storage_get',
  'storage_set',
  'storage_clear',
] as const;
