import { ToolError } from '../shared/errors';

declare global {
  interface Window {
    __corvusContentLoaded?: boolean;
  }
}

/**
 * @fileoverview Corvus content script.
 *
 * Runs in the top frame of every http, https and file page. Receives command
 * messages from the service worker, executes one of the browser tools it owns
 * and always answers exactly once with {ok:true, result} or {ok:false, error}.
 *
 * Uses no dynamic code, no inline scripts and no external resources.
 */

(() => {
  if (window.__corvusContentLoaded === true) return;
  window.__corvusContentLoaded = true;

  const CONTENT_VERSION = '1.4.0';
  const MAX_NODES_DEFAULT = 300;
  const MAX_NODES_LIMIT = 3000;
  const MIN_NODES_LIMIT = 40;
  const MAX_DEPTH = 80;
  const MAX_NAME = 200;
  const MAX_TEXT = 240;
  const COMPACT_NAME_LIMIT = 120;
  const COMPACT_VALUE_LIMIT = 80;
  const MAX_SELECTOR = 512;
  const MAX_INPUT = 10000;
  const MAX_REFS = 5000;
  const MAX_PAGE_TEXT = 200000;
  const DEFAULT_PAGE_TEXT = 100000;
  const MAX_PAGE_HTML = 500000;
  const DEFAULT_PAGE_HTML = 200000;
  const MAX_FIND_LIMIT = 100;
  const DEFAULT_FIND_LIMIT = 20;
  const MAX_SCROLL_PAGES = 50;
  const MAX_TIMEOUT_MS = 125000;
  const DEFAULT_TIMEOUT_MS = 60000;
  const REF_PATTERN = /^@?e(\d+)$/i;
  const NAVIGABLE_PATTERN = /^(https?|file):/i;

  const TOOL_MAP: Record<string, any> = {
    browser_navigate: 'navigate',
    page_snapshot: 'page_snapshot',
    browser_click: 'click',
    browser_type: 'type',
    page_text: 'page_text',
    page_html: 'page_html',
    page_find: 'page_find',
    browser_hover: 'hover',
    browser_press: 'press',
    browser_scroll: 'scroll',
    browser_select: 'select',
  };

  const SKIP_TAGS: Record<string, any> = {
    SCRIPT: true,
    STYLE: true,
    NOSCRIPT: true,
    TEMPLATE: true,
    HEAD: true,
    META: true,
    LINK: true,
    TITLE: true,
    BASE: true,
    PARAM: true,
    SOURCE: true,
    TRACK: true,
    BR: true,
    WBR: true,
  };

  const LANDMARKS: Record<string, any> = {
    NAV: true,
    MAIN: true,
    HEADER: true,
    FOOTER: true,
    ASIDE: true,
    FORM: true,
  };

  const TEXT_INPUT_TYPES: Record<string, any> = {
    text: true,
    search: true,
    email: true,
    url: true,
    tel: true,
    password: true,
    number: true,
    date: true,
    'datetime-local': true,
    month: true,
    week: true,
    time: true,
  };

  const refToElement = new Map<any, any>();
  const elementToRef = new WeakMap<any, any>();
  let refCounter = 0;
  let unloading = false;

  // pagehide fires both for a real departure and when the document enters the
  // back-forward cache. A cached document is frozen, but the same JavaScript
  // context can be restored later, so pageshow must clear the flag; otherwise
  // every command after a history back would report NAVIGATION_LOST on a
  // healthy page.
  window.addEventListener('pagehide', () => {
    unloading = true;
  });

  window.addEventListener('pageshow', () => {
    unloading = false;
  });

  // -------------------------------------------------------------------------
  // Errors
  // -------------------------------------------------------------------------

  function toPayload(error: any) {
    if (error instanceof ToolError) return error.toPayload();
    const message = error && error.message ? String(error.message) : String(error);
    return { code: 'INTERNAL', message: 'content script failure: ' + message, retryable: false };
  }

  function badArgs(message: string, details?: any) {
    return new ToolError('BAD_ARGS', message, details);
  }

  function notFound(message: string, details?: any) {
    return new ToolError('NOT_FOUND', message, details);
  }

  function notInteractable(message: string, details?: any) {
    return new ToolError('NOT_INTERACTABLE', message, details);
  }

  // -------------------------------------------------------------------------
  // Small DOM helpers
  // -------------------------------------------------------------------------

  function collapse(value: any) {
    return String(value === undefined || value === null ? '' : value)
      .replace(/\s+/g, ' ')
      .trim();
  }

  function truncate(value: any, limit: any) {
    const text = String(value === undefined || value === null ? '' : value);
    if (text.length <= limit) return text;
    return text.slice(0, limit - 1) + '\u2026';
  }

  function clampNodes(value: any) {
    const numeric = Number(value);
    if (!Number.isFinite(numeric)) return MAX_NODES_DEFAULT;
    return Math.min(MAX_NODES_LIMIT, Math.max(MIN_NODES_LIMIT, Math.floor(numeric)));
  }

  function clampTimeout(value: any) {
    const numeric = Number(value);
    if (!Number.isFinite(numeric) || numeric <= 0) return DEFAULT_TIMEOUT_MS;
    return Math.min(MAX_TIMEOUT_MS, Math.max(250, Math.round(numeric)));
  }

  function clampMaxChars(value: any, fallback: any, limit: any) {
    const numeric = Number(value);
    if (!Number.isFinite(numeric)) return fallback;
    return Math.min(limit, Math.max(1, Math.floor(numeric)));
  }

  function readIntegerRange(value: any, fallback: any, min: any, max: any, label: any) {
    if (value === undefined || value === null) return fallback;
    const numeric = Number(value);
    if (!Number.isInteger(numeric) || numeric < min || numeric > max) {
      throw badArgs(label + ' must be an integer from ' + min + ' to ' + max, { value: value });
    }
    return numeric;
  }

  function readBooleanFlag(value: any, fallback: any, label: any) {
    if (value === undefined || value === null) return fallback;
    if (typeof value !== 'boolean') {
      throw badArgs(label + ' must be a boolean', { value: value });
    }
    return value;
  }

  function visibleBodyText() {
    const root = document.body || document.documentElement;
    if (!root) return '';
    try {
      if (typeof root.innerText === 'string') return root.innerText;
    } catch (error: any) {
      // Fall through to textContent.
    }
    return root.textContent || '';
  }

  function nextFrame(): Promise<void> {
    return new Promise<void>((resolve: any) => {
      if (typeof window.requestAnimationFrame === 'function') {
        window.requestAnimationFrame(() => resolve());
      } else {
        window.setTimeout(resolve, 0);
      }
    });
  }

  function withDeadline(promise: Promise<any>, timeoutMs: number, label: string): Promise<any> {
    let timer: number | null = null;
    const deadline = new Promise((resolve: any, reject: any) => {
      timer = window.setTimeout(() => {
        reject(new ToolError('TIMEOUT', label + ' did not finish within ' + timeoutMs + ' ms'));
      }, timeoutMs);
    });
    return Promise.race([promise, deadline]).finally(() => {
      if (timer !== null) window.clearTimeout(timer);
    });
  }

  function isRendered(element: any) {
    if (!element || element.nodeType !== 1 || !element.isConnected) return false;
    try {
      if (typeof element.checkVisibility === 'function') {
        return element.checkVisibility({ checkOpacity: true, checkVisibilityCSS: true });
      }
    } catch (error: any) {
      // Fall through to the style based check.
    }
    const style = window.getComputedStyle(element);
    if (!style) return false;
    if (style.display === 'none' || style.visibility === 'hidden' || style.visibility === 'collapse') return false;
    if (style.opacity === '0') return false;
    return true;
  }

  function scrollIntoView(element: any) {
    try {
      element.scrollIntoView({ block: 'center', inline: 'center', behavior: 'auto' });
    } catch (error: any) {
      try {
        element.scrollIntoView();
      } catch (inner: any) {
        // Ignore: the hit test below reports the real problem.
      }
    }
  }

  function focusElement(element: any) {
    try {
      if (typeof element.focus === 'function') {
        element.focus({ preventScroll: true });
      }
    } catch (error: any) {
      try {
        element.focus();
      } catch (inner: any) {
        // Ignore.
      }
    }
  }

  function describeElement(element: any) {
    if (!element || element.nodeType !== 1) return null;
    return {
      tag: element.tagName ? element.tagName.toLowerCase() : '',
      id: element.id || null,
      role: element.getAttribute ? element.getAttribute('role') : null,
    };
  }

  // -------------------------------------------------------------------------
  // Element refs
  // -------------------------------------------------------------------------

  function pruneRefs() {
    while (refToElement.size > MAX_REFS) {
      const oldest = refToElement.keys().next();
      if (oldest.done) break;
      refToElement.delete(oldest.value);
    }
  }

  function refFor(element: any) {
    const existing = elementToRef.get(element);
    if (existing) {
      const number = Number(existing.slice(2));
      if (refToElement.get(number) === element) return existing;
    }
    refCounter += 1;
    const ref = '@e' + refCounter;
    elementToRef.set(element, ref);
    refToElement.set(refCounter, element);
    pruneRefs();
    return ref;
  }

  function resolveRef(raw: any) {
    const match = REF_PATTERN.exec(String(raw || '').trim());
    if (!match) return null;
    const element = refToElement.get(Number(match[1]));
    if (!element || !element.isConnected) return null;
    return element;
  }

  // -------------------------------------------------------------------------
  // Accessibility and snapshot helpers
  // -------------------------------------------------------------------------

  function roleOf(element: any) {
    const explicit = element.getAttribute ? element.getAttribute('role') : null;
    if (explicit && collapse(explicit)) return collapse(explicit).split(/\s+/)[0].toLowerCase();

    const tag = element.tagName;
    if (tag === 'A' || tag === 'AREA') return element.hasAttribute('href') ? 'link' : '';
    if (tag === 'BUTTON' || tag === 'SUMMARY') return 'button';
    if (tag === 'SELECT') return element.multiple ? 'listbox' : 'combobox';
    if (tag === 'TEXTAREA') return 'textbox';
    if (tag === 'IMG') return 'img';
    if (tag === 'IFRAME') return 'iframe';
    if (tag === 'INPUT') {
      const type = String(element.type || 'text').toLowerCase();
      if (type === 'checkbox') return 'checkbox';
      if (type === 'radio') return 'radio';
      if (type === 'submit' || type === 'button' || type === 'reset' || type === 'image') return 'button';
      if (type === 'range') return 'slider';
      if (type === 'number') return 'spinbutton';
      if (type === 'search') return 'searchbox';
      if (type === 'file') return 'button';
      return 'textbox';
    }
    if (/^H[1-6]$/.test(tag)) return 'heading';
    if (tag === 'NAV') return 'navigation';
    if (tag === 'MAIN') return 'main';
    if (tag === 'HEADER') return 'banner';
    if (tag === 'FOOTER') return 'contentinfo';
    if (tag === 'ASIDE') return 'complementary';
    if (tag === 'FORM') return 'form';
    if (tag === 'TABLE') return 'table';
    if (tag === 'UL' || tag === 'OL') return 'list';
    if (tag === 'LI') return 'listitem';
    if (tag === 'DIALOG') return 'dialog';
    if (tag === 'svg') return 'graphics';
    return '';
  }

  function labelsText(element: any) {
    const labels = element.labels;
    if (!labels || !labels.length) return '';
    const parts = [];
    for (let i = 0; i < labels.length; i += 1) {
      const text = collapse(labels[i].textContent);
      if (text) parts.push(text);
    }
    return parts.join(' ');
  }

  function accessibleName(element: any) {
    const ariaLabel = element.getAttribute ? element.getAttribute('aria-label') : null;
    if (ariaLabel && collapse(ariaLabel)) return collapse(ariaLabel);

    const labelledBy = element.getAttribute ? element.getAttribute('aria-labelledby') : null;
    if (labelledBy) {
      const parts = [];
      const ids = labelledBy.split(/\s+/);
      for (let i = 0; i < ids.length; i += 1) {
        if (!ids[i]) continue;
        const target = document.getElementById(ids[i]);
        if (target) {
          const text = collapse(target.textContent);
          if (text) parts.push(text);
        }
      }
      if (parts.length) return parts.join(' ');
    }

    const tag = element.tagName;
    if (tag === 'IMG' || tag === 'AREA') {
      const alt = element.getAttribute('alt');
      if (alt && collapse(alt)) return collapse(alt);
    }
    if (tag === 'INPUT') {
      const type = String(element.type || 'text').toLowerCase();
      if (type === 'submit' || type === 'button' || type === 'reset') {
        if (element.value) return collapse(element.value);
      }
      const fromLabels = labelsText(element);
      if (fromLabels) return fromLabels;
      if (element.placeholder) return collapse(element.placeholder);
    }
    if (tag === 'TEXTAREA') {
      const fromLabels = labelsText(element);
      if (fromLabels) return fromLabels;
      if (element.placeholder) return collapse(element.placeholder);
    }
    if (tag === 'SELECT') {
      const fromLabels = labelsText(element);
      if (fromLabels) return fromLabels;
    }
    const title = element.getAttribute ? element.getAttribute('title') : null;
    if (title && collapse(title)) return collapse(title);

    const text = collapse(element.textContent);
    if (text) return text;
    if (tag === 'INPUT' && element.value) return collapse(element.value);
    return '';
  }

  function statesOf(element: any) {
    const states: Record<string, any> = {};
    if (element.disabled === true || element.getAttribute('aria-disabled') === 'true') states.disabled = true;
    if (typeof element.checked === 'boolean' && (element.type === 'checkbox' || element.type === 'radio')) {
      states.checked = element.indeterminate ? 'mixed' : element.checked;
    }
    const expanded = element.getAttribute('aria-expanded');
    if (expanded !== null) states.expanded = expanded === 'true';
    const selected = element.getAttribute('aria-selected');
    if (selected !== null) states.selected = selected === 'true';
    if (document.activeElement === element) states.focused = true;
    if (element.required === true || element.getAttribute('aria-required') === 'true') states.required = true;
    return states;
  }

  function isInteresting(element: any) {
    const tag = element.tagName;
    if (tag === 'A') return element.hasAttribute('href') || element.hasAttribute('role');
    if (
      tag === 'BUTTON' ||
      tag === 'INPUT' ||
      tag === 'SELECT' ||
      tag === 'TEXTAREA' ||
      tag === 'SUMMARY' ||
      tag === 'IFRAME' ||
      tag === 'DIALOG'
    )
      return true;
    if (/^H[1-6]$/.test(tag)) return true;
    if (LANDMARKS[tag] === true) return true;
    if (tag === 'IMG') return element.hasAttribute('alt') || element.hasAttribute('aria-label');
    if (tag === 'svg')
      return element.hasAttribute('role') || element.hasAttribute('aria-label') || !!element.querySelector('title');
    if (element.hasAttribute('role')) return true;
    if (element.hasAttribute('tabindex')) return true;
    if (element.hasAttribute('contenteditable')) return true;
    if (element.isContentEditable === true) return true;
    return false;
  }

  const INTERACTIVE_ROLES: Record<string, any> = {
    button: true,
    link: true,
    checkbox: true,
    radio: true,
    textbox: true,
    combobox: true,
    listbox: true,
    menuitem: true,
    menuitemcheckbox: true,
    menuitemradio: true,
    option: true,
    slider: true,
    spinbutton: true,
    switch: true,
    tab: true,
    treeitem: true,
    searchbox: true,
  };

  function isActionable(element: any) {
    const tag = element.tagName;
    if (tag === 'A') return element.hasAttribute('href') || element.hasAttribute('role');
    if (tag === 'BUTTON' || tag === 'INPUT' || tag === 'SELECT' || tag === 'TEXTAREA' || tag === 'SUMMARY') return true;
    const role = String(element.getAttribute('role') || '').toLowerCase();
    if (INTERACTIVE_ROLES[role] === true) return true;
    if (element.isContentEditable === true || element.hasAttribute('contenteditable')) return true;
    const tabindex = element.getAttribute('tabindex');
    if (tabindex !== null && Number(tabindex) >= 0) return true;
    return false;
  }

  function buildNode(element: any, depth: any, rect: any) {
    const box = rect || element.getBoundingClientRect();
    const tag = element.tagName.toLowerCase();
    const node: any = {
      kind: 'element',
      ref: refFor(element),
      tag: tag,
      role: roleOf(element) || null,
      name: truncate(accessibleName(element), MAX_NAME),
      bbox: {
        x: Math.round(box.left),
        y: Math.round(box.top),
        w: Math.round(box.width),
        h: Math.round(box.height),
      },
      depth: depth,
      states: statesOf(element),
    };

    const attrs: Record<string, any> = {};
    if (tag === 'a' && element.href) attrs.href = truncate(element.href, 300);
    if (tag === 'input' || tag === 'textarea' || tag === 'select') {
      attrs.type = String(element.type || tag).toLowerCase();
      if (attrs.type === 'password') {
        attrs.value = element.value ? '***' : '';
      } else if (element.value !== undefined && element.value !== null) {
        attrs.value = truncate(element.value, 120);
      }
      if (element.placeholder) attrs.placeholder = truncate(element.placeholder, 120);
    }
    if (element.id) attrs.id = element.id;
    if (element.getAttribute && element.getAttribute('name')) attrs.name = truncate(element.getAttribute('name'), 80);
    if (tag === 'h1' || tag === 'h2' || tag === 'h3' || tag === 'h4' || tag === 'h5' || tag === 'h6') {
      attrs.level = Number(tag.slice(1));
    }
    if (Object.keys(attrs).length > 0) node.attributes = attrs;
    return node;
  }

  function compactEscaped(value: any, limit: any) {
    const normalized = String(value === undefined || value === null ? '' : value)
      .replace(/\r\n?/g, '\n')
      .replace(/[\n\r\t\f\v]+/g, ' ')
      .replace(/\\/g, '\\\\')
      .replace(/"/g, '\\"');
    if (normalized.length <= limit) return normalized;
    let cut = normalized.slice(0, limit - 1);
    if (cut.charAt(cut.length - 1) === '\\') cut = cut.slice(0, -1);
    return cut + '\u2026';
  }

  function compactStates(states: any) {
    const active = states || {};
    const tokens = [];
    if (active.disabled === true) tokens.push('disabled');
    if (active.checked === true) tokens.push('checked');
    if (active.checked === 'mixed' || active.mixed === true) tokens.push('mixed');
    if (active.focused === true) tokens.push('focused');
    if (active.required === true) tokens.push('required');
    if (active.expanded === true) tokens.push('expanded');
    if (active.selected === true) tokens.push('selected');
    return tokens;
  }

  function compactValue(node: any) {
    const attrs = node.attributes;
    if (!attrs || !Object.prototype.hasOwnProperty.call(attrs, 'value')) return '';
    const value = attrs.value;
    if (typeof value !== 'string' || value.length === 0) return '';
    return compactEscaped(value, COMPACT_VALUE_LIMIT);
  }

  function compactLineForNode(node: any, includeBbox: any) {
    if (node.kind === 'text') {
      return '# "' + compactEscaped(node.text, MAX_TEXT) + '"';
    }
    const role = String(node.role || node.tag || '').toLowerCase();
    let line = node.ref + ' ' + role + ' "' + compactEscaped(node.name, COMPACT_NAME_LIMIT) + '"';
    const states = compactStates(node.states);
    if (states.length > 0) line += ' [' + states.join(',') + ']';
    const value = compactValue(node);
    if (value) line += ' value="' + value + '"';
    if (includeBbox) {
      const bbox = node.bbox || { x: 0, y: 0, w: 0, h: 0 };
      line += ' bbox=' + bbox.x + ',' + bbox.y + ',' + bbox.w + ',' + bbox.h;
    }
    return line;
  }

  function visualViewportBounds() {
    const view = window.visualViewport;
    if (view && Number.isFinite(view.width) && Number.isFinite(view.height) && view.width > 0 && view.height > 0) {
      const left = Number.isFinite(view.offsetLeft) ? view.offsetLeft : 0;
      const top = Number.isFinite(view.offsetTop) ? view.offsetTop : 0;
      return { left: left, top: top, right: left + view.width, bottom: top + view.height };
    }
    return {
      left: 0,
      top: 0,
      right: Math.max(0, Number(window.innerWidth) || 0),
      bottom: Math.max(0, Number(window.innerHeight) || 0),
    };
  }

  function rectIntersectsBounds(rect: any, bounds: any) {
    if (!rect || !bounds) return false;
    return rect.right > bounds.left && rect.left < bounds.right && rect.bottom > bounds.top && rect.top < bounds.bottom;
  }

  function textBoundingRect(node: any) {
    if (!node || !node.parentNode || typeof document.createRange !== 'function') return null;
    try {
      const range = document.createRange();
      range.selectNodeContents(node);
      const rect = range.getBoundingClientRect();
      if (!rect || (rect.width === 0 && rect.height === 0)) return null;
      return rect;
    } catch (error: any) {
      return null;
    }
  }

  function snapshot(args: any) {
    const cap = clampNodes(args.max_nodes);
    let format = 'compact';
    if (args.format !== undefined && args.format !== null) {
      const requested = String(args.format).trim().toLowerCase();
      if (requested !== 'compact' && requested !== 'json') {
        throw badArgs('args.format must be "compact" or "json"', { format: truncate(String(args.format), 40) });
      }
      format = requested;
    }
    const viewportOnly = readBooleanFlag(args.viewport_only, false, 'args.viewport_only');
    const includeBbox = readBooleanFlag(args.include_bbox, false, 'args.include_bbox');
    let mode = 'interactive';
    if (args.mode !== undefined && args.mode !== null) {
      const requested = String(args.mode).trim().toLowerCase();
      if (requested !== 'interactive' && requested !== 'full') {
        throw badArgs('args.mode must be "interactive" or "full"', { mode: truncate(String(args.mode), 40) });
      }
      mode = requested;
    }
    const interesting = mode === 'interactive' ? isActionable : isInteresting;
    const bounds = viewportOnly ? visualViewportBounds() : null;
    const nodes: any[] = [];
    const seenRefs = new Set();
    let elementCount = 0;
    let textCount = 0;
    let truncated = false;

    function budgetLeft() {
      if (nodes.length >= cap) {
        truncated = true;
        return false;
      }
      return true;
    }

    function inViewport(rect: any) {
      return !viewportOnly || rectIntersectsBounds(rect, bounds);
    }

    function emitText(node: any, depth: any) {
      if (!budgetLeft()) return;
      const text = collapse(node.nodeValue);
      if (!text) return;
      if (viewportOnly && !inViewport(textBoundingRect(node))) return;
      nodes.push({ kind: 'text', depth: depth, text: truncate(text, MAX_TEXT) });
      textCount += 1;
    }

    function walk(element: any, depth: any) {
      if (!budgetLeft()) return;
      if (!element || element.nodeType !== 1) return;
      if (depth > MAX_DEPTH) {
        truncated = true;
        return;
      }
      const tag = element.tagName;
      if (SKIP_TAGS[tag] === true) return;
      if (!isRendered(element)) return;

      if (interesting(element)) {
        const rect = element.getBoundingClientRect();
        if (inViewport(rect)) {
          const node = buildNode(element, depth, rect);
          if (!seenRefs.has(node.ref)) {
            nodes.push(node);
            seenRefs.add(node.ref);
            elementCount += 1;
          }
        }
        const children = element.children;
        for (let i = 0; i < children.length; i += 1) {
          if (!budgetLeft()) return;
          walk(children[i], depth + 1);
        }
        return;
      }

      const childNodes = element.childNodes;
      for (let i = 0; i < childNodes.length; i += 1) {
        if (!budgetLeft()) return;
        const child = childNodes[i];
        if (child.nodeType === 3) {
          emitText(child, depth);
        } else if (child.nodeType === 1) {
          walk(child, depth + 1);
        }
      }
    }

    walk(document.documentElement, 0);

    const viewport = {
      width: window.innerWidth,
      height: window.innerHeight,
      scrollX: Math.round(window.scrollX),
      scrollY: Math.round(window.scrollY),
    };
    if (format === 'compact') {
      const lines = [];
      for (let i = 0; i < nodes.length; i += 1) {
        lines.push(compactLineForNode(nodes[i], includeBbox));
      }
      return {
        format: 'compact',
        url: location.href,
        title: truncate(document.title, 300),
        viewport: viewport,
        mode: mode,
        node_count: lines.length,
        truncated: truncated,
        text: lines.join('\n'),
      };
    }

    return {
      format: 'json',
      url: location.href,
      title: truncate(document.title, 300),
      viewport: viewport,
      mode: mode,
      node_cap: cap,
      node_count: nodes.length,
      element_count: elementCount,
      text_count: textCount,
      ref_count: refToElement.size,
      truncated: truncated,
      nodes: nodes,
    };
  }

  // -------------------------------------------------------------------------
  // Page text, HTML and find
  // -------------------------------------------------------------------------

  function pageText(args: any) {
    const maxChars = readIntegerRange(args.max_chars, DEFAULT_PAGE_TEXT, 1, MAX_PAGE_TEXT, 'args.max_chars');
    const full = String(visibleBodyText()).replace(/\r\n/g, '\n').replace(/\r/g, '\n');
    const truncated = full.length > maxChars;
    return {
      url: location.href,
      title: document.title,
      text: truncated ? full.slice(0, maxChars) : full,
      truncated: truncated,
    };
  }

  function pageHtml(args: any) {
    const maxChars = readIntegerRange(args.max_chars, DEFAULT_PAGE_HTML, 1, MAX_PAGE_HTML, 'args.max_chars');
    let element = document.documentElement;
    if (args.target !== undefined && args.target !== null) {
      element = resolveTarget(args, 'page_html').element;
    }
    const full = element && typeof element.outerHTML === 'string' ? element.outerHTML : '';
    const truncated = full.length > maxChars;
    return {
      url: location.href,
      html: truncated ? full.slice(0, maxChars) : full,
      truncated: truncated,
    };
  }

  function findMatcher(query: any, regex: any) {
    if (regex === true) {
      try {
        const compiled = new RegExp(query, 'i');
        return function matchRegex(value: any) {
          return compiled.test(value);
        };
      } catch (error: any) {
        throw badArgs(
          'args.query is not a valid regular expression: ' + String(error && error.message ? error.message : error),
          {
            query: truncate(query, 120),
          },
        );
      }
    }
    const needle = collapse(query).toLowerCase();
    if (!needle) throw badArgs('args.query must contain searchable text');
    return function matchLiteral(value: any) {
      return String(value).toLowerCase().indexOf(needle) !== -1;
    };
  }

  function closestFindCandidate(element: any) {
    if (!element || element.nodeType !== 1) return null;
    let candidate = element;
    try {
      const actionable = element.closest(
        'a[href],button,input,select,textarea,summary,[role],[contenteditable],[tabindex]',
      );
      if (actionable) candidate = actionable;
    } catch (error: any) {
      candidate = element;
    }
    if (!candidate || candidate.nodeType !== 1 || !candidate.isConnected) return null;
    return candidate;
  }

  function pageFind(args: any) {
    const rawQuery = args && typeof args.query === 'string' ? args.query : '';
    if (!rawQuery.trim()) throw badArgs('args.query must be a non-empty string');
    if (args.regex !== undefined && args.regex !== null && typeof args.regex !== 'boolean') {
      throw badArgs('args.regex must be a boolean', { regex: args.regex });
    }
    let roleFilter = '';
    if (args.role !== undefined && args.role !== null) {
      if (typeof args.role !== 'string' || !args.role.trim()) {
        throw badArgs('args.role must be a non-empty string when provided');
      }
      roleFilter = args.role.trim().toLowerCase();
    }
    const limit = readIntegerRange(args.limit, DEFAULT_FIND_LIMIT, 1, MAX_FIND_LIMIT, 'args.limit');
    const matcher = findMatcher(rawQuery, args.regex === true);
    const matches: any[] = [];
    const seen = new Set();

    function pushMatch(candidate: any, text: any) {
      if (matches.length >= limit) return;
      if (!candidate || candidate.nodeType !== 1 || !candidate.isConnected) return;
      if (!isRendered(candidate)) return;
      const role = roleOf(candidate).toLowerCase();
      if (roleFilter && role !== roleFilter) return;
      if (seen.has(candidate)) return;
      seen.add(candidate);
      const match = {
        ref: refFor(candidate),
        tag: candidate.tagName.toLowerCase(),
        role: roleOf(candidate) || null,
        text: truncate(collapse(text), MAX_TEXT),
      };
      matches.push(match);
    }

    function scanText(node: any) {
      if (matches.length >= limit || !node) return;
      if (node.nodeType === 3) {
        const text = collapse(node.nodeValue);
        if (text && matcher(text)) {
          pushMatch(closestFindCandidate(node.parentElement), text);
        }
        return;
      }
      if (node.nodeType !== 1) return;
      if (SKIP_TAGS[node.tagName] === true) return;
      if (!isRendered(node)) return;
      const children = node.childNodes;
      for (let i = 0; i < children.length; i += 1) {
        if (matches.length >= limit) return;
        scanText(children[i]);
      }
    }

    scanText(document.body || document.documentElement);

    // Controls expose their text through value and accessible name, not only
    // through text nodes, so run a bounded second pass over form controls.
    if (matches.length < limit) {
      let controls: any = [];
      try {
        controls = document.querySelectorAll(
          'input, textarea, select, button, a[href], [role], [contenteditable], [tabindex]',
        );
      } catch (error: any) {
        controls = [];
      }
      const controlLimit = Math.min(controls.length, 2000);
      for (let i = 0; i < controlLimit && matches.length < limit; i += 1) {
        const control = controls[i];
        if (!isRendered(control)) continue;
        const pieces = [accessibleName(control)];
        if (control.value !== undefined && control.value !== null) pieces.push(String(control.value));
        if (control.placeholder) pieces.push(String(control.placeholder));
        const haystack = collapse(pieces.join(' '));
        if (haystack && matcher(haystack)) pushMatch(closestFindCandidate(control), haystack);
      }
    }

    return {
      url: location.href,
      count: matches.length,
      matches: matches,
    };
  }

  // -------------------------------------------------------------------------
  // Hover
  // -------------------------------------------------------------------------

  function hoverElement(args: any) {
    const target = resolveTarget(args, 'hover');
    const element = target.element;
    assertInteractable(element, 'hover');
    scrollIntoView(element);

    const rect = element.getBoundingClientRect();
    const point = hitTest(element, rect, 'hover');
    const fired = [];
    if (dispatchPointer(element, 'pointerover', point)) fired.push('pointerover');
    if (dispatchPointer(element, 'pointerenter', point)) fired.push('pointerenter');
    if (dispatchPointer(element, 'pointermove', point)) fired.push('pointermove');
    if (dispatchMouse(element, 'mouseover', point)) fired.push('mouseover');
    if (dispatchMouse(element, 'mouseenter', point)) fired.push('mouseenter');
    if (dispatchMouse(element, 'mousemove', point)) fired.push('mousemove');

    const targetInfo: any = {
      ref: elementToRef.get(element) || refFor(element),
      tag: element.tagName.toLowerCase(),
    };
    const text = truncate(accessibleName(element), MAX_NAME);
    if (text) targetInfo.text = text;

    return {
      hovered: true,
      url: location.href,
      target: targetInfo,
    };
  }

  // -------------------------------------------------------------------------
  // Press
  // -------------------------------------------------------------------------

  const NAMED_KEYS: Record<string, any> = {
    enter: { key: 'Enter', code: 'Enter', keyCode: 13, printable: true, char: '\r' },
    tab: { key: 'Tab', code: 'Tab', keyCode: 9, printable: false },
    escape: { key: 'Escape', code: 'Escape', keyCode: 27, printable: false },
    backspace: { key: 'Backspace', code: 'Backspace', keyCode: 8, printable: false },
    delete: { key: 'Delete', code: 'Delete', keyCode: 46, printable: false },
    space: { key: ' ', code: 'Space', keyCode: 32, printable: true, char: ' ' },
    arrowup: { key: 'ArrowUp', code: 'ArrowUp', keyCode: 38, printable: false },
    arrowdown: { key: 'ArrowDown', code: 'ArrowDown', keyCode: 40, printable: false },
    arrowleft: { key: 'ArrowLeft', code: 'ArrowLeft', keyCode: 37, printable: false },
    arrowright: { key: 'ArrowRight', code: 'ArrowRight', keyCode: 39, printable: false },
    pageup: { key: 'PageUp', code: 'PageUp', keyCode: 33, printable: false },
    pagedown: { key: 'PageDown', code: 'PageDown', keyCode: 34, printable: false },
    home: { key: 'Home', code: 'Home', keyCode: 36, printable: false },
    end: { key: 'End', code: 'End', keyCode: 35, printable: false },
  };

  const MODIFIER_KEYS: Record<string, any> = {
    control: { name: 'Control', key: 'Control', code: 'ControlLeft', keyCode: 17, prop: 'ctrlKey' },
    shift: { name: 'Shift', key: 'Shift', code: 'ShiftLeft', keyCode: 16, prop: 'shiftKey' },
    alt: { name: 'Alt', key: 'Alt', code: 'AltLeft', keyCode: 18, prop: 'altKey' },
    meta: { name: 'Meta', key: 'Meta', code: 'MetaLeft', keyCode: 91, prop: 'metaKey' },
  };

  const SHIFTED_DIGITS: Record<string, any> = {
    '1': '!',
    '2': '@',
    '3': '#',
    '4': '$',
    '5': '%',
    '6': '^',
    '7': '&',
    '8': '*',
    '9': '(',
    '0': ')',
  };

  function modifierFor(token: any) {
    return MODIFIER_KEYS[String(token).trim().toLowerCase()] || null;
  }

  function keyForToken(token: any, shift: any) {
    const text = String(token).trim();
    const named = NAMED_KEYS[text.toLowerCase()];
    if (named) {
      return {
        key: named.key,
        code: named.code,
        keyCode: named.keyCode,
        printable: named.printable,
        char: named.char,
      };
    }
    const modifier = modifierFor(text);
    if (modifier) {
      return { key: modifier.key, code: modifier.code, keyCode: modifier.keyCode, printable: false };
    }
    const keyCodeMatch = /^Key([A-Za-z])$/i.exec(text);
    if (keyCodeMatch) {
      const letter = keyCodeMatch[1].toUpperCase();
      return {
        key: shift ? letter : letter.toLowerCase(),
        code: 'Key' + letter,
        keyCode: letter.charCodeAt(0),
        printable: true,
        char: shift ? letter : letter.toLowerCase(),
      };
    }
    const digitMatch = /^Digit([0-9])$/i.exec(text);
    if (digitMatch) {
      const digit = digitMatch[1];
      return {
        key: shift && SHIFTED_DIGITS[digit] ? SHIFTED_DIGITS[digit] : digit,
        code: 'Digit' + digit,
        keyCode: digit.charCodeAt(0),
        printable: true,
        char: shift && SHIFTED_DIGITS[digit] ? SHIFTED_DIGITS[digit] : digit,
      };
    }
    const characters = Array.from(text);
    if (characters.length === 1) {
      const character = characters[0];
      const upper = character.toUpperCase();
      let code = '';
      if (/^[A-Za-z]$/.test(character)) code = 'Key' + upper;
      else if (/^[0-9]$/.test(character)) code = 'Digit' + character;
      return {
        key: shift ? upper : character,
        code: code,
        keyCode: upper.charCodeAt(0),
        printable: true,
        char: shift ? upper : character,
      };
    }
    throw badArgs('unsupported key name: ' + truncate(text, 40), { key: truncate(text, 40) });
  }

  function parseKeyChord(raw: any) {
    if (typeof raw !== 'string' || !raw.trim()) {
      throw badArgs('keys entries must be non-empty strings');
    }
    const text = raw.trim();
    let parts = null;
    if (text === '+') {
      parts = ['+'];
    } else if (text.length > 2 && text.slice(-2) === '++') {
      parts = text.slice(0, -2).split('+').concat('+');
    } else {
      parts = text.split('+');
    }
    if (parts.some((part: any) => part.trim() === '')) {
      throw badArgs('invalid key chord: ' + truncate(text, 60), { key: truncate(text, 60) });
    }
    const keyToken = parts.pop();
    const modifiers: any[] = [];
    let shift = false;
    for (let i = 0; i < parts.length; i += 1) {
      const modifier = modifierFor(parts[i]);
      if (!modifier) {
        throw badArgs('unsupported modifier: ' + truncate(parts[i], 40), { key: truncate(text, 60) });
      }
      if (!modifiers.some((item: any) => item.prop === modifier.prop)) modifiers.push(modifier);
      if (modifier.prop === 'shiftKey') shift = true;
    }
    return { text: text, modifiers: modifiers, key: keyForToken(keyToken, shift) };
  }

  function keyboardFlags(modifiers: any) {
    const flags: Record<string, any> = { ctrlKey: false, shiftKey: false, altKey: false, metaKey: false };
    for (let i = 0; i < modifiers.length; i += 1) flags[modifiers[i].prop] = true;
    return flags;
  }

  function emitKeyboardEvent(target: any, type: any, keyDefinition: any, flags: any, charCode: any) {
    const init = {
      bubbles: true,
      cancelable: true,
      composed: true,
      view: window,
      key: keyDefinition.key,
      code: keyDefinition.code || '',
      location: 0,
      ctrlKey: flags.ctrlKey === true,
      shiftKey: flags.shiftKey === true,
      altKey: flags.altKey === true,
      metaKey: flags.metaKey === true,
      repeat: false,
      isComposing: false,
      charCode: charCode || 0,
    };
    let event = null;
    try {
      event = new KeyboardEvent(type, init);
    } catch (error: any) {
      try {
        event = new Event(type, { bubbles: true, cancelable: true, composed: true });
      } catch (inner: any) {
        return false;
      }
    }
    try {
      Object.defineProperty(event, 'keyCode', {
        configurable: true,
        get: function getKeyCode() {
          return keyDefinition.keyCode;
        },
      });
      Object.defineProperty(event, 'which', {
        configurable: true,
        get: function getWhich() {
          return type === 'keypress' ? charCode || 0 : keyDefinition.keyCode;
        },
      });
    } catch (error: any) {
      // Legacy fields are best effort.
    }
    try {
      target.dispatchEvent(event);
      return true;
    } catch (error: any) {
      return false;
    }
  }

  function pressKeys(args: any) {
    const raw = args.keys;
    let entries = null;
    if (typeof raw === 'string') {
      entries = [raw];
    } else if (Array.isArray(raw)) {
      entries = raw.slice();
    } else {
      throw badArgs('args.keys must be a string or an array of strings');
    }
    if (entries.length === 0) throw badArgs('args.keys must contain at least one key');
    const chords = entries.map(parseKeyChord);

    let target = null;
    if (args.target !== undefined && args.target !== null) {
      target = resolveTarget(args, 'press').element;
      assertInteractable(target, 'press');
      scrollIntoView(target);
      focusElement(target);
    } else {
      target = document.activeElement;
      if (!target || target === document.documentElement) target = document.body || document.documentElement;
    }
    if (!target) throw new ToolError('NOT_INTERACTABLE', 'no element can receive key events', { action: 'press' });

    for (let i = 0; i < chords.length; i += 1) {
      const chord = chords[i];
      const activeFlags: Record<string, boolean> = { ctrlKey: false, shiftKey: false, altKey: false, metaKey: false };
      for (let m = 0; m < chord.modifiers.length; m += 1) {
        const modifier = chord.modifiers[m];
        activeFlags[modifier.prop] = true;
        emitKeyboardEvent(target, 'keydown', modifier, activeFlags, 0);
      }
      const flags = keyboardFlags(chord.modifiers);
      emitKeyboardEvent(target, 'keydown', chord.key, flags, 0);
      if (chord.key.printable) {
        const charCode = chord.key.char !== undefined ? chord.key.char.charCodeAt(0) : chord.key.keyCode;
        emitKeyboardEvent(target, 'keypress', chord.key, flags, charCode);
      }
      emitKeyboardEvent(target, 'keyup', chord.key, flags, 0);
      for (let m = chord.modifiers.length - 1; m >= 0; m -= 1) {
        const modifier = chord.modifiers[m];
        emitKeyboardEvent(target, 'keyup', modifier, activeFlags, 0);
        activeFlags[modifier.prop] = false;
      }
    }

    return {
      pressed: true,
      keys: chords.map((chord: any) => chord.text),
      url: location.href,
    };
  }

  // -------------------------------------------------------------------------
  // Scroll
  // -------------------------------------------------------------------------

  function scrollPage(args: any) {
    const rawDirection = args && typeof args.direction === 'string' ? args.direction.trim().toLowerCase() : '';
    if (rawDirection !== 'up' && rawDirection !== 'down' && rawDirection !== 'left' && rawDirection !== 'right') {
      throw badArgs('args.direction must be up, down, left or right', {
        direction: truncate(String(args && args.direction), 40),
      });
    }
    let pages = 1;
    if (args.pages !== undefined && args.pages !== null) {
      if (typeof args.pages !== 'number') {
        throw badArgs('args.pages must be a number', { pages: args.pages });
      }
      pages = args.pages;
      if (!Number.isFinite(pages) || pages < 0 || pages > MAX_SCROLL_PAGES) {
        throw badArgs('args.pages must be a number from 0 to ' + MAX_SCROLL_PAGES, { pages: args.pages });
      }
    }

    let element = null;
    if (args.target !== undefined && args.target !== null) {
      element = resolveTarget(args, 'scroll').element;
      if (!isRendered(element)) {
        throw notInteractable('the scroll target is not visible', {
          action: 'scroll',
          element: describeElement(element),
        });
      }
    }

    let deltaX = 0;
    let deltaY = 0;
    const windowTarget = !element || element === document.body || element === document.documentElement;
    const unitY = windowTarget ? window.innerHeight : element.clientHeight;
    const unitX = windowTarget ? window.innerWidth : element.clientWidth;
    if (rawDirection === 'up') deltaY = -Math.round(unitY * pages);
    if (rawDirection === 'down') deltaY = Math.round(unitY * pages);
    if (rawDirection === 'left') deltaX = -Math.round(unitX * pages);
    if (rawDirection === 'right') deltaX = Math.round(unitX * pages);

    if (windowTarget) {
      try {
        window.scrollBy({ left: deltaX, top: deltaY, behavior: 'instant' });
      } catch (error: any) {
        window.scrollTo(window.scrollX + deltaX, window.scrollY + deltaY);
      }
      return {
        scrolled: true,
        x: Math.round(window.scrollX),
        y: Math.round(window.scrollY),
        url: location.href,
      };
    }

    try {
      element.scrollBy({ left: deltaX, top: deltaY, behavior: 'instant' });
    } catch (error: any) {
      element.scrollLeft += deltaX;
      element.scrollTop += deltaY;
    }
    return {
      scrolled: true,
      x: Math.round(element.scrollLeft),
      y: Math.round(element.scrollTop),
      url: location.href,
    };
  }

  // -------------------------------------------------------------------------
  // Select
  // -------------------------------------------------------------------------

  function fireSelectEvent(element: any, type: any) {
    try {
      element.dispatchEvent(new Event(type, { bubbles: true, composed: true }));
      return true;
    } catch (error: any) {
      return false;
    }
  }

  function selectOptions(args: any) {
    const target = resolveTarget(args, 'select');
    const element = target.element;
    if (!element || element.tagName !== 'SELECT') {
      throw badArgs('browser_select target must be a <select> element', {
        action: 'select',
        element: describeElement(element),
      });
    }
    assertInteractable(element, 'select');

    if (!Array.isArray(args.values)) {
      throw badArgs('args.values must be an array of option values');
    }
    if (args.values.some((value: any) => typeof value !== 'string')) {
      throw badArgs('every entry of args.values must be a string');
    }
    const values = args.values.slice();
    if (values.length === 0) throw badArgs('args.values must contain at least one option value');
    if (element.multiple !== true && values.length !== 1) {
      throw badArgs('a single-select element needs exactly one value', { count: values.length });
    }

    const options = Array.prototype.slice.call(element.options);
    for (let i = 0; i < values.length; i += 1) {
      if (!options.some((option: any) => option.value === values[i])) {
        throw notFound('no option has the requested value', { value: values[i] });
      }
    }

    if (element.multiple === true) {
      for (let i = 0; i < options.length; i += 1) {
        options[i].selected = values.indexOf(options[i].value) !== -1;
      }
    } else {
      let chosen = null;
      for (let i = 0; i < options.length; i += 1) {
        if (!chosen && options[i].value === values[0]) chosen = options[i];
        options[i].selected = options[i] === chosen;
      }
    }

    fireSelectEvent(element, 'input');
    fireSelectEvent(element, 'change');

    const selected = element.selectedOptions
      ? Array.prototype.slice.call(element.selectedOptions).map((option: any) => String(option.value))
      : options.filter((option: any) => option.selected).map((option: any) => String(option.value));
    return {
      selected: selected,
      value: String(element.value === undefined || element.value === null ? '' : element.value),
      url: location.href,
    };
  }

  // -------------------------------------------------------------------------
  // Wait probe
  // -------------------------------------------------------------------------

  function waitProbe(message: any) {
    const selector = typeof message.selector === 'string' && message.selector.trim() ? message.selector.trim() : '';
    const text = typeof message.text === 'string' && message.text.trim() ? message.text : '';
    if (!selector && !text) throw badArgs('wait probe needs a selector or text');

    let matched = true;
    if (selector) {
      try {
        matched = document.querySelector(selector) !== null;
      } catch (error: any) {
        throw badArgs('invalid CSS selector: ' + String(error && error.message ? error.message : error), {
          selector: truncate(selector, 80),
        });
      }
    }
    if (matched && text) {
      matched = visibleBodyText().toLowerCase().indexOf(text.toLowerCase()) !== -1;
    }
    return { ok: true, matched: matched, url: location.href };
  }

  // -------------------------------------------------------------------------
  // Target resolution
  // -------------------------------------------------------------------------

  function resolveSelector(selector: any) {
    if (selector.length > MAX_SELECTOR) {
      throw badArgs('selector is longer than ' + MAX_SELECTOR + ' characters', { selector: truncate(selector, 80) });
    }
    let element = null;
    try {
      element = document.querySelector(selector);
    } catch (error: any) {
      throw badArgs('invalid CSS selector: ' + String(error && error.message ? error.message : error), {
        selector: truncate(selector, 80),
      });
    }
    if (!element) throw notFound('no element matches the selector', { selector: truncate(selector, 120) });
    return element;
  }

  function resolveTarget(args: any, action: any) {
    // The frozen public argument is "target" ("@eN" or a CSS selector).
    // "ref" and "selector" remain accepted aliases; target takes precedence.
    const rawRef = typeof args.ref === 'string' ? args.ref.trim() : '';
    const rawTarget = typeof args.target === 'string' ? args.target.trim() : '';
    const rawSelector = typeof args.selector === 'string' ? args.selector.trim() : '';

    function fromRef(value: any, label: any) {
      const element = resolveRef(value);
      if (!element) {
        const details: Record<string, any> = { action: action };
        details[label] = value;
        throw notFound('the element reference is unknown or stale; take a new snapshot', details);
      }
      return { element: element, ref: elementToRef.get(element) || value, selector: null };
    }

    function fromSelector(value: any) {
      const element = resolveSelector(value);
      return { element: element, ref: refFor(element), selector: value };
    }

    if (rawTarget) {
      if (REF_PATTERN.test(rawTarget)) return fromRef(rawTarget, 'target');
      return fromSelector(rawTarget);
    }
    if (rawRef) {
      if (!REF_PATTERN.test(rawRef)) {
        throw badArgs('args.ref must look like @e12', { ref: truncate(rawRef, 40), action: action });
      }
      return fromRef(rawRef, 'ref');
    }
    if (rawSelector) return fromSelector(rawSelector);
    throw badArgs('args.target is required; args.ref and args.selector are accepted aliases', { action: action });
  }

  // Returns a selector that resolves back to this exact element. A unique id
  // is the shortest form; otherwise a tag and nth-of-type chain is built.
  function escapeCssIdentifier(value: any) {
    if (typeof CSS !== 'undefined' && CSS && typeof CSS.escape === 'function') return CSS.escape(String(value));
    return String(value);
  }

  function cssSelectorFor(element: any) {
    if (!element || element.nodeType !== 1) return null;
    const doc = element.ownerDocument || document;

    function isUnique(selector: any) {
      try {
        const matches = doc.querySelectorAll(selector);
        return matches.length === 1 && matches[0] === element;
      } catch (error: any) {
        return false;
      }
    }

    if (element.id) {
      const idSelector = '#' + escapeCssIdentifier(element.id);
      if (isUnique(idSelector)) return idSelector;
    }

    const parts = [];
    let node = element;
    while (node && node.nodeType === 1) {
      let part = node.tagName ? node.tagName.toLowerCase() : '';
      if (!part) return null;
      const parent = node.parentElement;
      if (parent) {
        let index = 1;
        let sibling = node;
        while ((sibling = sibling.previousElementSibling)) {
          if (sibling.tagName === node.tagName) index += 1;
        }
        let sameTagCount = 0;
        const children = parent.children;
        for (let i = 0; i < children.length; i += 1) {
          if (children[i].tagName === node.tagName) sameTagCount += 1;
        }
        if (sameTagCount > 1) part += ':nth-of-type(' + index + ')';
      }
      parts.unshift(part);
      if (node === doc.documentElement) break;
      node = parent;
    }

    const selector = parts.join(' > ');
    if (!selector) return null;
    return isUnique(selector) ? selector : null;
  }

  // Internal helper for upload_file. Hidden file inputs are valid targets, so
  // this intentionally does not run the visibility checks used by click/type.
  function uploadTarget(message: any) {
    const target = message && typeof message.target === 'string' ? message.target.trim() : '';
    if (!target) throw badArgs('args.target is required; use an @eN reference or a CSS selector');
    const resolved = resolveTarget({ target: target }, 'upload_file');
    const element = resolved.element;
    const tag = element.tagName ? element.tagName.toLowerCase() : '';
    const inputType = tag === 'input' && typeof element.type === 'string' ? element.type.toLowerCase() : '';
    if (tag !== 'input' || inputType !== 'file') {
      throw badArgs('the target is not a file input', {
        reason: 'not_a_file_input',
        tag: tag,
        input_type: inputType || null,
      });
    }
    const selector = cssSelectorFor(element);
    if (!selector) throw notFound('the file input has no stable selector', { tag: 'input' });
    let text = '';
    try {
      text = truncate(collapse(accessibleName(element) || ''), MAX_NAME);
    } catch (error: any) {
      text = '';
    }
    return {
      selector: selector,
      ref: resolved.ref || null,
      tag: 'input',
      text: text,
      accept: typeof element.accept === 'string' && element.accept ? element.accept : null,
      multiple: element.multiple === true,
    };
  }

  function assertInteractable(element: any, action: any) {
    if (!isRendered(element)) {
      throw notInteractable('the element is not visible', { action: action, element: describeElement(element) });
    }
    if (element.disabled === true || (element.getAttribute && element.getAttribute('aria-disabled') === 'true')) {
      throw notInteractable('the element is disabled', { action: action, element: describeElement(element) });
    }
    const rect = element.getBoundingClientRect();
    if (rect.width < 1 || rect.height < 1) {
      throw notInteractable('the element has no hit area', {
        action: action,
        element: describeElement(element),
        bbox: {
          x: Math.round(rect.left),
          y: Math.round(rect.top),
          w: Math.round(rect.width),
          h: Math.round(rect.height),
        },
      });
    }
    return rect;
  }

  function hitTest(element: any, rect: any, action: any) {
    const viewportWidth = Math.max(window.innerWidth, 1);
    const viewportHeight = Math.max(window.innerHeight, 1);
    const x = Math.min(Math.max(rect.left + rect.width / 2, 0), viewportWidth - 1);
    const y = Math.min(Math.max(rect.top + rect.height / 2, 0), viewportHeight - 1);
    const hit = document.elementFromPoint(x, y);
    if (!hit) {
      throw notInteractable('the element is outside the viewport', {
        action: action,
        element: describeElement(element),
      });
    }
    if (hit === element || element.contains(hit)) {
      return { x: x, y: y, hit: hit };
    }
    throw notInteractable('another element covers the target', {
      action: action,
      element: describeElement(element),
      covering: describeElement(hit),
    });
  }

  // -------------------------------------------------------------------------
  // Click
  // -------------------------------------------------------------------------

  function dispatchPointer(element: any, type: any, point: any) {
    if (typeof window.PointerEvent !== 'function') return false;
    try {
      const event = new PointerEvent(type, {
        bubbles: type !== 'pointerenter',
        cancelable: type !== 'pointerenter',
        composed: true,
        view: window,
        detail: 1,
        clientX: point.x,
        clientY: point.y,
        screenX: window.screenX + point.x,
        screenY: window.screenY + point.y,
        button: 0,
        buttons: type === 'pointerup' ? 0 : 1,
        pointerId: 1,
        pointerType: 'mouse',
        isPrimary: true,
        width: 1,
        height: 1,
      });
      element.dispatchEvent(event);
      return true;
    } catch (error: any) {
      return false;
    }
  }

  function dispatchMouse(element: any, type: any, point: any) {
    try {
      const event = new MouseEvent(type, {
        bubbles: true,
        cancelable: true,
        composed: true,
        view: window,
        detail: 1,
        clientX: point.x,
        clientY: point.y,
        screenX: window.screenX + point.x,
        screenY: window.screenY + point.y,
        button: 0,
        buttons: type === 'mouseup' ? 0 : 1,
      });
      element.dispatchEvent(event);
      return true;
    } catch (error: any) {
      return false;
    }
  }

  function clickElement(element: any, action: any, selector: any) {
    const rect = assertInteractable(element, action);
    scrollIntoView(element);

    let currentRect = element.getBoundingClientRect();
    if (currentRect.width < 1 || currentRect.height < 1) currentRect = rect;
    const point = hitTest(element, currentRect, action);

    focusElement(element);

    const fired = [];
    if (dispatchPointer(element, 'pointerover', point)) fired.push('pointerover');
    if (dispatchPointer(element, 'pointerenter', point)) fired.push('pointerenter');
    if (dispatchPointer(element, 'pointermove', point)) fired.push('pointermove');
    if (dispatchPointer(element, 'pointerdown', point)) fired.push('pointerdown');
    if (dispatchMouse(element, 'mousedown', point)) fired.push('mousedown');
    if (dispatchPointer(element, 'pointerup', point)) fired.push('pointerup');
    if (dispatchMouse(element, 'mouseup', point)) fired.push('mouseup');
    try {
      element.click();
      fired.push('click');
    } catch (error: any) {
      throw new ToolError(
        'INTERNAL',
        'the element click threw: ' + String(error && error.message ? error.message : error),
        {
          element: describeElement(element),
        },
      );
    }

    return {
      clicked: true,
      ref: elementToRef.get(element) || null,
      selector: selector || null,
      tag: element.tagName.toLowerCase(),
      role: roleOf(element) || null,
      name: truncate(accessibleName(element), MAX_NAME),
      bbox: {
        x: Math.round(currentRect.left),
        y: Math.round(currentRect.top),
        w: Math.round(currentRect.width),
        h: Math.round(currentRect.height),
      },
      point: { x: Math.round(point.x), y: Math.round(point.y) },
      events: fired,
      url: location.href,
    };
  }

  // -------------------------------------------------------------------------
  // Type
  // -------------------------------------------------------------------------

  function isTextInput(element: any) {
    if (element.tagName === 'TEXTAREA') return true;
    if (element.tagName === 'INPUT') {
      const type = String(element.type || 'text').toLowerCase();
      return TEXT_INPUT_TYPES[type] === true;
    }
    return false;
  }

  function setNativeValue(element: any, value: any) {
    const prototype = Object.getPrototypeOf(element);
    const descriptor = prototype ? Object.getOwnPropertyDescriptor(prototype, 'value') : null;
    if (descriptor && typeof descriptor.set === 'function') {
      descriptor.set.call(element, value);
      return;
    }
    if (element.tagName === 'TEXTAREA') {
      const fallback = Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype, 'value');
      if (fallback && typeof fallback.set === 'function') {
        fallback.set.call(element, value);
        return;
      }
    }
    const inputDescriptor = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value');
    if (inputDescriptor && typeof inputDescriptor.set === 'function') {
      inputDescriptor.set.call(element, value);
      return;
    }
    element.value = value;
  }

  function createInputEvent(type: any, text: any, inputType: any) {
    try {
      return new InputEvent(type, {
        bubbles: true,
        cancelable: type === 'beforeinput',
        composed: true,
        data: text,
        inputType: inputType,
      });
    } catch (error: any) {
      try {
        return new Event(type, { bubbles: true, cancelable: type === 'beforeinput', composed: true });
      } catch (inner: any) {
        return null;
      }
    }
  }

  function fireInput(element: any, text: any, inputType: any) {
    const fired = [];
    const before = createInputEvent('beforeinput', text, inputType);
    if (before) {
      try {
        element.dispatchEvent(before);
        fired.push('beforeinput');
      } catch (error: any) {
        // Ignore one failing listener set; the input event still matters.
      }
    }
    const input = createInputEvent('input', text, inputType);
    if (input) {
      try {
        element.dispatchEvent(input);
        fired.push('input');
      } catch (error: any) {
        // Ignore.
      }
    }
    return fired;
  }

  function fireChange(element: any) {
    try {
      element.dispatchEvent(new Event('change', { bubbles: true, composed: true }));
      return true;
    } catch (error: any) {
      return false;
    }
  }

  async function typeInto(args: any) {
    const target = resolveTarget(args, 'type');
    const element = target.element;

    if (typeof args.text !== 'string') throw badArgs('args.text must be a string', { action: 'type' });
    const text = args.text;
    if (text.length > MAX_INPUT)
      throw badArgs('args.text exceeds ' + MAX_INPUT + ' characters', { action: 'type', length: text.length });
    if (args.clear !== undefined && typeof args.clear !== 'boolean') {
      throw badArgs('args.clear must be a boolean', { action: 'type' });
    }
    if (args.blur !== undefined && typeof args.blur !== 'boolean') {
      throw badArgs('args.blur must be a boolean', { action: 'type' });
    }

    const clear = args.clear !== false;
    assertInteractable(element, 'type');
    scrollIntoView(element);
    focusElement(element);
    await nextFrame();

    const editable =
      element.isContentEditable === true ||
      (element.getAttribute && element.getAttribute('contenteditable') === 'true');
    const textField = isTextInput(element);

    if (!editable && !textField) {
      throw notInteractable('the element is not a text input or editable region', {
        action: 'type',
        element: describeElement(element),
        tag: element.tagName.toLowerCase(),
      });
    }

    const events: any[] = [];
    let finalValue = '';

    if (editable) {
      if (clear) element.textContent = '';
      try {
        const inserted = document.execCommand('insertText', false, text);
        if (!inserted) element.textContent = (clear ? '' : element.textContent) + text;
      } catch (error: any) {
        element.textContent = (clear ? '' : element.textContent) + text;
      }
      events.push.apply(events, fireInput(element, text, 'insertText'));
      finalValue = String(element.textContent || '');
    } else {
      if (element.readOnly === true) {
        throw notInteractable('the input is read only', { action: 'type', element: describeElement(element) });
      }
      const previous = String(element.value === undefined || element.value === null ? '' : element.value);
      const next = clear ? text : previous + text;
      try {
        setNativeValue(element, next);
      } catch (error: any) {
        throw new ToolError(
          'INTERNAL',
          'the native value setter failed: ' + String(error && error.message ? error.message : error),
          {
            element: describeElement(element),
          },
        );
      }
      events.push.apply(events, fireInput(element, text, clear ? 'insertText' : 'insertText'));
      if (fireChange(element)) events.push('change');
      if (args.blur === true) {
        try {
          element.blur();
          events.push('blur');
        } catch (error: any) {
          // Ignore.
        }
      }
      finalValue = String(element.value === undefined || element.value === null ? '' : element.value);
    }

    const result: any = {
      typed: true,
      ref: elementToRef.get(element) || null,
      selector: target.selector || null,
      tag: element.tagName.toLowerCase(),
      role: roleOf(element) || null,
      cleared: clear,
      events: events,
      value_length: finalValue.length,
      url: location.href,
    };

    const type = String(element.type || '').toLowerCase();
    if (type === 'password') {
      result.masked = true;
    } else {
      result.value = truncate(finalValue, 400);
    }
    return result;
  }

  // -------------------------------------------------------------------------
  // Navigate
  // -------------------------------------------------------------------------

  function navigate(args: any) {
    const raw = args.url !== undefined ? args.url : args.href;
    if (typeof raw !== 'string' || !raw.trim())
      throw badArgs('args.url must be a non-empty string', { action: 'navigate' });
    if (unloading) throw new ToolError('NAVIGATION_LOST', 'the page is unloading before navigation could start');

    let resolved = null;
    try {
      resolved = new URL(raw.trim(), location.href);
    } catch (error: any) {
      throw badArgs('args.url is not a valid URL', { action: 'navigate', url: truncate(raw, 200) });
    }
    if (!NAVIGABLE_PATTERN.test(resolved.protocol)) {
      throw badArgs('only http, https and file URLs can be navigated to', {
        action: 'navigate',
        url: truncate(resolved.href, 200),
      });
    }

    const target = resolved.href;
    const result: any = {
      navigated: true,
      url: target,
      previous: location.href,
      same_document: target === location.href,
      started_at: Date.now(),
    };
    window.setTimeout(() => {
      try {
        location.assign(target);
      } catch (error: any) {
        // The reply is already on its way; a failed assignment is reported on
        // the next command by the service worker as NAVIGATION_LOST or TIMEOUT.
      }
    }, 50);
    return result;
  }

  // -------------------------------------------------------------------------
  // Dispatch
  // -------------------------------------------------------------------------

  function dispatch(action: any, args: any): any {
    if (unloading) throw new ToolError('NAVIGATION_LOST', 'the page is unloading');
    switch (action) {
      case 'navigate':
        return navigate(args);
      case 'page_snapshot':
        return snapshot(args);
      case 'page_text':
        return pageText(args);
      case 'page_html':
        return pageHtml(args);
      case 'page_find':
        return pageFind(args);
      case 'click': {
        const target = resolveTarget(args, 'click');
        return clickElement(target.element, 'click', target.selector);
      }
      case 'hover':
        return hoverElement(args);
      case 'press':
        return pressKeys(args);
      case 'scroll':
        return scrollPage(args);
      case 'select':
        return selectOptions(args);
      case 'type':
        return typeInto(args);
      default:
        throw badArgs('unknown action: ' + action);
    }
  }

  async function handleCommand(message: any) {
    const rawTool = typeof message.tool === 'string' ? message.tool.trim() : '';
    if (!rawTool) throw badArgs('command is missing tool');
    const action = TOOL_MAP[rawTool.toLowerCase()];
    if (!action) throw badArgs('unknown tool: ' + rawTool, { tool: rawTool });
    const args = message.args && typeof message.args === 'object' ? message.args : {};
    const timeoutMs = clampTimeout(message.meta && message.meta.timeout_ms);
    return withDeadline(
      Promise.resolve().then(() => dispatch(action, args) as any),
      timeoutMs,
      action,
    );
  }

  // -------------------------------------------------------------------------
  // Message plumbing
  // -------------------------------------------------------------------------

  chrome.runtime.onMessage.addListener((message: any, sender: any, sendResponse: any) => {
    if (!message || typeof message !== 'object') return false;

    if (message.type === 'corvus.ping') {
      sendResponse({ ok: true, pong: true, url: location.href, version: CONTENT_VERSION });
      return false;
    }

    if (message.type === 'corvus.probe') {
      try {
        sendResponse(waitProbe(message));
      } catch (error: any) {
        sendResponse({ ok: false, error: toPayload(error) });
      }
      return false;
    }

    if (message.type === 'corvus.upload_target') {
      try {
        sendResponse({ ok: true, result: uploadTarget(message) });
      } catch (error: any) {
        sendResponse({ ok: false, error: toPayload(error) });
      }
      return false;
    }

    if (message.type !== 'corvus.command') return false;

    let answered = false;
    const finish = (reply: any) => {
      if (answered) return;
      answered = true;
      try {
        sendResponse(reply);
      } catch (error: any) {
        // The channel is gone; nothing else can be delivered.
      }
    };

    handleCommand(message).then(
      (result: any) => finish({ ok: true, result: result === undefined ? null : result }),
      (error: any) => finish({ ok: false, error: toPayload(error) }),
    );

    return true;
  });
})();
