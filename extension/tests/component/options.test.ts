import { flushPromises, mount } from '@vue/test-utils';
import { createPinia } from 'pinia';
import { afterEach, describe, expect, it, vi } from 'vitest';
import OptionsApp from '../../src/ui/options/App.vue';
import { applyLocale, i18n, LOCALE_STORAGE_KEY } from '../../src/ui/shared/i18n';
import { installFakeChromeStorage, installFakeUiPort, sampleState } from '../helpers/ui-port';

afterEach(() => {
  applyLocale('en');
  vi.unstubAllGlobals();
});

describe('options component', () => {
  it('shows daemon and CDP settings and sends a typed daemon save command', async () => {
    const harness = installFakeUiPort((message) => {
      if (message.type === 'corvus.daemon_save') {
        return { ok: true, state: sampleState({ wsUrl: String(message.payload.url) }) };
      }
      return { ok: true, state: sampleState() };
    });
    installFakeChromeStorage();
    applyLocale('en');

    const wrapper = mount(OptionsApp, { global: { plugins: [createPinia(), i18n] } });
    await flushPromises();
    await wrapper.vm.$nextTick();

    expect(wrapper.text()).toContain('Daemon connection');
    expect(wrapper.text()).toContain('Browser CDP endpoint');
    expect(wrapper.text()).toContain('Chrome/157.0.0.0');
    expect(wrapper.text()).toContain('match');

    const inputs = wrapper.findAll('input');
    expect(inputs.length).toBeGreaterThanOrEqual(2);
    await inputs[0].setValue('ws://127.0.0.1:29331');

    const save = wrapper.findAll('button').find((button) => button.text() === 'Save');
    expect(save).toBeDefined();
    await save?.trigger('click');
    await flushPromises();

    const saved = harness.messages.find((message) => message.type === 'corvus.daemon_save');
    expect(saved).toBeDefined();
    expect(saved?.payload.url).toBe('ws://127.0.0.1:29331');
    expect(wrapper.text()).toContain('Saved. Reconnecting...');
    wrapper.unmount();
  });

  it('switches locale reactively and persists the explicit choice', async () => {
    installFakeUiPort(() => ({ ok: true, state: sampleState() }));
    const storage = installFakeChromeStorage();
    applyLocale('en');

    const wrapper = mount(OptionsApp, { global: { plugins: [createPinia(), i18n] } });
    await flushPromises();
    await wrapper.vm.$nextTick();

    expect(wrapper.text()).toContain('Daemon connection');
    expect(document.documentElement.lang).toBe('en');

    const chinese = wrapper.findAll('button').find((button) => button.text() === '中文');
    expect(chinese).toBeDefined();
    await chinese?.trigger('click');
    await flushPromises();

    expect(wrapper.text()).toContain('守护进程连接');
    expect(wrapper.text()).toContain('浏览器 CDP 端点');
    expect(wrapper.text()).not.toContain('Daemon connection');
    expect(document.documentElement.lang).toBe('zh-CN');
    expect(storage.values[LOCALE_STORAGE_KEY]).toBe('zh-CN');

    const english = wrapper.findAll('button').find((button) => button.text() === 'English');
    expect(english).toBeDefined();
    await english?.trigger('click');
    await flushPromises();

    expect(wrapper.text()).toContain('Daemon connection');
    expect(document.documentElement.lang).toBe('en');
    expect(storage.values[LOCALE_STORAGE_KEY]).toBe('en');
    wrapper.unmount();
  });
});
