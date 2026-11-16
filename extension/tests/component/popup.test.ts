import { flushPromises, mount } from '@vue/test-utils';
import { createPinia } from 'pinia';
import { afterEach, describe, expect, it, vi } from 'vitest';
import PopupApp from '../../src/ui/popup/App.vue';
import { applyLocale, i18n } from '../../src/ui/shared/i18n';
import { disconnectFakeUiPort, installFakeUiPort, sampleState } from '../helpers/ui-port';

afterEach(() => {
  disconnectFakeUiPort();
  applyLocale('en');
  vi.unstubAllGlobals();
});

describe('popup component', () => {
  it('renders the provider projection from the service worker', async () => {
    installFakeUiPort(() => ({ ok: true, state: sampleState() }));
    applyLocale('en');
    const wrapper = mount(PopupApp, { global: { plugins: [createPinia(), i18n] } });
    await flushPromises();
    await wrapper.vm.$nextTick();

    const text = wrapper.text();
    expect(text).toContain('Corvus');
    expect(text).toContain('connected');
    expect(text).toContain('ws://127.0.0.1:29330');
    expect(text).toContain('Chrome/157.0.0.0');
    expect(text).toContain('saved');
    expect(text).toContain('Settings');
    wrapper.unmount();
  });

  it('renders the Chinese catalog when the locale is zh-CN', async () => {
    installFakeUiPort(() => ({ ok: true, state: sampleState() }));
    applyLocale('zh-CN');
    const wrapper = mount(PopupApp, { global: { plugins: [createPinia(), i18n] } });
    await flushPromises();
    await wrapper.vm.$nextTick();

    const text = wrapper.text();
    expect(text).toContain('已连接');
    expect(text).toContain('本地地址');
    expect(text).toContain('配对令牌');
    expect(text).toContain('设置');
    expect(text).not.toContain('Settings');
    expect(document.documentElement.lang).toBe('zh-CN');
    wrapper.unmount();
  });

  it('shows a pairing-required state without a saved token', async () => {
    installFakeUiPort(() => ({
      ok: true,
      state: sampleState({
        status: 'pairing_required',
        connected: false,
        token_saved: false,
        browser: null,
      }),
    }));
    applyLocale('en');
    const wrapper = mount(PopupApp, { global: { plugins: [createPinia(), i18n] } });
    await flushPromises();
    await wrapper.vm.$nextTick();

    const text = wrapper.text();
    expect(text).toContain('pairing required');
    expect(text).toContain('not saved');
    expect(text).toContain('not connected');
    wrapper.unmount();
  });

  it('shows the safe last error message', async () => {
    installFakeUiPort(() => ({
      ok: true,
      state: sampleState({
        status: 'error',
        connected: false,
        last_error: { code: 'AUTHENTICATION', message: 'Pairing token was not accepted.' },
      }),
    }));
    applyLocale('en');
    const wrapper = mount(PopupApp, { global: { plugins: [createPinia(), i18n] } });
    await flushPromises();
    await wrapper.vm.$nextTick();

    expect(wrapper.text()).toContain('Pairing token was not accepted.');
    wrapper.unmount();
  });
});
