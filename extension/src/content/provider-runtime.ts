/**
 * Provider-only content runtime.
 *
 * It implements exactly the P2 page.read, page.snapshot, input.click and
 * input.type actions. Nothing here talks to a daemon, stores secrets or uses
 * the debugger. Refs are local to this document and are replaced on every
 * snapshot.
 */

type ContentCode = 'BAD_ARGS' | 'NOT_FOUND' | 'INTERNAL';

class ContentActionError extends Error {
  readonly code: ContentCode;
  constructor(code: ContentCode, message: string) {
    super(message);
    this.name = 'ContentActionError';
    this.code = code;
  }
}

interface SnapshotNode {
  ref: string;
  role: string;
  name: string;
  value?: string;
  bbox?: { x: number; y: number; width: number; height: number };
}

const refs = new Map<string, Element>();
let nextRef = 1;

function fail(code: ContentCode, message: string): never {
  throw new ContentActionError(code, message);
}

function truncate(value: string, max: number): string {
  return value.length > max ? value.slice(0, max) : value;
}

function composedParent(element: Element): Element | null {
  if (element.parentElement) return element.parentElement;
  const root = element.getRootNode();
  return root instanceof ShadowRoot ? root.host : null;
}

function isHidden(element: Element): boolean {
  for (let current: Element | null = element; current; current = composedParent(current)) {
    if (current.getAttribute('aria-hidden') === 'true') return true;
    const style = getComputedStyle(current);
    if (style.display === 'none' || style.visibility === 'hidden') return true;
  }
  const rect = element.getBoundingClientRect();
  return rect.width <= 0 || rect.height <= 0;
}

function collectElements(root: ParentNode, out: Element[]): void {
  for (const child of Array.from(root.children)) {
    out.push(child);
    if (child.shadowRoot) collectElements(child.shadowRoot, out);
    else collectElements(child, out);
  }
}

function queryDeepOne(selector: string): Element | null {
  const parts = selector
    .split('>>>')
    .map((part) => part.trim())
    .filter(Boolean);
  if (parts.length === 0 || parts.length > 8) fail('BAD_ARGS', 'invalid selector');
  let scope: ParentNode = document;
  let found: Element | null = null;
  for (const part of parts) {
    try {
      found = scope.querySelector(part);
    } catch {
      fail('BAD_ARGS', 'invalid selector');
    }
    if (!found) return null;
    scope = found.shadowRoot ?? found;
  }
  return found;
}

function textOf(element: Element): string {
  const value = (element as HTMLElement).innerText;
  if (typeof value === 'string') return value;
  return element.textContent ?? '';
}

function directText(element: Element): string {
  let text = '';
  for (const child of Array.from(element.childNodes)) {
    if (child.nodeType === 3) text += child.textContent ?? '';
  }
  return text;
}

function labelFor(element: Element): string {
  const byId = element.getAttribute('id');
  if (byId) {
    try {
      const label = document.querySelector('label[for="' + CSS.escape(byId) + '"]');
      if (label) return textOf(label);
    } catch {
      // Ignore an invalid id selector.
    }
  }
  const wrapping = element.closest('label');
  if (wrapping) return textOf(wrapping);
  return '';
}

function nameFor(element: Element): string {
  const aria = element.getAttribute('aria-label');
  if (aria && aria.trim()) return truncate(aria.trim(), 200);
  const labelledBy = element.getAttribute('aria-labelledby');
  if (labelledBy) {
    const parts = labelledBy
      .split(/\s+/)
      .map((id) => document.getElementById(id)?.textContent?.trim() ?? '')
      .filter(Boolean);
    if (parts.length) return truncate(parts.join(' '), 200);
  }
  if (
    element instanceof HTMLInputElement ||
    element instanceof HTMLTextAreaElement ||
    element instanceof HTMLSelectElement
  ) {
    const label = labelFor(element).trim();
    if (label) return truncate(label, 200);
    const placeholder = element.getAttribute('placeholder');
    if (placeholder && placeholder.trim()) return truncate(placeholder.trim(), 200);
    return element.getAttribute('name') ?? '';
  }
  if (element instanceof HTMLImageElement) {
    return truncate(element.alt || element.title || '', 200);
  }
  if (element instanceof HTMLButtonElement || element instanceof HTMLAnchorElement) {
    return truncate(textOf(element).trim(), 200);
  }
  const direct = directText(element).trim();
  if (direct) return truncate(direct, 200);
  return '';
}

