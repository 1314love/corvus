import { describe, expect, it, vi } from 'vitest';
import { captureScreenshot, mimeTypeFor, parseScreenshotArgs } from '../../src/background/screenshot';
import type { ScreenshotDeps } from '../../src/background/screenshot';
import { ToolError } from '../../src/shared/errors';

function makeDeps(overrides: Partial<ScreenshotDeps> = {}) {
  const sendCdp = vi.fn(async (..._args: any[]) => ({ data: 'AAAA' }));
  const captureVisibleTab = vi.fn(async (..._args: any[]) => 'data:image/png;base64,iVBORw0KGgo=');
  const resolveElementRect = vi.fn(async (..._args: any[]) => ({
    x: 10,
    y: 20,
    width: 30,
    height: 40,
    url: 'http://page.test/frames.html',
  }));
  const deps: ScreenshotDeps = {
    sendCdp: sendCdp as any,
    captureVisibleTab: captureVisibleTab as any,
    resolveElementRect: resolveElementRect as any,
    ...overrides,
  };
  return { deps, sendCdp, captureVisibleTab, resolveElementRect };
}

function errorCode(operation: () => void): string | null {
  try {
    operation();
    return null;
  } catch (error: any) {
    return error instanceof ToolError ? error.code : null;
  }
}

const tab = { id: 7, url: 'http://page.test/frames.html', windowId: 3, active: true };

describe('browser_screenshot argument parsing', () => {
  it('applies the documented defaults', () => {
    expect(parseScreenshotArgs({})).toEqual({
      target: 'viewport',
      ref: null,
      format: 'webp',
      quality: 80,
      inline: false,
    });
  });

  it('normalizes explicit values and keeps selection arguments out of the capture request', () => {
    expect(
      parseScreenshotArgs({
        target: 'ELEMENT',
        ref: ' @e12 ',
        format: 'PNG',
        quality: 1,
        inline: true,
        tab_id: '7',
        browser_id: 'browser-1',
      }),
    ).toEqual({ target: 'element', ref: '@e12', format: 'png', quality: 1, inline: true });
  });

  it('rejects invalid target, format, quality, ref and unknown arguments', () => {
    expect(errorCode(() => parseScreenshotArgs({ target: 'window' }))).toBe('BAD_ARGS');
    expect(errorCode(() => parseScreenshotArgs({ format: 'gif' }))).toBe('BAD_ARGS');
    expect(errorCode(() => parseScreenshotArgs({ quality: 0 }))).toBe('BAD_ARGS');
    expect(errorCode(() => parseScreenshotArgs({ quality: 101 }))).toBe('BAD_ARGS');
    expect(errorCode(() => parseScreenshotArgs({ quality: 1.5 }))).toBe('BAD_ARGS');
    expect(errorCode(() => parseScreenshotArgs({ target: 'element' }))).toBe('BAD_ARGS');
    expect(errorCode(() => parseScreenshotArgs({ target: 'element', ref: '@c1' }))).toBe('BAD_ARGS');
    expect(errorCode(() => parseScreenshotArgs({ target: 'element', ref: 'e1' }))).toBe('BAD_ARGS');
    expect(errorCode(() => parseScreenshotArgs({ ref: '@e1' }))).toBe('BAD_ARGS');
    expect(errorCode(() => parseScreenshotArgs({ extra: true }))).toBe('BAD_ARGS');
    expect(errorCode(() => parseScreenshotArgs(null))).toBe('BAD_ARGS');
  });
});

