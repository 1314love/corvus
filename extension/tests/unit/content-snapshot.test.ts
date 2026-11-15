import { afterAll, beforeAll, beforeEach, describe, expect, it } from 'vitest';

type Rect = { left: number; top: number; width: number; height: number };

let commandListener: any;

function rect(left: number, top: number, width: number, height: number): Rect {
  return { left, top, width, height };
}

function setRect(element: any, box: Rect): void {
  Object.defineProperty(element, 'getBoundingClientRect', {
    configurable: true,
    value: () => ({
      left: box.left,
      top: box.top,
      width: box.width,
      height: box.height,
      right: box.left + box.width,
      bottom: box.top + box.height,
      x: box.left,
      y: box.top,
    }),
  });
}

function setFrameGeometry(frame: any, box: Rect, clientLeft: number, clientTop: number): void {
  setRect(frame, box);
  Object.defineProperty(frame, 'clientLeft', { configurable: true, value: clientLeft });
  Object.defineProperty(frame, 'clientTop', { configurable: true, value: clientTop });
}

function makeFrameDocument(html: string): Document {
  const frameDocument = document.implementation.createHTMLDocument('frame');
  frameDocument.body.innerHTML = html;
  Object.defineProperty(frameDocument, 'defaultView', { configurable: true, value: window });
  return frameDocument;
}

function attachFrame(frameDocument: Document, box: Rect, clientLeft = 0, clientTop = 0): any {
  const frame = document.createElement('iframe');
  document.body.appendChild(frame);
  Object.defineProperty(frame, 'contentDocument', { configurable: true, get: () => frameDocument });
  setFrameGeometry(frame, box, clientLeft, clientTop);
  return frame;
}

function elementIn(documentRef: Document, id: string): any {
  const element = documentRef.getElementById(id);
  if (!element) throw new Error('missing fixture element ' + id);
  return element;
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
  document.title = 'Snapshot fixture';
});

afterAll(() => {
  document.body.innerHTML = '';
});

