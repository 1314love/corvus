import { describe, expect, it, vi } from 'vitest';
import {
  ProviderActionError,
  dispatchProviderAction,
  parseProviderAction,
  providerCapabilities,
  providerSystemInfo,
  type ProviderActionDeps,
  type ProviderTabInfo,
} from '../../src/background/provider-actions';

function deps(overrides: Partial<ProviderActionDeps> = {}): ProviderActionDeps {
  return {
    listTabs: vi.fn(async () => [] as ProviderTabInfo[]),
    openTab: vi.fn(async (url: string, active: boolean) => ({
      tab_id: '8',
      window_id: 1,
      title: url,
      url,
      active,
    })),
    closeTab: vi.fn(async () => undefined),
    navigate: vi.fn(async () => ({ tab_id: '7', url: 'https://example.com/', status: 'loaded' as const })),
    contentAction: vi.fn(async () => ({ ok: true })),
    captureViewport: vi.fn(async () => ({
      tab_id: '7',
      url: 'https://example.com/',
      format: 'png' as const,
      mime_type: 'image/png' as const,
      width: 1,
      height: 1,
      data: 'AA==',
    })),
    systemInfo: providerSystemInfo,
    ...overrides,
  };
}

describe('provider action parsing', () => {
  it('parses every P2 action with its documented defaults', () => {
    expect(parseProviderAction('system.info.info', {})).toEqual({ action: 'system.info.info', args: {} });
    expect(parseProviderAction('tabs.list', {})).toEqual({ action: 'tabs.list', args: {} });
    expect(parseProviderAction('tabs.open', { url: 'https://example.com/' })).toMatchObject({
      action: 'tabs.open',
      args: { url: 'https://example.com/', active: true },
    });
    expect(parseProviderAction('tabs.close', { tab_id: '7' })).toMatchObject({ args: { tab_id: '7' } });
    expect(parseProviderAction('page.navigate', { url: 'https://example.com/' })).toMatchObject({
      args: { url: 'https://example.com/', wait_until: 'load', timeout_ms: 30000 },
    });
    expect(parseProviderAction('page.read', {})).toMatchObject({
      args: { mode: 'text', max_chars: 20000 },
    });
    expect(parseProviderAction('page.snapshot', {})).toMatchObject({
      args: { max_nodes: 500, include_hidden: false },
    });
    expect(parseProviderAction('input.click', { ref: '@e1' })).toMatchObject({ args: { ref: '@e1' } });
    expect(parseProviderAction('input.type', { ref: '@e1', text: 'hello' })).toMatchObject({
      args: { ref: '@e1', text: 'hello', clear: true, submit: false },
    });
    expect(parseProviderAction('capture.viewport', {})).toMatchObject({
      args: { format: 'png', quality: undefined },
    });
    expect(parseProviderAction('capture.viewport', { format: 'png', quality: 10 })).toMatchObject({
      args: { format: 'png', quality: undefined },
    });
  });

  it('rejects unknown groups, unknown actions and unknown fields with the frozen codes', () => {
    expect(() => parseProviderAction('storage.read', {})).toThrow(ProviderActionError);
    try {
      parseProviderAction('storage.read', {});
    } catch (error) {
      expect((error as ProviderActionError).code).toBe('CAPABILITY_UNAVAILABLE');
    }
    expect(() => parseProviderAction('tabs.remove', { tab_id: '1' })).toThrow(ProviderActionError);
    try {
      parseProviderAction('tabs.remove', { tab_id: '1' });
    } catch (error) {
      expect((error as ProviderActionError).code).toBe('BAD_ARGS');
    }
    expect(() => parseProviderAction('page.read', { extra: true })).toThrow(ProviderActionError);
  });

  it('enforces action bounds and quality limits', () => {
    const bad: Array<[string, Record<string, unknown>]> = [
      ['tabs.open', { url: '' }],
      ['tabs.open', { url: 'https://example.com/', active: 'yes' }],
      ['tabs.close', { tab_id: '' }],
      ['page.navigate', { url: 'https://example.com/', timeout_ms: 999 }],
      ['page.navigate', { url: 'https://example.com/', wait_until: 'networkidle' }],
      ['page.read', { max_chars: 999 }],
      ['page.read', { mode: 'markdown' }],
      ['page.snapshot', { max_nodes: 49 }],
      ['input.type', { ref: '@e1', text: 'x'.repeat(20001) }],
      ['capture.viewport', { format: 'webp' }],
      ['capture.viewport', { format: 'jpeg', quality: 0 }],
    ];
    for (const [action, args] of bad) {
      try {
        parseProviderAction(action, args);
        throw new Error('expected a provider action error for ' + action);
      } catch (error) {
        expect(error).toBeInstanceOf(ProviderActionError);
        expect((error as ProviderActionError).code).toBe('BAD_ARGS');
      }
    }
  });
});

