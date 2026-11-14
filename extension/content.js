'use strict';

/**
 * Corvus content script.
 *
 * Runs in the top frame of every http, https and file page. Receives command
 * messages from the service worker, executes one of the four browser tools and
 * always answers exactly once with {ok:true, result} or {ok:false, error}.
 *
 * Uses no dynamic code, no inline scripts and no external resources.
 */

(() => {
  if (window.__corvusContentLoaded === true) return;
  window.__corvusContentLoaded = true;

  const CONTENT_VERSION = '0.1.0';
  const MAX_NODES_DEFAULT = 700;
  const MAX_NODES_LIMIT = 3000;
  const MIN_NODES_LIMIT = 40;
  const MAX_DEPTH = 80;
  const MAX_NAME = 200;
  const MAX_TEXT = 240;
  const MAX_SELECTOR = 512;
  const MAX_INPUT = 10000;
  const MAX_REFS = 5000;
  const MAX_TIMEOUT_MS = 120000;
  const DEFAULT_TIMEOUT_MS = 10000;
  const REF_PATTERN = /^@?e(\d+)$/i;
  const NAVIGABLE_PATTERN = /^(https?|file):/i;

  const RETRYABLE = {
    BAD_ARGS: false,
    NOT_FOUND: true,
    NOT_INTERACTABLE: true,
    TIMEOUT: true,
    NAVIGATION_LOST: true,
    CAPABILITY_UNAVAILABLE: false,
    BROWSER_GONE: false,
    TRANSPORT_DOWN: true,
    INTERNAL: false,
  };

  const TOOL_MAP = {
    browser_navigate: 'navigate',
    page_snapshot: 'page_snapshot',
    browser_click: 'click',
    browser_type: 'type',
  };

  const SKIP_TAGS = {
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

  const LANDMARKS = {
    NAV: true,
    MAIN: true,
    HEADER: true,
    FOOTER: true,
    ASIDE: true,
    FORM: true,
  };

  const TEXT_INPUT_TYPES = {
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

  const refToElement = new Map();
  const elementToRef = new WeakMap();
  let refCounter = 0;
  let unloading = false;

  window.addEventListener('pagehide', () => {
    unloading = true;
  });

  // -------------------------------------------------------------------------
  // Errors
  // -------------------------------------------------------------------------

  class ToolError extends Error {
    constructor(code, message, details) {
      super(message);
      this.name = 'ToolError';
      this.code = Object.prototype.hasOwnProperty.call(RETRYABLE, code) ? code : 'INTERNAL';
      this.details = details === undefined ? null : details;
    }

    toPayload() {
      const payload = {
        code: this.code,
        message: this.message,
        retryable: RETRYABLE[this.code] === true,
      };
      if (this.details !== null) payload.details = this.details;
      return payload;
    }
  }

  function toPayload(error) {
    if (error instanceof ToolError) return error.toPayload();
    const message = error && error.message ? String(error.message) : String(error);
    return { code: 'INTERNAL', message: 'content script failure: ' + message, retryable: false };
  }

  function badArgs(message, details) {
    return new ToolError('BAD_ARGS', message, details);
  }

  function notFound(message, details) {
    return new ToolError('NOT_FOUND', message, details);
  }

  function notInteractable(message, details) {
    return new ToolError('NOT_INTERACTABLE', message, details);
  }

  // -------------------------------------------------------------------------
  // Small DOM helpers
  // -------------------------------------------------------------------------

  function collapse(value) {
    return String(value === undefined || value === null ? '' : value).replace(/\s+/g, ' ').trim();
  }

  function truncate(value, limit) {
    const text = String(value === undefined || value === null ? '' : value);
    if (text.length <= limit) return text;
    return text.slice(0, limit - 1) + '\u2026';
  }

  function clampNodes(value) {
    const numeric = Number(value);
    if (!Number.isFinite(numeric)) return MAX_NODES_DEFAULT;
    return Math.min(MAX_NODES_LIMIT, Math.max(MIN_NODES_LIMIT, Math.floor(numeric)));
  }

  function clampTimeout(value) {
    const numeric = Number(value);
    if (!Number.isFinite(numeric) || numeric <= 0) return DEFAULT_TIMEOUT_MS;
    return Math.min(MAX_TIMEOUT_MS, Math.max(250, Math.round(numeric)));
  }

  function nextFrame() {
    return new Promise((resolve) => {
      if (typeof window.requestAnimationFrame === 'function') {
        window.requestAnimationFrame(() => resolve());
      } else {
        window.setTimeout(resolve, 0);
      }
    });
  }

  function withDeadline(promise, timeoutMs, label) {
    let timer = null;
    const deadline = new Promise((resolve, reject) => {
      timer = window.setTimeout(() => {
        reject(new ToolError('TIMEOUT', label + ' did not finish within ' + timeoutMs + ' ms'));
      }, timeoutMs);
    });
    return Promise.race([promise, deadline]).finally(() => {
      if (timer !== null) window.clearTimeout(timer);
    });
  }

  function isRendered(element) {
    if (!element || element.nodeType !== 1 || !element.isConnected) return false;
    try {
      if (typeof element.checkVisibility === 'function') {
        return element.checkVisibility({ checkOpacity: true, checkVisibilityCSS: true });
      }
    } catch (error) {
      // Fall through to the style based check.
    }
    const style = window.getComputedStyle(element);
    if (!style) return false;
    if (style.display === 'none' || style.visibility === 'hidden' || style.visibility === 'collapse') return false;
    if (style.opacity === '0') return false;
    return true;
  }

  function scrollIntoView(element) {
    try {
      element.scrollIntoView({ block: 'center', inline: 'center', behavior: 'auto' });
    } catch (error) {
      try {
        element.scrollIntoView();
      } catch (inner) {
        // Ignore: the hit test below reports the real problem.
      }
    }
  }

  function focusElement(element) {
    try {
      if (typeof element.focus === 'function') {
        element.focus({ preventScroll: true });
      }
    } catch (error) {
      try {
        element.focus();
      } catch (inner) {
        // Ignore.
      }
    }
  }

  function describeElement(element) {
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

  function refFor(element) {
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

  function resolveRef(raw) {
    const match = REF_PATTERN.exec(String(raw || '').trim());
    if (!match) return null;
    const element = refToElement.get(Number(match[1]));
    if (!element || !element.isConnected) return null;
    return element;
  }

  // -------------------------------------------------------------------------
  // Accessibility and snapshot helpers
  // -------------------------------------------------------------------------

  function roleOf(element) {
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

  function labelsText(element) {
    const labels = element.labels;
    if (!labels || !labels.length) return '';
    const parts = [];
    for (let i = 0; i < labels.length; i += 1) {
      const text = collapse(labels[i].textContent);
      if (text) parts.push(text);
    }
    return parts.join(' ');
  }

  function accessibleName(element) {
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

  function statesOf(element) {
    const states = {};
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

  function isInteresting(element) {
    const tag = element.tagName;
    if (tag === 'A') return element.hasAttribute('href') || element.hasAttribute('role');
    if (tag === 'BUTTON' || tag === 'INPUT' || tag === 'SELECT' || tag === 'TEXTAREA' || tag === 'SUMMARY' || tag === 'IFRAME' || tag === 'DIALOG') return true;
    if (/^H[1-6]$/.test(tag)) return true;
    if (LANDMARKS[tag] === true) return true;
    if (tag === 'IMG') return element.hasAttribute('alt') || element.hasAttribute('aria-label');
    if (tag === 'svg') return element.hasAttribute('role') || element.hasAttribute('aria-label') || !!element.querySelector('title');
    if (element.hasAttribute('role')) return true;
    if (element.hasAttribute('tabindex')) return true;
    if (element.hasAttribute('contenteditable')) return true;
    if (element.isContentEditable === true) return true;
    return false;
  }

  const INTERACTIVE_ROLES = {
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

  function isActionable(element) {
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

  function buildNode(element, depth) {
    const rect = element.getBoundingClientRect();
    const tag = element.tagName.toLowerCase();
    const node = {
      kind: 'element',
      ref: refFor(element),
      tag: tag,
      role: roleOf(element) || null,
      name: truncate(accessibleName(element), MAX_NAME),
      bbox: {
        x: Math.round(rect.left),
        y: Math.round(rect.top),
        w: Math.round(rect.width),
        h: Math.round(rect.height),
      },
      depth: depth,
      states: statesOf(element),
    };

    const attrs = {};
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

  function snapshot(args) {
    const cap = clampNodes(args.max_nodes);
    let mode = 'interactive';
    if (args.mode !== undefined && args.mode !== null) {
      const requested = String(args.mode).trim().toLowerCase();
      if (requested !== 'interactive' && requested !== 'full') {
        throw badArgs('args.mode must be "interactive" or "full"', { mode: truncate(String(args.mode), 40) });
      }
      mode = requested;
    }
    const interesting = mode === 'interactive' ? isActionable : isInteresting;
    const nodes = [];
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

    function emitText(node, depth) {
      if (!budgetLeft()) return;
      const text = collapse(node.nodeValue);
      if (!text) return;
      nodes.push({ kind: 'text', depth: depth, text: truncate(text, MAX_TEXT) });
      textCount += 1;
    }

    function walk(element, depth) {
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
        nodes.push(buildNode(element, depth));
        elementCount += 1;
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

    return {
      url: location.href,
      title: truncate(document.title, 300),
      viewport: {
        width: window.innerWidth,
        height: window.innerHeight,
        scrollX: Math.round(window.scrollX),
        scrollY: Math.round(window.scrollY),
      },
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
  // Target resolution
  // -------------------------------------------------------------------------

  function resolveSelector(selector) {
    if (selector.length > MAX_SELECTOR) {
      throw badArgs('selector is longer than ' + MAX_SELECTOR + ' characters', { selector: truncate(selector, 80) });
    }
    let element = null;
    try {
      element = document.querySelector(selector);
    } catch (error) {
      throw badArgs('invalid CSS selector: ' + String(error && error.message ? error.message : error), { selector: truncate(selector, 80) });
    }
    if (!element) throw notFound('no element matches the selector', { selector: truncate(selector, 120) });
    return element;
  }

  function resolveTarget(args, action) {
    // The frozen public argument is "target" ("@eN" or a CSS selector).
    // "ref" and "selector" remain accepted aliases; target takes precedence.
    const rawRef = typeof args.ref === 'string' ? args.ref.trim() : '';
    const rawTarget = typeof args.target === 'string' ? args.target.trim() : '';
    const rawSelector = typeof args.selector === 'string' ? args.selector.trim() : '';

    function fromRef(value, label) {
      const element = resolveRef(value);
      if (!element) {
        const details = { action: action };
        details[label] = value;
        throw notFound('the element reference is unknown or stale; take a new snapshot', details);
      }
      return { element: element, ref: elementToRef.get(element) || value, selector: null };
    }

    function fromSelector(value) {
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

  function assertInteractable(element, action) {
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
        bbox: { x: Math.round(rect.left), y: Math.round(rect.top), w: Math.round(rect.width), h: Math.round(rect.height) },
      });
    }
    return rect;
  }

  function hitTest(element, rect, action) {
    const viewportWidth = Math.max(window.innerWidth, 1);
    const viewportHeight = Math.max(window.innerHeight, 1);
    const x = Math.min(Math.max(rect.left + rect.width / 2, 0), viewportWidth - 1);
    const y = Math.min(Math.max(rect.top + rect.height / 2, 0), viewportHeight - 1);
    const hit = document.elementFromPoint(x, y);
    if (!hit) {
      throw notInteractable('the element is outside the viewport', { action: action, element: describeElement(element) });
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

  function dispatchPointer(element, type, point) {
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
    } catch (error) {
      return false;
    }
  }

  function dispatchMouse(element, type, point) {
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
    } catch (error) {
      return false;
    }
  }

  function clickElement(element, action, selector) {
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
    } catch (error) {
      throw new ToolError('INTERNAL', 'the element click threw: ' + String(error && error.message ? error.message : error), {
        element: describeElement(element),
      });
    }

    return {
      clicked: true,
      ref: elementToRef.get(element) || null,
      selector: selector || null,
      tag: element.tagName.toLowerCase(),
      role: roleOf(element) || null,
      name: truncate(accessibleName(element), MAX_NAME),
      bbox: { x: Math.round(currentRect.left), y: Math.round(currentRect.top), w: Math.round(currentRect.width), h: Math.round(currentRect.height) },
      point: { x: Math.round(point.x), y: Math.round(point.y) },
      events: fired,
      url: location.href,
    };
  }

  // -------------------------------------------------------------------------
  // Type
  // -------------------------------------------------------------------------

  function isTextInput(element) {
    if (element.tagName === 'TEXTAREA') return true;
    if (element.tagName === 'INPUT') {
      const type = String(element.type || 'text').toLowerCase();
      return TEXT_INPUT_TYPES[type] === true;
    }
    return false;
  }

  function setNativeValue(element, value) {
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

  function createInputEvent(type, text, inputType) {
    try {
      return new InputEvent(type, {
        bubbles: true,
        cancelable: type === 'beforeinput',
        composed: true,
        data: text,
        inputType: inputType,
      });
    } catch (error) {
      try {
        return new Event(type, { bubbles: true, cancelable: type === 'beforeinput', composed: true });
      } catch (inner) {
        return null;
      }
    }
  }

  function fireInput(element, text, inputType) {
    const fired = [];
    const before = createInputEvent('beforeinput', text, inputType);
    if (before) {
      try {
        element.dispatchEvent(before);
        fired.push('beforeinput');
      } catch (error) {
        // Ignore one failing listener set; the input event still matters.
      }
    }
    const input = createInputEvent('input', text, inputType);
    if (input) {
      try {
        element.dispatchEvent(input);
        fired.push('input');
      } catch (error) {
        // Ignore.
      }
    }
    return fired;
  }

  function fireChange(element) {
    try {
      element.dispatchEvent(new Event('change', { bubbles: true, composed: true }));
      return true;
    } catch (error) {
      return false;
    }
  }

  async function typeInto(args) {
    const target = resolveTarget(args, 'type');
    const element = target.element;

    if (typeof args.text !== 'string') throw badArgs('args.text must be a string', { action: 'type' });
    const text = args.text;
    if (text.length > MAX_INPUT) throw badArgs('args.text exceeds ' + MAX_INPUT + ' characters', { action: 'type', length: text.length });

    const clear = args.clear !== false;
    assertInteractable(element, 'type');
    scrollIntoView(element);
    focusElement(element);
    await nextFrame();

    const editable = element.isContentEditable === true || (element.getAttribute && element.getAttribute('contenteditable') === 'true');
    const textField = isTextInput(element);

    if (!editable && !textField) {
      throw notInteractable('the element is not a text input or editable region', {
        action: 'type',
        element: describeElement(element),
        tag: element.tagName.toLowerCase(),
      });
    }

    const events = [];
    let finalValue = '';

    if (editable) {
      if (clear) element.textContent = '';
      try {
        const inserted = document.execCommand('insertText', false, text);
        if (!inserted) element.textContent = (clear ? '' : element.textContent) + text;
      } catch (error) {
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
      } catch (error) {
        throw new ToolError('INTERNAL', 'the native value setter failed: ' + String(error && error.message ? error.message : error), {
          element: describeElement(element),
        });
      }
      events.push.apply(events, fireInput(element, text, clear ? 'insertText' : 'insertText'));
      if (fireChange(element)) events.push('change');
      if (args.blur === true) {
        try {
          element.blur();
          events.push('blur');
        } catch (error) {
          // Ignore.
        }
      }
      finalValue = String(element.value === undefined || element.value === null ? '' : element.value);
    }

    const result = {
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

  function navigate(args) {
    const raw = args.url !== undefined ? args.url : args.href;
    if (typeof raw !== 'string' || !raw.trim()) throw badArgs('args.url must be a non-empty string', { action: 'navigate' });
    if (unloading) throw new ToolError('NAVIGATION_LOST', 'the page is unloading before navigation could start');

    let resolved = null;
    try {
      resolved = new URL(raw.trim(), location.href);
    } catch (error) {
      throw badArgs('args.url is not a valid URL', { action: 'navigate', url: truncate(raw, 200) });
    }
    if (!NAVIGABLE_PATTERN.test(resolved.protocol)) {
      throw badArgs('only http, https and file URLs can be navigated to', { action: 'navigate', url: truncate(resolved.href, 200) });
    }

    const target = resolved.href;
    const result = {
      navigated: true,
      url: target,
      previous: location.href,
      same_document: target === location.href,
      started_at: Date.now(),
    };
    window.setTimeout(() => {
      try {
        location.assign(target);
      } catch (error) {
        // The reply is already on its way; a failed assignment is reported on
        // the next command by the service worker as NAVIGATION_LOST or TIMEOUT.
      }
    }, 50);
    return result;
  }

  // -------------------------------------------------------------------------
  // Dispatch
  // -------------------------------------------------------------------------

  function dispatch(action, args) {
    if (unloading) throw new ToolError('NAVIGATION_LOST', 'the page is unloading');
    switch (action) {
      case 'navigate':
        return navigate(args);
      case 'page_snapshot':
        return snapshot(args);
      case 'click':
        const target = resolveTarget(args, 'click');
        return clickElement(target.element, 'click', target.selector);
      case 'type':
        return typeInto(args);
      default:
        throw badArgs('unknown action: ' + action);
    }
  }

  async function handleCommand(message) {
    const rawTool = typeof message.tool === 'string' ? message.tool.trim() : '';
    if (!rawTool) throw badArgs('command is missing tool');
    const action = TOOL_MAP[rawTool.toLowerCase()];
    if (!action) throw badArgs('unknown tool: ' + rawTool, { tool: rawTool });
    const args = message.args && typeof message.args === 'object' ? message.args : {};
    const timeoutMs = clampTimeout(message.meta && message.meta.timeout_ms);
    return withDeadline(Promise.resolve().then(() => dispatch(action, args)), timeoutMs, action);
  }

  // -------------------------------------------------------------------------
  // Message plumbing
  // -------------------------------------------------------------------------

  chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
    if (!message || typeof message !== 'object') return false;

    if (message.type === 'corvus.ping') {
      sendResponse({ ok: true, pong: true, url: location.href, version: CONTENT_VERSION });
      return false;
    }

    if (message.type !== 'corvus.command') return false;

    let answered = false;
    const finish = (reply) => {
      if (answered) return;
      answered = true;
      try {
        sendResponse(reply);
      } catch (error) {
        // The channel is gone; nothing else can be delivered.
      }
    };

    handleCommand(message).then(
      (result) => finish({ ok: true, result: result === undefined ? null : result }),
      (error) => finish({ ok: false, error: toPayload(error) })
    );

    return true;
  });
})();