describe('browser_screenshot capture requests', () => {
  it('captures the viewport through CDP with quality and no clip', async () => {
    const { deps, sendCdp } = makeDeps();
    const result = await captureScreenshot({}, tab, deps);

    expect(sendCdp).toHaveBeenCalledTimes(1);
    expect(sendCdp.mock.calls[0][0]).toBe(7);
    expect(sendCdp.mock.calls[0][1]).toBe('Page.captureScreenshot');
    expect(sendCdp.mock.calls[0][2]).toEqual({
      format: 'webp',
      quality: 80,
      fromSurface: true,
      captureBeyondViewport: false,
    });
    expect(result).toEqual({
      format: 'webp',
      mime_type: 'image/webp',
      target: 'viewport',
      tab_id: '7',
      url: 'http://page.test/frames.html',
      data: 'AAAA',
    });
    expect(result).not.toHaveProperty('bbox');
    expect(result).not.toHaveProperty('inline');
  });

  it('omits quality for PNG and passes JPEG quality through', async () => {
    const png = makeDeps();
    await captureScreenshot({ format: 'png', quality: 42 }, tab, png.deps);
    expect(png.sendCdp.mock.calls[0][2]).toMatchObject({ format: 'png', fromSurface: true });
    expect(png.sendCdp.mock.calls[0][2]).not.toHaveProperty('quality');

    const jpeg = makeDeps();
    const result = await captureScreenshot({ format: 'jpeg', quality: 42 }, tab, jpeg.deps);
    expect(jpeg.sendCdp.mock.calls[0][2]).toMatchObject({ format: 'jpeg', quality: 42 });
    expect(result.mime_type).toBe('image/jpeg');
  });

  it('uses the full content size for full-page capture', async () => {
    const sendCdp = vi.fn(async (_tabId: number, method: string, _params: any) => {
      if (method === 'Page.getLayoutMetrics') {
        return { cssContentSize: { width: 800, height: 2400 }, contentSize: { width: 640, height: 1200 } };
      }
      return { data: 'FULL' };
    });
    const { deps } = makeDeps({ sendCdp: sendCdp as any });
    const result = await captureScreenshot({ target: 'full_page', format: 'png' }, tab, deps);

    expect(sendCdp.mock.calls[0][1]).toBe('Page.getLayoutMetrics');
    expect(sendCdp.mock.calls[1][1]).toBe('Page.captureScreenshot');
    expect(sendCdp.mock.calls[1][2]).toEqual({
      format: 'png',
      fromSurface: true,
      captureBeyondViewport: true,
      clip: { x: 0, y: 0, width: 800, height: 2400, scale: 1 },
    });
    expect(result.data).toBe('FULL');
    expect(result.target).toBe('full_page');
  });

  it('falls back to contentSize when cssContentSize is absent', async () => {
    const sendCdp = vi.fn(async (_tabId: number, method: string, _params: any) => {
      if (method === 'Page.getLayoutMetrics') return { contentSize: { width: 640, height: 1200 } };
      return { data: 'FULL' };
    });
    const { deps } = makeDeps({ sendCdp: sendCdp as any });
    await captureScreenshot({ target: 'full_page' }, tab, deps);
    expect((sendCdp.mock.calls[1] as any)[2].clip).toEqual({ x: 0, y: 0, width: 640, height: 1200, scale: 1 });
  });

  it('captures an element clip from the resolved @e ref and reports its page bbox', async () => {
    const { deps, sendCdp, resolveElementRect } = makeDeps();
    const result = await captureScreenshot({ target: 'element', ref: '@e4', format: 'webp' }, tab, deps);

    expect(resolveElementRect).toHaveBeenCalledWith(7, '@e4');
    expect(sendCdp.mock.calls[0][2]).toEqual({
      format: 'webp',
      quality: 80,
      fromSurface: true,
      captureBeyondViewport: true,
      clip: { x: 10, y: 20, width: 30, height: 40, scale: 1 },
    });
    expect(result.bbox).toEqual({ x: 10, y: 20, width: 30, height: 40 });
    expect(result.target).toBe('element');
    expect(result.url).toBe('http://page.test/frames.html');
  });

  it('rejects an element without a visible box before asking the browser to capture', async () => {
    const { deps, sendCdp, resolveElementRect } = makeDeps();
    resolveElementRect.mockResolvedValueOnce({ x: 10, y: 20, width: 0, height: 40, url: null } as any);
    await expect(captureScreenshot({ target: 'element', ref: '@e4' }, tab, deps)).rejects.toMatchObject({
      code: 'BAD_ARGS',
    });
    expect(sendCdp).not.toHaveBeenCalled();
  });

  it('propagates a missing element ref as NOT_FOUND, not INTERNAL', async () => {
    const { deps, sendCdp } = makeDeps({
      resolveElementRect: vi.fn(async () => {
        throw new ToolError('NOT_FOUND', 'the element ref is no longer available');
      }) as any,
    });
    await expect(captureScreenshot({ target: 'element', ref: '@e999' }, tab, deps)).rejects.toMatchObject({
      code: 'NOT_FOUND',
    });
    expect(sendCdp).not.toHaveBeenCalled();
  });

  it('does not resolve @c refs or malformed refs through the content script', async () => {
    const { deps, resolveElementRect, sendCdp } = makeDeps();
    await expect(captureScreenshot({ target: 'element', ref: '@c1' }, tab, deps)).rejects.toMatchObject({
      code: 'BAD_ARGS',
    });
    expect(resolveElementRect).not.toHaveBeenCalled();
    expect(sendCdp).not.toHaveBeenCalled();
  });
});

