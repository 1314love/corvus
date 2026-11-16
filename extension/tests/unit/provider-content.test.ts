import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

type Listener = (message: unknown, sender: unknown, sendResponse: (value: unknown) => void) => boolean | undefined;

let listener: Listener = () => false;

beforeAll(async () => {
  vi.stubGlobal('chrome', {
    runtime: {
      onMessage: {
        addListener: (fn: Listener) => {
          listener = fn;
        },
      },
    },
  });
  await import('../../src/content/provider-runtime');
});

beforeEach(() => {
  document.body.innerHTML = '';
  document.documentElement.removeAttribute('style');
  document.body.removeAttribute('style');
});

function rect(element: Element, x: number, y: number, width: number, height: number): void {
  element.getBoundingClientRect = () =>
    ({
      x,
      y,
      top: y,
      left: x,
      right: x + width,
      bottom: y + height,
      width,
      height,
      toJSON: () => ({}),
    }) as DOMRect;
}

function call(action: string, args: Record<string, unknown>): Promise<any> {
  return new Promise((resolve) => {
    listener({ type: 'corvus.provider_action', action, args }, {}, resolve);
  });
}

async function snapshot(args: Record<string, unknown> = {}): Promise<any> {
  const reply = await call('page.snapshot', args);
  expect(reply.ok).toBe(true);
  return reply.value;
}

describe('provider content page.read', () => {
  it('reads text and html from the body and masks password values', async () => {
    document.body.innerHTML =
      '<p id="copy">Hello page</p><input id="password" type="password" value="topsecret"><input id="name" name="name" value="Ada">';
    rect(document.documentElement, 0, 0, 800, 600);
    rect(document.body, 0, 0, 800, 600);
    const text = await call('page.read', {});
    expect(text.ok).toBe(true);
    expect(text.value.content).toContain('Hello page');
    expect(text.value.mode).toBe('text');
    expect(text.value.truncated).toBe(false);

    const html = await call('page.read', { mode: 'html' });
    expect(html.ok).toBe(true);
    expect(html.value.content).toContain('value="***"');
    expect(html.value.content).not.toContain('topsecret');
  });

  it('scopes to a selector, reports truncation and returns NOT_FOUND for no match', async () => {
    document.body.innerHTML = '<div id="scope">' + 'x'.repeat(50) + '</div>';
    const reply = await call('page.read', { selector: '#scope', max_chars: 10 });
    expect(reply.ok).toBe(true);
    expect(reply.value.content).toHaveLength(10);
    expect(reply.value.truncated).toBe(true);
    const missing = await call('page.read', { selector: '#missing' });
    expect(missing.ok).toBe(false);
    expect(missing.error.code).toBe('NOT_FOUND');
  });
});

describe('provider content page.snapshot', () => {
  it('returns visible, open-shadow nodes with bbox and no password value', async () => {
    document.body.innerHTML =
      '<button id="action">Action</button><input id="password" name="password" type="password" value="secret"><div id="hidden" style="display:none"><button id="ghost">Ghost</button></div><div id="host"></div>';
    rect(document.documentElement, 0, 0, 800, 600);
    rect(document.body, 0, 0, 800, 600);
    const button = document.getElementById('action') as HTMLElement;
    const password = document.getElementById('password') as HTMLElement;
    rect(button, 10, 20, 120, 32);
    rect(password, 10, 80, 200, 24);
    const shadow = (document.getElementById('host') as HTMLElement).attachShadow({ mode: 'open' });
    shadow.innerHTML = '<button id="shadow">Shadow action</button>';
    rect(shadow.getElementById('shadow') as HTMLElement, 10, 140, 100, 30);

    const value = await snapshot();
    const buttonNode = value.nodes.find((node: any) => node.name === 'Action');
    expect(buttonNode).toMatchObject({ role: 'button', bbox: { x: 10, y: 20, width: 120, height: 32 } });
    expect(buttonNode.ref).toMatch(/^@e[1-9][0-9]*$/);
    expect(value.nodes.some((node: any) => node.name === 'Ghost')).toBe(false);
    const passwordNode = value.nodes.find((node: any) => node.role === 'textbox' && node.name === 'password');
    expect(passwordNode).toBeDefined();
    expect(passwordNode.value).toBeUndefined();
    expect(value.nodes.some((node: any) => node.name === 'Shadow action')).toBe(true);
  });

  it('honours max_nodes and can include hidden nodes on request', async () => {
    document.body.innerHTML =
      '<button id="one">One</button><button id="two">Two</button><button id="three">Three</button>';
    rect(document.documentElement, 0, 0, 800, 600);
    rect(document.body, 0, 0, 800, 600);
    for (const id of ['one', 'two', 'three']) rect(document.getElementById(id) as HTMLElement, 0, 0, 10, 10);
    const limited = await snapshot({ max_nodes: 50 });
    expect(limited.nodes.length).toBeLessThanOrEqual(50);
    const hidden = document.createElement('button');
    hidden.textContent = 'Hidden';
    hidden.style.display = 'none';
    document.body.appendChild(hidden);
    expect((await snapshot()).nodes.some((node: any) => node.name === 'Hidden')).toBe(false);
    expect((await snapshot({ include_hidden: true })).nodes.some((node: any) => node.name === 'Hidden')).toBe(true);
  });
});

describe('provider content input actions', () => {
  it('clicks a snapshot ref and types into an input with clear/submit', async () => {
    document.body.innerHTML =
      '<form id="form"><input id="field" name="field"><button id="go" type="button">Go</button></form>';
    rect(document.documentElement, 0, 0, 800, 600);
    rect(document.body, 0, 0, 800, 600);
    const field = document.getElementById('field') as HTMLInputElement;
    const go = document.getElementById('go') as HTMLButtonElement;
    rect(field, 10, 10, 100, 20);
    rect(go, 10, 40, 50, 20);
    let clicked = false;
    go.addEventListener('click', () => {
      clicked = true;
    });
    let submitted = false;
    (document.getElementById('form') as HTMLFormElement).addEventListener('submit', (event) => {
      event.preventDefault();
      submitted = true;
    });

    const value = await snapshot();
    const fieldNode = value.nodes.find((node: any) => node.role === 'textbox');
    const goNode = value.nodes.find((node: any) => node.name === 'Go');
    const clickedReply = await call('input.click', { ref: goNode.ref });
    expect(clickedReply).toEqual({ ok: true, value: { ref: goNode.ref, clicked: true } });
    expect(clicked).toBe(true);
    const typed = await call('input.type', { ref: fieldNode.ref, text: 'hello', submit: true });
    expect(typed.ok).toBe(true);
    expect(typed.value).toEqual({ ref: fieldNode.ref, typed_chars: 5, submitted: true });
    expect(field.value).toBe('hello');
    expect(submitted).toBe(true);
  });

  it('rejects wrong/stale refs and unsupported actions with the frozen codes', async () => {
    const wrong = await call('input.click', { ref: '@c1' });
    expect(wrong.error.code).toBe('BAD_ARGS');
    const missing = await call('input.type', { ref: '@e999999', text: 'x' });
    expect(missing.error.code).toBe('NOT_FOUND');
    const unsupported = await call('page.navigate', {});
    expect(unsupported.error.code).toBe('BAD_ARGS');
  });
});