function roleFor(element: Element): string | null {
  const explicit = element.getAttribute('role');
  if (explicit) return explicit;
  if (element instanceof HTMLButtonElement) return 'button';
  if (element instanceof HTMLAnchorElement) return element.hasAttribute('href') ? 'link' : null;
  if (element instanceof HTMLInputElement) {
    const type = element.type.toLowerCase();
    if (type === 'checkbox') return 'checkbox';
    if (type === 'radio') return 'radio';
    if (type === 'range') return 'slider';
    if (type === 'file') return 'button';
    if (type === 'submit' || type === 'button' || type === 'reset') return 'button';
    return 'textbox';
  }
  if (element instanceof HTMLTextAreaElement) return 'textbox';
  if (element instanceof HTMLSelectElement) return 'combobox';
  if (element instanceof HTMLImageElement) return 'img';
  if (/^H[1-6]$/.test(element.tagName)) return 'heading';
  if (element.tagName === 'UL' || element.tagName === 'OL') return 'list';
  if (element.tagName === 'LI') return 'listitem';
  if (element.tagName === 'TABLE') return 'table';
  if (element.tagName === 'TR') return 'row';
  if (element.tagName === 'TD' || element.tagName === 'TH') return 'cell';
  if (element.tagName === 'FORM') return 'form';
  if (element.tagName === 'NAV') return 'navigation';
  if (element.tagName === 'MAIN') return 'main';
  if (element.tagName === 'DIALOG') return 'dialog';
  if (element.tagName === 'SUMMARY') return 'button';
  if (element.tagName === 'P' || element.tagName === 'SPAN' || element.tagName === 'DIV') {
    return directText(element).trim() ? 'text' : null;
  }
  return null;
}

function valueFor(element: Element): string | undefined {
  if (element instanceof HTMLInputElement) {
    if (element.type.toLowerCase() === 'password') return undefined;
    if (element.type.toLowerCase() === 'checkbox' || element.type.toLowerCase() === 'radio') {
      return element.checked ? 'true' : 'false';
    }
    return truncate(element.value, 200);
  }
  if (element instanceof HTMLTextAreaElement) return truncate(element.value, 200);
  if (element instanceof HTMLSelectElement) {
    const selected = element.selectedOptions[0];
    return selected ? truncate(selected.textContent?.trim() ?? selected.value, 200) : '';
  }
  return undefined;
}

function bboxFor(element: Element): SnapshotNode['bbox'] | undefined {
  const rect = element.getBoundingClientRect();
  if (!Number.isFinite(rect.width) || !Number.isFinite(rect.height) || rect.width <= 0 || rect.height <= 0) {
    return undefined;
  }
  return {
    x: Math.round(rect.x * 100) / 100,
    y: Math.round(rect.y * 100) / 100,
    width: Math.round(rect.width * 100) / 100,
    height: Math.round(rect.height * 100) / 100,
  };
}

function sanitizedHtml(element: Element): string {
  const clone = element.cloneNode(true) as Element;
  for (const input of Array.from(clone.querySelectorAll('input'))) {
    if (input.getAttribute('type')?.toLowerCase() === 'password') {
      input.removeAttribute('value');
      input.setAttribute('value', '***');
    }
  }
  return clone.outerHTML;
}

function pageRead(args: Record<string, unknown>): Record<string, unknown> {
  const mode = args.mode === 'html' ? 'html' : 'text';
  const maxChars = typeof args.max_chars === 'number' ? args.max_chars : 20000;
  const selector = typeof args.selector === 'string' ? args.selector : '';
  const target = selector ? queryDeepOne(selector) : document.body;
  if (!target) fail('NOT_FOUND', 'the selector matched no element');
  const full = mode === 'html' ? sanitizedHtml(target) : textOf(target);
  return {
    url: location.href,
    mode,
    content: full.slice(0, maxChars),
    truncated: full.length > maxChars,
  };
}