describe('browser_screenshot fallback and errors', () => {
  it('uses captureVisibleTab only for a PNG viewport when the debugger is unavailable', async () => {
    const { deps, captureVisibleTab } = makeDeps({
      sendCdp: vi.fn(async () => {
        throw new ToolError('CAPABILITY_UNAVAILABLE', 'the debugger channel is unavailable', {
          reason: 'debugger_attached_elsewhere',
        });
      }) as any,
    });
    const result = await captureScreenshot({ format: 'png' }, tab, deps);
    expect(captureVisibleTab).toHaveBeenCalledWith(3);
    expect(result).toMatchObject({ format: 'png', mime_type: 'image/png', data: 'iVBORw0KGgo=' });
  });

  it('refuses webp and jpeg viewport fallback with a clear capability error', async () => {
    for (const format of ['webp', 'jpeg']) {
      const { deps, captureVisibleTab } = makeDeps({
        sendCdp: vi.fn(async () => {
          throw new ToolError('CAPABILITY_UNAVAILABLE', 'the debugger channel is unavailable');
        }) as any,
      });
      await expect(captureScreenshot({ format }, tab, deps)).rejects.toMatchObject({
        code: 'CAPABILITY_UNAVAILABLE',
        message: expect.stringContaining('only PNG'),
      });
      expect(captureVisibleTab).not.toHaveBeenCalled();
    }
  });

  it('refuses the PNG fallback when the target tab is not active', async () => {
    const { deps, captureVisibleTab } = makeDeps({
      sendCdp: vi.fn(async () => {
        throw new ToolError('CAPABILITY_UNAVAILABLE', 'the debugger channel is unavailable');
      }) as any,
    });
    await expect(captureScreenshot({ format: 'png' }, { ...tab, active: false }, deps)).rejects.toMatchObject({
      code: 'CAPABILITY_UNAVAILABLE',
      details: { reason: 'inactive_tab' },
    });
    expect(captureVisibleTab).not.toHaveBeenCalled();
  });

  it('maps browser, timeout and unsupported-method failures to typed errors', async () => {
    const cases: Array<[string, string]> = [
      ['No tab with id: 9', 'BROWSER_GONE'],
      ['Inspected target navigated or closed', 'BROWSER_GONE'],
      ['screenshot timed out', 'TIMEOUT'],
      ['Method not found', 'CAPABILITY_UNAVAILABLE'],
    ];
    for (const [message, code] of cases) {
      const { deps } = makeDeps({
        sendCdp: vi.fn(async () => {
          throw new Error(message);
        }) as any,
      });
      await expect(captureScreenshot({}, tab, deps)).rejects.toMatchObject({ code });
    }
  });

  it('maps empty CDP image data and malformed fallback payloads to CAPABILITY_UNAVAILABLE', async () => {
    const empty = makeDeps({ sendCdp: vi.fn(async () => ({ data: '' })) as any });
    await expect(captureScreenshot({ target: 'element', ref: '@e4' }, tab, empty.deps)).rejects.toMatchObject({
      code: 'CAPABILITY_UNAVAILABLE',
      details: { reason: 'empty_capture' },
    });

    const emptyViewport = makeDeps({ sendCdp: vi.fn(async () => ({ data: '' })) as any });
    await expect(captureScreenshot({ format: 'png' }, tab, emptyViewport.deps)).rejects.toMatchObject({
      code: 'CAPABILITY_UNAVAILABLE',
      details: { reason: 'empty_capture' },
    });
    expect(emptyViewport.captureVisibleTab).not.toHaveBeenCalled();

    const malformed = makeDeps({
      sendCdp: vi.fn(async () => {
        throw new ToolError('CAPABILITY_UNAVAILABLE', 'unavailable');
      }) as any,
      captureVisibleTab: vi.fn(async () => 'not-a-data-url') as any,
    });
    await expect(captureScreenshot({ format: 'png' }, tab, malformed.deps)).rejects.toMatchObject({
      code: 'CAPABILITY_UNAVAILABLE',
    });
  });

  it('passes the raw base64 payload through for both inline settings', async () => {
    for (const inline of [false, true]) {
      const { deps } = makeDeps();
      const result = await captureScreenshot({ inline }, tab, deps);
      expect(result.data).toBe('AAAA');
      expect(result).not.toHaveProperty('inline');
    }
  });

  it('reports the MIME type for every supported format', () => {
    expect(mimeTypeFor('webp')).toBe('image/webp');
    expect(mimeTypeFor('png')).toBe('image/png');
    expect(mimeTypeFor('jpeg')).toBe('image/jpeg');
  });
});
