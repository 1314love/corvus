import { afterAll, beforeAll, beforeEach, describe, expect, it } from 'vitest';

let commandListener: any;

function rect(left: number, top: number, width: number, height: number): any {
  return { left, top, width, height, right: left + width, bottom: top + height, x: left, y: top };
}

function setRect(element: any, box: any): void {
  Object.defineProperty(element, 'getBoundingClientRect', {
    configurable: true,
    value: () => rect(box.left, box.top, box.width, box.height),
  });
  Object.defineProperty(element, 'getClientRects', {
    configurable: true,
    value: () => [rect(box.left, box.top, box.width, box.height)],
  });
}

function hide(element: any): void {
  element.style.display = 'none';
  Object.defineProperty(element, 'getClientRects', { configurable: true, value: () => [] });
}

function callMessage(message: Record<string, unknown>): Promise<any> {
  return new Promise((resolve, reject) => {
    commandListener(message, {}, (reply: any) => {
      if (reply && reply.ok === true) {
        resolve(reply.result);
        return;
      }
      const error = new Error(reply && reply.error ? reply.error.message : 'message failed');
      (error as any).toolError = reply ? reply.error : null;
      reject(error);
    });
  });
}

function callTool(tool: string, args: Record<string, unknown>): Promise<any> {
  return new Promise((resolve, reject) => {
    const accepted = commandListener(
      { type: 'corvus.command', tool, args, meta: { timeout_ms: 5000 } },
      {},
      (reply: any) => {
        if (reply && reply.ok === true) {
          resolve(reply.result);
          return;
        }
        const error = new Error(reply && reply.error ? reply.error.message : 'tool failed');
        (error as any).toolError = reply ? reply.error : null;
        reject(error);
      },
    );
    if (accepted !== true) reject(new Error('content listener did not accept the command'));
  });
}

async function expectContentError(promise: Promise<any>, code: string): Promise<void> {
  try {
    await promise;
    throw new Error('expected the content command to fail');
  } catch (error: any) {
    expect(error.toolError ? error.toolError.code : null).toBe(code);
  }
}

beforeAll(async () => {
  (globalThis as any).chrome = {
    runtime: {
      onMessage: {
        addListener(listener: any) {
          commandListener = listener;
        },
      },
    },
  };
  await import('../../src/content/runtime');
});

beforeEach(() => {
  document.body.innerHTML = '';
  window.localStorage.clear();
  window.sessionStorage.clear();
  setRect(document.documentElement, rect(0, 0, 1024, 768));
  setRect(document.body, rect(0, 0, 1024, 768));
});

afterAll(() => {
  document.body.innerHTML = '';
});

describe('page_links', () => {
  it('extracts visible absolute links from nested shadow roots with options and dedupe', async () => {
    const scope = document.createElement('div');
    scope.id = 'scope';
    const first = document.createElement('a');
    first.href = '/docs';
    first.textContent = '  Docs   home ';
    first.title = 'Documentation';
    first.setAttribute('rel', 'nofollow');
    first.setAttribute('target', '_blank');
    setRect(first, rect(10, 10, 100, 20));
    const duplicate = document.createElement('a');
    duplicate.href = '/docs';
    duplicate.textContent = 'Docs home';
    setRect(duplicate, rect(10, 40, 100, 20));
    const external = document.createElement('a');
    external.href = 'mailto:help@example.com';
    external.textContent = 'Mail';
    setRect(external, rect(10, 70, 100, 20));
    const zeroArea = document.createElement('a');
    zeroArea.href = '/zero-area';
    zeroArea.textContent = 'Zero area';
    zeroArea.style.display = 'inline-block';
    setRect(zeroArea, rect(10, 100, 0, 0));
    const hidden = document.createElement('a');
    hidden.href = '/hidden';
    hidden.textContent = 'Hidden';
    hide(hidden);
    const skipped = document.createElement('a');
    skipped.setAttribute('href', 'javascript:void(0)');
    skipped.textContent = 'Script';
    const empty = document.createElement('a');
    empty.href = '#';
    empty.textContent = 'Hash';
    scope.append(first, duplicate, external, zeroArea, hidden, skipped, empty);
    const host = document.createElement('div');
    scope.appendChild(host);
    const shadow = host.attachShadow({ mode: 'open' });
    const shadowLink = document.createElement('a');
    shadowLink.href = 'https://example.org/shadow';
    shadowLink.textContent = 'Shadow link';
    setRect(shadowLink, rect(20, 100, 100, 20));
    shadow.appendChild(shadowLink);
    document.body.appendChild(scope);
    setRect(scope, rect(0, 0, 500, 500));

    const result = await callTool('page_links', { selector: '#scope', max_results: 10 });
    expect(result.count).toBe(3);
    expect(result.truncated).toBe(false);
    expect(result.links).toEqual([
      {
        text: 'Docs home',
        url: new URL('/docs', location.href).href,
        title: 'Documentation',
        rel: 'nofollow',
        target: '_blank',
      },
      { text: 'Mail', url: 'mailto:help@example.com' },
      { text: 'Shadow link', url: 'https://example.org/shadow' },
    ]);

    const withHidden = await callTool('page_links', { include_hidden: true });
    expect(withHidden.count).toBe(5);
    expect(withHidden.links.map((link: any) => link.text)).toContain('Hidden');
    expect(withHidden.links.map((link: any) => link.text)).toContain('Zero area');

    const capped = await callTool('page_links', { max_results: 1 });
    expect(capped.count).toBe(1);
    expect(capped.truncated).toBe(true);
    await expectContentError(callTool('page_links', { selector: '#missing' }), 'NOT_FOUND');
    await expectContentError(callTool('page_links', { max_results: 0 }), 'BAD_ARGS');
  });
});

