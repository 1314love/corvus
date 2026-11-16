import { ToolError, describeError } from '../shared/errors';

export type ScreenshotTarget = 'viewport' | 'full_page' | 'element';
export type ScreenshotFormat = 'webp' | 'png' | 'jpeg';

export interface ScreenshotRequest {
  target: ScreenshotTarget;
  ref: string | null;
  format: ScreenshotFormat;
  quality: number;
  inline: boolean;
}

export interface ScreenshotElementRect {
  x: number;
  y: number;
  width: number;
  height: number;
  url?: string | null;
}

export interface ScreenshotTabInfo {
  id: number;
  url?: string;
  windowId?: number;
  active?: boolean;
}

export interface ScreenshotDeps {
  sendCdp: (tabId: number, method: string, params: any) => Promise<any>;
  captureVisibleTab: (windowId?: number) => Promise<string>;
  resolveElementRect: (tabId: number, ref: string) => Promise<ScreenshotElementRect>;
}

export const SCREENSHOT_TARGETS: readonly ScreenshotTarget[] = ['viewport', 'full_page', 'element'];
export const SCREENSHOT_FORMATS: readonly ScreenshotFormat[] = ['webp', 'png', 'jpeg'];

const MIME_TYPES: Readonly<Record<ScreenshotFormat, string>> = Object.freeze({
  webp: 'image/webp',
  png: 'image/png',
  jpeg: 'image/jpeg',
});

const SCREENSHOT_ARG_KEYS = ['target', 'ref', 'format', 'quality', 'inline', 'tab_id', 'browser_id'];

function readTarget(value: any): ScreenshotTarget {
  if (value === undefined || value === null) return 'viewport';
  if (typeof value !== 'string' || value.trim() === '') {
    throw new ToolError('BAD_ARGS', 'args.target must be viewport, full_page or element', { value: value });
  }
  const normalized = value.trim().toLowerCase();
  if ((SCREENSHOT_TARGETS as readonly string[]).indexOf(normalized) === -1) {
    throw new ToolError('BAD_ARGS', 'args.target must be viewport, full_page or element', { value: value });
  }
  return normalized as ScreenshotTarget;
}

function readFormat(value: any): ScreenshotFormat {
  if (value === undefined || value === null) return 'webp';
  if (typeof value !== 'string' || value.trim() === '') {
    throw new ToolError('BAD_ARGS', 'args.format must be webp, png or jpeg', { value: value });
  }
  const normalized = value.trim().toLowerCase();
  if ((SCREENSHOT_FORMATS as readonly string[]).indexOf(normalized) === -1) {
    throw new ToolError('BAD_ARGS', 'args.format must be webp, png or jpeg', { value: value });
  }
  return normalized as ScreenshotFormat;
}

function readQuality(value: any): number {
  if (value === undefined || value === null) return 80;
  const numeric = Number(value);
  if (!Number.isInteger(numeric) || numeric < 1 || numeric > 100) {
    throw new ToolError('BAD_ARGS', 'args.quality must be an integer from 1 to 100', { value: value });
  }
  return numeric;
}

function readInline(value: any): boolean {
  if (value === undefined || value === null) return false;
  if (typeof value !== 'boolean') {
    throw new ToolError('BAD_ARGS', 'args.inline must be a boolean', { value: value });
  }
  return value;
}

export function parseScreenshotArgs(args: any): ScreenshotRequest {
  if (!args || typeof args !== 'object' || Array.isArray(args)) {
    throw new ToolError('BAD_ARGS', 'browser_screenshot args must be an object');
  }
  const keys = Object.keys(args);
  for (let i = 0; i < keys.length; i += 1) {
    if (SCREENSHOT_ARG_KEYS.indexOf(keys[i]) === -1) {
      throw new ToolError('BAD_ARGS', 'unknown browser_screenshot argument: ' + keys[i], { argument: keys[i] });
    }
  }

  const target = readTarget(args.target);
  const format = readFormat(args.format);
  const quality = readQuality(args.quality);
  const inline = readInline(args.inline);

  if (target !== 'element') {
    if (args.ref !== undefined && args.ref !== null) {
      throw new ToolError('BAD_ARGS', 'args.ref is only valid when args.target is element', { ref: args.ref });
    }
    return { target: target, ref: null, format: format, quality: quality, inline: inline };
  }

  if (typeof args.ref !== 'string' || args.ref.trim() === '') {
    throw new ToolError('BAD_ARGS', 'target=element requires a @eN ref from page_snapshot', { ref: args.ref });
  }
  const ref = args.ref.trim();
  if (/^@c\d+$/i.test(ref)) {
    throw new ToolError(
      'BAD_ARGS',
      '@cN refs come from the CDP snapshot; browser_screenshot target=element requires a @eN ref from page_snapshot',
      { ref: ref },
    );
  }
  if (!/^@e\d+$/i.test(ref)) {
    throw new ToolError('BAD_ARGS', 'args.ref must be a @eN ref returned by page_snapshot', { ref: ref });
  }
  return { target: target, ref: ref, format: format, quality: quality, inline: inline };
}

