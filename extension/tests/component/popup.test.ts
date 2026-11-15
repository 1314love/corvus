import { flushPromises, mount } from '@vue/test-utils';
import { createPinia } from 'pinia';
import { afterEach, describe, expect, it, vi } from 'vitest';
import PopupApp from '../../src/ui/popup/App.vue';
import { applyLocale, i18n } from '../../src/ui/shared/i18n';
import { installFakeUiPort, sampleState } from '../helpers/ui-port';

afterEach(() => {
  applyLocale('en');
  vi.unstubAllGlobals();
});

describe('popup component', () => {
  it('renders the typed projection from the service worker', async () => {
    installFakeUiPort(() => ({ ok: true, state: sampleState({ status: 'connected' }) }));
    applyLocale('en');
    const wrapper = mount(PopupApp, { global: { plugins: [createPinia(), i18n] } });
    await flushPromises();
    await wrapper.vm.$nextTick();

    const text = wrapper.text();
    expect(text).toContain('Corvus');
    expect(text).toContain('connected');
    expect(text).toContain('ws://127.0.0.1:29330');
    expect(text).toContain('chromium:test');
    expect(text).toContain('Chrome/157.0.0.0');
    expect(text).toContain('Settings');
    wrapper.unmount();
  });

  it('renders the Chinese catalog when the locale is zh-CN', async () => {
    installFakeUiPort(() => ({ ok: true, state: sampleState({ status: 'connected' }) }));
    applyLocale('zh-CN');
    const wrapper = mount(PopupApp, { global: { plugins: [createPinia(), i18n] } });
    await flushPromises();
    await wrapper.vm.$nextTick();

    const text = wrapper.text();
    expect(text).toContain('守护进程');
    expect(text).toContain('已连接');
    expect(text).toContain('设置');
    expect(text).not.toContain('Settings');
    expect(document.documentElement.lang).toBe('zh-CN');
    wrapper.unmount();
  });
});
