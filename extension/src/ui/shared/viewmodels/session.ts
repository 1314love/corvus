/** Options/popup ViewModel: pairing drafts, commands and derived presentation state. */

import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import { storeToRefs } from 'pinia';
import { DEFAULT_DAEMON_URL } from '../../../shared/storage/daemon-url';
import { providerEndpointFromBase } from '../../../shared/provider/endpoint';
import type { ProviderUiState, UiReply } from '../../../shared/contracts/ui';
import { errorMessage, uiService } from '../rpc/service';
import { useSessionStore } from '../stores/session';

export function useSessionViewModel(pollMs = 1500) {
  const store = useSessionStore();
  const { snapshot, loading, error } = storeToRefs(store);

  const status = computed(() => store.status);
  const connected = computed(() => store.connected);
  const tokenSaved = computed(() => store.tokenSaved);
  const browser = computed(() => store.browser);

  const baseDraft = ref(DEFAULT_DAEMON_URL);
  const tokenDraft = ref('');
  const baseError = ref('');
  const notice = ref('');
  const busy = ref(false);
  let initialized = false;
  let timer: ReturnType<typeof setInterval> | null = null;

  function applyState(state: ProviderUiState, forceDraft = false): void {
    store.setSnapshot(state);
    if (!initialized || forceDraft) {
      baseDraft.value = state.base_url;
      initialized = true;
    }
  }

  watch(
    snapshot,
    (value) => {
      if (value && !initialized) applyState(value);
    },
    { immediate: true },
  );

  async function refresh(): Promise<void> {
    store.setLoading(true);
    try {
      const reply = await uiService.getState();
      if (reply.ok) applyState(reply.state);
      else store.setError(reply.error);
    } catch (failure) {
      store.setError(errorMessage(failure));
    } finally {
      store.setLoading(false);
    }
  }

  async function perform(action: () => Promise<UiReply>, success: string, forceDraft = false): Promise<void> {
    busy.value = true;
    notice.value = '';
    try {
      const reply = await action();
      if (reply.ok) {
        applyState(reply.state, forceDraft);
        notice.value = success;
      } else {
        store.setError(reply.error);
      }
    } catch (failure) {
      store.setError(errorMessage(failure));
    } finally {
      busy.value = false;
    }
  }

  async function saveAndConnect(): Promise<void> {
    baseError.value = '';
    const endpoint = providerEndpointFromBase(baseDraft.value);
    if (!endpoint) {
      baseError.value = 'validation.base';
      return;
    }
    const token = tokenDraft.value.trim();
    if (!token && !tokenSaved.value) {
      baseError.value = 'validation.token';
      return;
    }
    await perform(() => uiService.saveAndConnect(baseDraft.value.trim(), token || undefined), 'notice.saved', true);
    tokenDraft.value = '';
  }

  async function connect(): Promise<void> {
    await perform(() => uiService.connect(), 'notice.connecting');
  }

  async function disconnect(): Promise<void> {
    await perform(() => uiService.disconnect(), 'notice.disconnected');
  }

  function openOptions(): void {
    void chrome.runtime.openOptionsPage();
  }

  onMounted(() => {
    void refresh();
    if (pollMs > 0) timer = setInterval(() => void refresh(), pollMs);
  });

  onUnmounted(() => {
    if (timer !== null) clearInterval(timer);
  });

  return {
    state: snapshot,
    loading,
    error,
    status,
    connected,
    tokenSaved,
    browser,
    baseDraft,
    tokenDraft,
    baseError,
    notice,
    busy,
    baseDefault: DEFAULT_DAEMON_URL,
    refresh,
    saveAndConnect,
    connect,
    disconnect,
    openOptions,
  };
}