export function mimeTypeFor(format: ScreenshotFormat): string {
  return MIME_TYPES[format];
}

function captureParams(request: ScreenshotRequest, beyondViewport: boolean, clip: any): any {
  const params: any = {
    format: request.format,
    fromSurface: true,
    captureBeyondViewport: beyondViewport,
  };
  if (request.format !== 'png') params.quality = request.quality;
  if (clip) params.clip = clip;
  return params;
}

function captureData(shot: any, target: string): string {
  if (!shot || typeof shot.data !== 'string' || shot.data === '') {
    throw new ToolError('CAPABILITY_UNAVAILABLE', 'the browser returned no image data for ' + target, {
      reason: 'empty_capture',
    });
  }
  return shot.data;
}

function mapScreenshotError(error: any): any {
  if (error instanceof ToolError) return error;
  const message = describeError(error);
  if (
    /No tab with id|Invalid tab ID|tab was closed|has been closed|BROWSER_GONE|Inspected target|navigated or closed|browser has disconnected/i.test(
      message,
    )
  ) {
    return new ToolError('BROWSER_GONE', 'the tab went away during screenshot capture: ' + message, {
      reason: 'tab_closed',
      cause: message,
    });
  }
  if (/timed? ?out|timeout/i.test(message)) {
    return new ToolError('TIMEOUT', 'screenshot capture timed out: ' + message, { cause: message });
  }
  if (
    /not attached|detached|devtools|another debugger|cannot attach|not allowed|method not found|not supported/i.test(
      message,
    )
  ) {
    return new ToolError('CAPABILITY_UNAVAILABLE', 'the debugger channel cannot capture screenshots: ' + message, {
      reason: 'debugger_unavailable',
      cause: message,
    });
  }
  return new ToolError('INTERNAL', 'screenshot capture failed: ' + message, { cause: message });
}

async function runCdp(deps: ScreenshotDeps, tabId: number, method: string, params: any): Promise<any> {
  try {
    return await deps.sendCdp(tabId, method, params);
  } catch (error: any) {
    throw mapScreenshotError(error);
  }
}

function decodePngDataUrl(dataUrl: any): string {
  const match = /^data:image\/png;base64,([A-Za-z0-9+/=]+)$/i.exec(String(dataUrl || '').trim());
  if (!match || !match[1]) {
    throw new ToolError('CAPABILITY_UNAVAILABLE', 'PNG viewport fallback returned no PNG payload', {
      reason: 'empty_capture',
    });
  }
  return match[1];
}

function readElementRect(raw: any, ref: string): ScreenshotElementRect {
  if (!raw || typeof raw !== 'object') {
    throw new ToolError('INTERNAL', 'the content script returned no element rectangle', { ref: ref });
  }
  const x = Number(raw.x);
  const y = Number(raw.y);
  const width = Number(raw.width);
  const height = Number(raw.height);
  if (![x, y, width, height].every((value) => Number.isFinite(value))) {
    throw new ToolError('INTERNAL', 'the content script returned an invalid element rectangle', { ref: ref });
  }
  if (width < 1 || height < 1) {
    throw new ToolError('BAD_ARGS', 'the element has no visible box to capture', { ref: ref });
  }
  return {
    x: x,
    y: y,
    width: width,
    height: height,
    url: typeof raw.url === 'string' ? raw.url : null,
  };
}