describe('page_forms', () => {
  it('describes fields in DOM order without values by default and keeps password values out', async () => {
    const form = document.createElement('form');
    form.id = 'login';
    form.name = 'login-form';
    form.setAttribute('action', '/submit');
    form.method = 'post';
    const user = document.createElement('input');
    user.name = 'user';
    user.id = 'user';
    user.value = 'alice';
    user.setAttribute('aria-label', 'User name');
    const password = document.createElement('input');
    password.type = 'password';
    password.name = 'password';
    password.value = 'hunter2';
    password.required = true;
    const remember = document.createElement('input');
    remember.type = 'checkbox';
    remember.name = 'remember';
    remember.checked = true;
    const select = document.createElement('select');
    select.name = 'plan';
    select.innerHTML = '<option value="free">Free</option><option value="pro" selected>Pro</option>';
    const textarea = document.createElement('textarea');
    textarea.name = 'note';
    textarea.value = 'hello';
    const label = document.createElement('label');
    label.setAttribute('for', 'user');
    label.textContent = 'Account';
    form.append(label, user, password, remember, select, textarea);
    document.body.appendChild(form);

    const defaults = await callTool('page_forms', {});
    expect(defaults.count).toBe(1);
    expect(defaults.truncated).toBe(false);
    expect(defaults.forms[0].index).toBe(0);
    expect(defaults.forms[0].name).toBe('login-form');
    expect(defaults.forms[0].id).toBe('login');
    expect(defaults.forms[0].method).toBe('post');
    expect(defaults.forms[0].action).toBe(new URL('/submit', location.href).href);
    const names = defaults.forms[0].fields.map((field: any) => field.name);
    expect(names).toEqual(['user', 'password', 'remember', 'plan', 'note']);
    const userField = defaults.forms[0].fields[0];
    expect(userField.label).toBe('User name');
    expect(userField.value).toBeUndefined();
    expect(userField.selector).toBe('#user');
    expect(defaults.forms[0].fields[1]).toMatchObject({ name: 'password', type: 'password', required: true });
    expect(defaults.forms[0].fields[2].checked).toBe(true);
    expect(defaults.forms[0].fields[3].options).toEqual([
      { value: 'free', label: 'Free', selected: false },
      { value: 'pro', label: 'Pro', selected: true },
    ]);

    const values = await callTool('page_forms', { include_values: true });
    expect(values.forms[0].fields[0].value).toBe('alice');
    expect(values.forms[0].fields[1].value).toBeUndefined();
    expect(values.forms[0].fields[4].value).toBe('hello');
    await expectContentError(callTool('page_forms', { selector: '#missing' }), 'NOT_FOUND');
    await expectContentError(callTool('page_forms', { max_forms: 101 }), 'BAD_ARGS');
  });
});

