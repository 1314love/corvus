import { flushPromises, mount } from '@vue/test-utils';
import { createPinia } from 'pinia';
import { afterEach, describe, expect, it, vi } from 'vitest';
import PopupApp from '../../src/ui/popup/App.vue';
import { installFakeUiPort, sampleState } from '../helpers/ui-port';

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('popup component', () => {
  it('renders the typed projection from the service worker', async () => {
    installFakeUiPort(() => ({ ok: true, state: sampleState({ status: 'connected' }) }));
    const wrapper = mount(PopupApp, { global: { plugins: [createPinia()] } });
    await flushPromises();
    await wrapper.vm.$nextTick();

    const text = wrapper.text();
    expect(text).toContain('Corvus');
    expect(text).toContain('connected');
    expect(text).toContain('ws://127.0.0.1:29330');
    expect(text).toContain('chromium:test');
    expect(text).toContain('Chrome/157.0.0.0');
    wrapper.unmount();
  });
});