function pageSnapshot(args: Record<string, unknown>): Record<string, unknown> {
  const maxNodes = typeof args.max_nodes === 'number' ? args.max_nodes : 500;
  const includeHidden = args.include_hidden === true;
  const all: Element[] = [];
  collectElements(document, all);
  refs.clear();
  nextRef = 1;
  const nodes: SnapshotNode[] = [];
  let truncated = false;
  for (const element of all) {
    if (element.tagName === 'SCRIPT' || element.tagName === 'STYLE' || element.tagName === 'NOSCRIPT') continue;
    if (!includeHidden && isHidden(element)) continue;
    const role = roleFor(element);
    if (!role) continue;
    if (nodes.length >= maxNodes) {
      truncated = true;
      break;
    }
    const ref = '@e' + nextRef;
    nextRef += 1;
    refs.set(ref, element);
    const node: SnapshotNode = { ref, role, name: nameFor(element) };
    const value = valueFor(element);
    if (value !== undefined) node.value = value;
    const bbox = bboxFor(element);
    if (bbox) node.bbox = bbox;
    nodes.push(node);
  }
  return { url: location.href, nodes, truncated };
}

function resolveRef(raw: unknown): { ref: string; element: Element } {
  if (typeof raw !== 'string' || !/^@e[1-9][0-9]*$/.test(raw)) {
    fail('BAD_ARGS', 'invalid element reference');
  }
  const element = refs.get(raw);
  if (!element || !element.isConnected) fail('NOT_FOUND', 'the element reference is stale');
  return { ref: raw, element };
}

function inputClick(args: Record<string, unknown>): Record<string, unknown> {
  const { ref, element } = resolveRef(args.ref);
  if (typeof (element as HTMLElement).click !== 'function') fail('BAD_ARGS', 'the element cannot be clicked');
  element.scrollIntoView({ block: 'center', inline: 'center' });
  (element as HTMLElement).click();
  return { ref, clicked: true };
}

function inputType(args: Record<string, unknown>): Record<string, unknown> {
  const { ref, element } = resolveRef(args.ref);
  const text = typeof args.text === 'string' ? args.text : '';
  const clear = args.clear !== false;
  const submit = args.submit === true;
  element.scrollIntoView({ block: 'center', inline: 'center' });
  (element as HTMLElement).focus();
  if (element instanceof HTMLInputElement || element instanceof HTMLTextAreaElement) {
    element.value = clear ? text : element.value + text;
    element.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertText', data: text }));
    element.dispatchEvent(new Event('change', { bubbles: true }));
  } else if ((element as HTMLElement).isContentEditable) {
    element.textContent = clear ? text : (element.textContent ?? '') + text;
    element.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertText', data: text }));
  } else {
    fail('BAD_ARGS', 'the element cannot be typed into');
  }
  let submitted = false;
  if (submit) {
    const form = element.closest('form');
    if (form) {
      if (typeof form.requestSubmit === 'function') form.requestSubmit();
      else form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
      submitted = true;
    }
  }
  return { ref, typed_chars: text.length, submitted };
}

async function handleAction(action: unknown, rawArgs: unknown): Promise<unknown> {
  const args =
    rawArgs && typeof rawArgs === 'object' && !Array.isArray(rawArgs) ? (rawArgs as Record<string, unknown>) : {};
  switch (action) {
    case 'page.read':
      return pageRead(args);
    case 'page.snapshot':
      return pageSnapshot(args);
    case 'input.click':
      return inputClick(args);
    case 'input.type':
      return inputType(args);
    default:
      fail('BAD_ARGS', 'unsupported page action');
  }
}

function install(): void {
  if (typeof chrome === 'undefined' || !chrome.runtime?.onMessage) return;
  chrome.runtime.onMessage.addListener((message: unknown, _sender, sendResponse) => {
    if (!message || typeof message !== 'object') return false;
    const record = message as Record<string, unknown>;
    if (record.type !== 'corvus.provider_action') return false;
    void handleAction(record.action, record.args).then(
      (value) => sendResponse({ ok: true, value }),
      (error: unknown) => {
        const code = error instanceof ContentActionError ? error.code : 'INTERNAL';
        const text = error instanceof ContentActionError ? error.message : 'page action failed';
        sendResponse({ ok: false, error: { code, message: text } });
      },
    );
    return true;
  });
}

const scope = globalThis as unknown as Record<string, unknown>;
if (!scope.__corvusProviderContentInstalled) {
  scope.__corvusProviderContentInstalled = true;
  install();
}

export {};