async function captureViewport(
  request: ScreenshotRequest,
  tab: ScreenshotTabInfo,
  deps: ScreenshotDeps,
): Promise<string> {
  try {
    const shot = await runCdp(deps, tab.id, 'Page.captureScreenshot', captureParams(request, false, null));
    return captureData(shot, 'viewport');
  } catch (error: any) {
    const emptyCapture =
      error instanceof ToolError &&
      error.code === 'CAPABILITY_UNAVAILABLE' &&
      Boolean(error.details) &&
      (error.details as any).reason === 'empty_capture';
    const capabilityFailure =
      !emptyCapture &&
      error instanceof ToolError &&
      (error.code === 'CAPABILITY_UNAVAILABLE' ||
        (error.code === 'INTERNAL' && /not supported|method not found|not allowed/i.test(error.message)));
    if (!capabilityFailure) throw error;
    if (request.format !== 'png') {
      throw new ToolError(
        'CAPABILITY_UNAVAILABLE',
        'the debugger channel is unavailable; viewport fallback supports only PNG (requested ' + request.format + ')',
        { reason: 'debugger_unavailable', format: request.format, cause: error.details || null },
      );
    }
    if (tab.active !== true) {
      throw new ToolError(
        'CAPABILITY_UNAVAILABLE',
        'the debugger channel is unavailable and the target tab is not active for the PNG fallback',
        { reason: 'inactive_tab', tab_id: String(tab.id), cause: error.details || null },
      );
    }
    let dataUrl;
    try {
      dataUrl = await deps.captureVisibleTab(tab.windowId);
    } catch (fallbackError: any) {
      throw new ToolError('CAPABILITY_UNAVAILABLE', 'PNG viewport fallback failed: ' + describeError(fallbackError), {
        reason: 'visible_tab_fallback_failed',
        cause: describeError(fallbackError),
      });
    }
    return decodePngDataUrl(dataUrl);
  }
}

async function captureFullPage(
  request: ScreenshotRequest,
  tab: ScreenshotTabInfo,
  deps: ScreenshotDeps,
): Promise<string> {
  const metrics = await runCdp(deps, tab.id, 'Page.getLayoutMetrics', {});
  const size = metrics && (metrics.cssContentSize || metrics.contentSize);
  const width = Number(size && size.width);
  const height = Number(size && size.height);
  if (!Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0) {
    throw new ToolError('CAPABILITY_UNAVAILABLE', 'the page did not report a capturable content size', {
      reason: 'geometry_unavailable',
    });
  }
  const clip = { x: 0, y: 0, width: width, height: height, scale: 1 };
  const shot = await runCdp(deps, tab.id, 'Page.captureScreenshot', captureParams(request, true, clip));
  return captureData(shot, 'full_page');
}

async function captureElement(
  request: ScreenshotRequest,
  tab: ScreenshotTabInfo,
  deps: ScreenshotDeps,
): Promise<{ data: string; bbox: any; url: string }> {
  const ref = request.ref as string;
  const rect = readElementRect(await deps.resolveElementRect(tab.id, ref), ref);
  const clip = { x: rect.x, y: rect.y, width: rect.width, height: rect.height, scale: 1 };
  const shot = await runCdp(deps, tab.id, 'Page.captureScreenshot', captureParams(request, true, clip));
  return {
    data: captureData(shot, 'element'),
    bbox: { x: rect.x, y: rect.y, width: rect.width, height: rect.height },
    url: rect.url || tab.url || '',
  };
}

export async function captureScreenshot(args: any, tab: ScreenshotTabInfo, deps: ScreenshotDeps): Promise<any> {
  const request = parseScreenshotArgs(args);
  if (!Number.isInteger(tab.id) || tab.id < 0) {
    throw new ToolError('BAD_ARGS', 'browser_screenshot needs a valid tab id');
  }

  let data: string;
  let bbox: any = null;
  let url = tab.url || '';
  if (request.target === 'viewport') {
    data = await captureViewport(request, tab, deps);
  } else if (request.target === 'full_page') {
    data = await captureFullPage(request, tab, deps);
  } else {
    const elementCapture = await captureElement(request, tab, deps);
    data = elementCapture.data;
    bbox = elementCapture.bbox;
    url = elementCapture.url;
  }

  const result: any = {
    format: request.format,
    mime_type: mimeTypeFor(request.format),
    target: request.target,
    tab_id: String(tab.id),
    url: url,
    data: data,
  };
  if (bbox) result.bbox = bbox;
  return result;
}