describe('browser_focus and browser_highlight', () => {
  it('focuses a snapshot ref and paints a removable fixed overlay', async () => {
    const button = document.createElement('button');
    button.textContent = 'Focus target';
    setRect(button, rect(30, 40, 120, 30));
    document.body.appendChild(button);

    const snapshot = await callTool('page_snapshot', { format: 'json' });
    const node = snapshot.nodes.find((candidate: any) => candidate.name === 'Focus target');
    expect(node).toBeTruthy();

    const focused = await callTool('browser_focus', { ref: node.ref });
    expect(focused).toEqual({ focused: true, ref: node.ref, url: location.href });
    expect(document.activeElement).toBe(button);

    const highlighted = await callTool('browser_highlight', { selector: 'button', duration_ms: 30, color: '#ff0000' });
    expect(highlighted).toMatchObject({ highlighted: true, selector: 'button', duration_ms: 30, url: location.href });
    let overlay = document.querySelector('[data-corvus-highlight]') as any;
    expect(overlay).toBeTruthy();
    expect(overlay.style.position).toBe('fixed');
    expect(overlay.style.left).toBe('30px');
    expect(overlay.style.top).toBe('40px');
    expect(overlay.style.width).toBe('120px');
    expect(overlay.style.height).toBe('30px');
    expect(overlay.style.pointerEvents).toBe('none');
    expect(overlay.style.border).toContain('#ff0000');
    await new Promise((resolve) => setTimeout(resolve, 60));
    overlay = document.querySelector('[data-corvus-highlight]');
    expect(overlay).toBeNull();

    await expectContentError(callTool('browser_highlight', {}), 'BAD_ARGS');
    await expectContentError(callTool('browser_highlight', { ref: '@e1', selector: 'button' }), 'BAD_ARGS');
    await expectContentError(callTool('browser_highlight', { selector: '#missing' }), 'NOT_FOUND');
    await expectContentError(callTool('browser_focus', { ref: '@c1' }), 'BAD_ARGS');
    await expectContentError(callTool('browser_focus', { ref: '@e999999' }), 'NOT_FOUND');
  });
});

describe('storage tools and ax root', () => {
  it('gets, sets and clears local and session storage while reporting the origin', async () => {
    const set = await callTool('storage_set', { entries: { alpha: 'one', beta: 'two' }, area: 'local' });
    expect(set).toEqual({ area: 'local', origin: location.origin, set: 2, keys: ['alpha', 'beta'] });
    const get = await callTool('storage_get', { area: 'local', keys: ['beta', 'missing'] });
    expect(get.count).toBe(1);
    expect(get.truncated).toBe(false);
    expect(get.entries).toEqual([{ key: 'beta', value: 'two' }]);
    const all = await callTool('storage_get', {});
    expect(all.count).toBe(2);
    const cleared = await callTool('storage_clear', { keys: ['alpha'], area: 'local' });
    expect(cleared).toEqual({ area: 'local', origin: location.origin, cleared: 1 });
    expect(window.localStorage.getItem('alpha')).toBeNull();
    expect(window.localStorage.getItem('beta')).toBe('two');

    await callTool('storage_set', { area: 'session', entries: { temp: 'value' } });
    const session = await callTool('storage_get', { area: 'session' });
    expect(session.entries).toEqual([{ key: 'temp', value: 'value' }]);
    expect(await callTool('storage_clear', { area: 'session' })).toEqual({
      area: 'session',
      origin: location.origin,
      cleared: 1,
    });
    await expectContentError(callTool('storage_set', { entries: {} }), 'BAD_ARGS');
  });

  it('resolves accessibility root segments and ref points', async () => {
    const host = document.createElement('div');
    host.id = 'ax-host';
    document.body.appendChild(host);
    const shadow = host.attachShadow({ mode: 'open' });
    const inner = document.createElement('button');
    inner.textContent = 'Shadow ax';
    setRect(inner, rect(12, 18, 80, 20));
    shadow.appendChild(inner);

    const root = await callMessage({ type: 'corvus.ax_root', selector: '#ax-host' });
    expect(root).toEqual({ segments: ['#ax-host'] });
    const shadowRoot = await callMessage({ type: 'corvus.ax_root', selector: '#ax-host >>> button' });
    expect(shadowRoot.segments).toEqual(['#ax-host', 'button']);
    await expectContentError(callMessage({ type: 'corvus.ax_root', selector: '#missing' }), 'NOT_FOUND');

    const snapshot = await callTool('page_snapshot', { format: 'json' });
    const node = snapshot.nodes.find((candidate: any) => candidate.name === 'Shadow ax');
    const point = await callMessage({ type: 'corvus.ref_point', ref: node.ref });
    expect(point).toMatchObject({
      ref: node.ref,
      x: 52,
      y: 28,
      pageX: 52 + window.scrollX,
      pageY: 28 + window.scrollY,
    });
    await expectContentError(callMessage({ type: 'corvus.ref_point', ref: '@c4' }), 'BAD_ARGS');
    await expectContentError(callMessage({ type: 'corvus.ref_point', ref: '@e999999' }), 'NOT_FOUND');
  });
});