describe('provider action dispatch', () => {
  it('caps tabs.list at 200 and keeps count deterministic', async () => {
    const tabs = Array.from({ length: 250 }, (_, index) => ({
      tab_id: String(index + 1),
      window_id: 1,
      title: 'Tab ' + String(index + 1),
      url: 'https://example.com/' + String(index + 1),
      active: index === 0,
    }));
    const result = (await dispatchProviderAction(
      parseProviderAction('tabs.list', {}),
      deps({ listTabs: async () => tabs }),
    )) as {
      count: number;
      tabs: unknown[];
    };
    expect(result.count).toBe(200);
    expect(result.tabs).toHaveLength(200);
  });

  it('forwards normalized arguments to tabs, page, input and capture deps', async () => {
    const fake = deps({
      navigate: vi.fn(async () => ({ tab_id: '3', url: 'https://example.com/', status: 'loaded' as const })),
      contentAction: vi.fn(async () => ({ ok: true })),
      captureViewport: vi.fn(async () => ({
        tab_id: '3',
        url: 'https://example.com/',
        format: 'jpeg' as const,
        mime_type: 'image/jpeg' as const,
        width: 2,
        height: 2,
        data: 'AA==',
      })),
    });
    expect(
      await dispatchProviderAction(
        parseProviderAction('tabs.open', { url: 'https://example.com/', active: false }),
        fake,
      ),
    ).toMatchObject({
      tab_id: '8',
      active: false,
    });
    await dispatchProviderAction(parseProviderAction('tabs.close', { tab_id: '9' }), fake);
    expect(fake.closeTab).toHaveBeenCalledWith('9');
    await dispatchProviderAction(
      parseProviderAction('page.navigate', {
        tab_id: '3',
        url: 'https://example.com/',
        wait_until: 'domcontentloaded',
        timeout_ms: 5000,
      }),
      fake,
    );
    expect(fake.navigate).toHaveBeenCalledWith('3', 'https://example.com/', 'domcontentloaded', 5000);
    await dispatchProviderAction(
      parseProviderAction('page.read', { tab_id: '3', selector: '#main', max_chars: 1000 }),
      fake,
    );
    expect(fake.contentAction).toHaveBeenCalledWith('3', 'page.read', {
      mode: 'text',
      selector: '#main',
      max_chars: 1000,
    });
    await dispatchProviderAction(parseProviderAction('page.snapshot', { max_nodes: 50 }), fake);
    expect(fake.contentAction).toHaveBeenCalledWith(undefined, 'page.snapshot', {
      max_nodes: 50,
      include_hidden: false,
    });
    await dispatchProviderAction(parseProviderAction('input.click', { ref: '@e4' }), fake);
    expect(fake.contentAction).toHaveBeenCalledWith(undefined, 'input.click', { ref: '@e4' });
    await dispatchProviderAction(parseProviderAction('input.type', { ref: '@e4', text: 'x', submit: true }), fake);
    expect(fake.contentAction).toHaveBeenCalledWith(undefined, 'input.type', {
      ref: '@e4',
      text: 'x',
      clear: true,
      submit: true,
    });
    await dispatchProviderAction(parseProviderAction('capture.viewport', { format: 'jpeg', quality: 60 }), fake);
    expect(fake.captureViewport).toHaveBeenCalledWith(undefined, 'jpeg', 60);
    await dispatchProviderAction(parseProviderAction('capture.viewport', { format: 'png', quality: 10 }), fake);
    expect(fake.captureViewport).toHaveBeenCalledWith(undefined, 'png', undefined);
  });

  it('exposes the P2 manifest groups and limits', () => {
    expect(providerCapabilities()).toEqual([
      { id: 'system.info', version: 1, actions: ['info'] },
      { id: 'tabs', version: 1, actions: ['list', 'open', 'close'] },
      { id: 'page', version: 1, actions: ['navigate', 'read', 'snapshot'] },
      { id: 'input', version: 1, actions: ['click', 'type'] },
      { id: 'capture', version: 1, actions: ['viewport'] },
    ]);
    const info = providerSystemInfo();
    expect(info.limits).toEqual({ max_invoke_bytes: 1048576, max_result_bytes: 8388608 });
    expect(info.capabilities.map((group) => group.group)).toEqual(['tabs', 'page', 'input', 'capture']);
  });
});