describe('page_snapshot frame and shadow traversal', () => {
  it('assigns frame paths and top-viewport bboxes through nested frames', async () => {
    const deepDocument = makeFrameDocument('<button id="deep">Deep action</button>');
    const outerDocument = makeFrameDocument('<button id="outer">Outer action</button>');

    const innerFrame = outerDocument.createElement('iframe');
    outerDocument.body.appendChild(innerFrame);
    Object.defineProperty(innerFrame, 'contentDocument', { configurable: true, get: () => deepDocument });
    setFrameGeometry(innerFrame, rect(20, 30, 100, 80), 1, 2);

    setRect(elementIn(outerDocument, 'outer'), rect(10, 20, 80, 30));
    setRect(elementIn(deepDocument, 'deep'), rect(3, 4, 40, 20));

    attachFrame(outerDocument, rect(100, 50, 400, 300), 7, 11);
    const secondDocument = makeFrameDocument('<button id="second">Second action</button>');
    setRect(elementIn(secondDocument, 'second'), rect(5, 6, 50, 20));
    attachFrame(secondDocument, rect(600, 70, 200, 100), 2, 3);

    const result = await callTool('page_snapshot', { format: 'json', include_bbox: true });
    const elements = result.nodes.filter((node: any) => node.kind === 'element');
    expect(elements).toHaveLength(3);
    expect(elements.map((node: any) => [node.name, node.frame, node.bbox])).toEqual([
      ['Outer action', 'f0', { x: 117, y: 81, w: 80, h: 30 }],
      ['Deep action', 'f0.0', { x: 131, y: 97, w: 40, h: 20 }],
      ['Second action', 'f1', { x: 607, y: 79, w: 50, h: 20 }],
    ]);
    expect(result.frames).toMatchObject({ total: 3, included: 3, skipped: [] });
  });

  it('traverses open shadow roots and lets browser_click resolve the ref', async () => {
    const host = document.createElement('div');
    document.body.appendChild(host);
    const shadowRoot = host.attachShadow({ mode: 'open' });
    shadowRoot.innerHTML = '<button id="shadow-button">Shadow action</button>';
    const button = shadowRoot.querySelector('#shadow-button') as HTMLButtonElement;
    setRect(host, rect(50, 40, 300, 100));
    setRect(button, rect(20, 10, 120, 30));
    (shadowRoot as any).elementFromPoint = () => button;
    (document as any).elementFromPoint = () => button;

    let clicks = 0;
    button.addEventListener('click', () => {
      clicks += 1;
    });

    const snapshot = await callTool('page_snapshot', { format: 'json' });
    const node = snapshot.nodes.find((candidate: any) => candidate.kind === 'element');
    expect(node).toBeTruthy();
    expect(node.name).toBe('Shadow action');
    expect(node.frame).toBeUndefined();

    const click = await callTool('browser_click', { ref: node.ref });
    expect(click.clicked).toBe(true);
    expect(clicks).toBe(1);
  });

  it('reports cross-origin frames and filters by the top viewport', async () => {
    const originalWidth = window.innerWidth;
    const originalHeight = window.innerHeight;
    const originalVisualViewport = window.visualViewport;
    Object.defineProperty(window, 'innerWidth', { configurable: true, value: 800 });
    Object.defineProperty(window, 'innerHeight', { configurable: true, value: 600 });
    Object.defineProperty(window, 'visualViewport', {
      configurable: true,
      value: { width: 800, height: 600, offsetLeft: 0, offsetTop: 0 },
    });
    try {
      const crossOrigin = document.createElement('iframe');
      document.body.appendChild(crossOrigin);
      Object.defineProperty(crossOrigin, 'src', { configurable: true, value: 'https://cross.example/frame' });
      setRect(crossOrigin, rect(0, 0, 300, 200));
      Object.defineProperty(crossOrigin, 'contentDocument', { configurable: true, get: () => null });
      Object.defineProperty(crossOrigin, 'contentWindow', { configurable: true, get: () => ({}) });

      const frameDocument = makeFrameDocument(
        '<button id="visible">Visible action</button><button id="offscreen">Offscreen action</button>',
      );
      setRect(elementIn(frameDocument, 'visible'), rect(10, 10, 100, 30));
      setRect(elementIn(frameDocument, 'offscreen'), rect(2000, 2000, 100, 30));
      attachFrame(frameDocument, rect(100, 100, 500, 400), 0, 0);

      const result = await callTool('page_snapshot', { format: 'json', viewport_only: true });
      const names = result.nodes.filter((node: any) => node.kind === 'element').map((node: any) => node.name);
      expect(names).toEqual(['Visible action']);
      expect(result.frames).toMatchObject({ total: 2, included: 1 });
      expect(result.frames.skipped).toEqual([
        { frame: 'f0', url: 'https://cross.example/frame', reason: 'cross_origin' },
      ]);
    } finally {
      Object.defineProperty(window, 'innerWidth', { configurable: true, value: originalWidth });
      Object.defineProperty(window, 'innerHeight', { configurable: true, value: originalHeight });
      Object.defineProperty(window, 'visualViewport', { configurable: true, value: originalVisualViewport });
    }
  });

  it('applies max_nodes globally across frames', async () => {
    const firstDocument = makeFrameDocument(
      '<button id="a">First action</button><button id="b">Second action</button>',
    );
    setRect(elementIn(firstDocument, 'a'), rect(0, 0, 40, 20));
    setRect(elementIn(firstDocument, 'b'), rect(0, 30, 40, 20));
    attachFrame(firstDocument, rect(0, 0, 200, 200));

    const secondDocument = makeFrameDocument('<button id="c">Third action</button>');
    setRect(elementIn(secondDocument, 'c'), rect(0, 0, 40, 20));
    attachFrame(secondDocument, rect(200, 0, 200, 200));

    const result = await callTool('page_snapshot', { format: 'json', max_nodes: 3 });
    const elements = result.nodes.filter((node: any) => node.kind === 'element');
    expect(elements.map((node: any) => node.name)).toEqual(['First action', 'Second action', 'Third action']);
    expect(result.node_count).toBe(3);
    expect(result.truncated).toBe(false);
  });
});
