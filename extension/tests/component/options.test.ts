import { flushPromises, mount } from '@vue/test-utils';
import { createPinia } from 'pinia';
import { afterEach, describe, expect, it, vi } from 'vitest';
import OptionsApp from '../../src/ui/options/App.vue';
import { applyLocale, i18n, LOCALE_STORAGE_KEY } from '../../src/ui/shared/i18n';
import { disconnectFakeUiPort, installFakeChromeStorage, installFakeUiPort, sampleState } from '../helpers/ui-port';

afterEach(() => {
  disconnectFakeUiPort();
  applyLocale('en');
  vi.unstubAllGlobals();
});

describe('options component', () => {
  it('shows pairing controls and sends a typed save-and-connect command', async () => {
    const harness = installFakeUiPort((message) => {
      if (message.type === 'corvus.save_and_connect') {
        return { ok: true, state: sampleState({ base_url: String(message.payload.base_url) }) };
      }
      return { ok: true, state: sampleState() };
    });
    installFakeChromeStorage();
    applyLocale('en');

    const wrapper = mount(OptionsApp, { global: { plugins: [createPinia(), i18n] } });
    await flushPromises();
    await wrapper.vm.$nextTick();

    expect(wrapper.text()).toContain('Local pairing');
    expect(wrapper.text()).toContain('Local address');
    expect(wrapper.text()).toContain('Chrome/157.0.0.0');
    expect(wrapper.text()).toContain('provider-test');
    expect(wrapper.text()).toContain('saved');

    const inputs = wrapper.findAll('input');
    expect(inputs.length).toBeGreaterThanOrEqual(2);
    await inputs[0].setValue('ws://127.0.0.1:29331');
    await inputs[1].setValue('pairing-token-value');

    const save = wrapper.findAll('button').find((button) => button.text() === 'Save and connect');
    expect(save).toBeDefined();
    await save?.trigger('click');
    await flushPromises();

    const saved = harness.messages.find((message) => message.type === 'corvus.save_and_connect');
    expect(saved).toBeDefined();
    expect(saved?.payload.base_url).toBe('ws://127.0.0.1:29331');
    expect(saved?.payload.token).toBe('pairing-token-value');
    expect(wrapper.text()).toContain('Saved. Connecting...');
    wrapper.unmount();
  });

  it('rejects a non-loopback address without sending a command', async () => {
    const harness = installFakeUiPort(() => ({ ok: true, state: sampleState() }));
    installFakeChromeStorage();
    applyLocale('en');

    const wrapper = mount(OptionsApp, { global: { plugins: [createPinia(), i18n] } });
    await flushPromises();
    await wrapper.vm.$nextTick();

    await wrapper.findAll('input')[0].setValue('https://example.com/daemon');
    const save = wrapper.findAll('button').find((button) => button.text() === 'Save and connect');
    await save?.trigger('click');
    await flushPromises();

    expect(wrapper.text()).toContain('Enter a loopback ws:// URL');
    expect(harness.messages.some((message) => message.type === 'corvus.save_and_connect')).toBe(false);
    wrapper.unmount();
  });

  it('switches locale reactively and persists the explicit choice', async () => {
    installFakeUiPort(() => ({ ok: true, state: sampleState() }));
    const storage = installFakeChromeStorage();
    applyLocale('en');

    const wrapper = mount(OptionsApp, { global: { plugins: [createPinia(), i18n] } });
    await flushPromises();
    await wrapper.vm.$nextTick();

    expect(wrapper.text()).toContain('Local pairing');
    expect(document.documentElement.lang).toBe('en');

    const chinese = wrapper.findAll('button').find((button) => button.text() === '中文');
    expect(chinese).toBeDefined();
    await chinese?.trigger('click');
    await flushPromises();

    expect(wrapper.text()).toContain('本地配对');
    expect(wrapper.text()).toContain('本地地址');
    expect(wrapper.text()).not.toContain('Local pairing');
    expect(document.documentElement.lang).toBe('zh-CN');
    expect(storage.values[LOCALE_STORAGE_KEY]).toBe('zh-CN');

    const english = wrapper.findAll('button').find((button) => button.text() === 'English');
    expect(english).toBeDefined();
    await english?.trigger('click');
    await flushPromises();

    expect(wrapper.text()).toContain('Local pairing');
    expect(document.documentElement.lang).toBe('en');
    expect(storage.values[LOCALE_STORAGE_KEY]).toBe('en');
    wrapper.unmount();
  });

  it('projects the safe last error from the service worker', async () => {
    installFakeUiPort(() => ({
      ok: true,
      state: sampleState({
        status: 'error',
        connected: false,
        last_error: { code: 'VERSION', message: 'The local service protocol version is not supported.' },
      }),
    }));
    installFakeChromeStorage();
    applyLocale('zh-CN');
    const wrapper = mount(OptionsApp, { global: { plugins: [createPinia(), i18n] } });
    await flushPromises();
    await wrapper.vm.$nextTick();

    expect(wrapper.text()).toContain('The local service protocol version is not supported.');
    wrapper.unmount();
  });
});
