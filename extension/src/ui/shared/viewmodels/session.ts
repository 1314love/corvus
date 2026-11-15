/** Options/popup ViewModel: commands, drafts and derived presentation state. */

import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import { storeToRefs } from 'pinia';
import { DEFAULT_CDP_ENDPOINT, validateCdpEndpoint } from '../../../shared/cdp-endpoint';
import { DEFAULT_DAEMON_URL, validateDaemonUrl } from '../../../shared/storage/daemon-url';
import type { UiReply, UiState } from '../../../shared/contracts/ui';
import { errorMessage, uiService } from '../rpc/service';
import { useSessionStore } from '../stores/session';

export function useSessionViewModel(pollMs = 1500) {
  const store = useSessionStore();
  const { snapshot, loading, error } = storeToRefs(store);

  const status = computed(() => store.status);
  const connected = computed(() => store.connected);
  const cdpAvailable = computed(() => store.cdpAvailable);

  const daemonDraft = ref('');
  const cdpDraft = ref(DEFAULT_CDP_ENDPOINT);
  const daemonError = ref('');
  const cdpError = ref('');
  const notice = ref('');
  const busy = ref(false);
  let initialized = false;
  let timer: ReturnType<typeof setInterval> | null = null;

  function applyState(state: UiState): void {
    store.setSnapshot(state);
    if (!initialized) {
      daemonDraft.value = state.wsUrl;
      cdpDraft.value = state.cdpEndpoint || DEFAULT_CDP_ENDPOINT;
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

  async function perform(action: () => Promise<UiReply>, success: string): Promise<void> {
    busy.value = true;
    notice.value = '';
    try {
      const reply = await action();
      if (reply.ok) {
        store.setSnapshot(reply.state);
        daemonDraft.value = reply.state.wsUrl;
        cdpDraft.value = reply.state.cdpEndpoint || DEFAULT_CDP_ENDPOINT;
        initialized = true;
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

  async function saveDaemon(): Promise<void> {
    daemonError.value = '';
    const checked = validateDaemonUrl(daemonDraft.value);
    if (!checked.ok) {
      daemonError.value = checked.error || 'Enter a valid daemon URL.';
      return;
    }
    daemonDraft.value = checked.value || daemonDraft.value;
    await perform(() => uiService.saveDaemon(daemonDraft.value), 'Saved. Reconnecting...');
  }

  async function resetDaemon(): Promise<void> {
    daemonError.value = '';
    daemonDraft.value = DEFAULT_DAEMON_URL;
    await perform(() => uiService.resetDaemon(), 'Reset to the local default. Reconnecting...');
  }

  async function saveCdp(): Promise<void> {
    cdpError.value = '';
    const text = cdpDraft.value.trim() || DEFAULT_CDP_ENDPOINT;
    const checked = validateCdpEndpoint(text);
    if (!checked.ok) {
      cdpError.value = checked.error || 'Enter auto or a loopback http:// URL.';
      return;
    }
    cdpDraft.value = checked.value || text;
    await perform(() => uiService.saveCdp(cdpDraft.value), 'CDP setting saved. Detection refreshed.');
  }

  async function resetCdp(): Promise<void> {
    cdpError.value = '';
    cdpDraft.value = DEFAULT_CDP_ENDPOINT;
    await perform(() => uiService.resetCdp(), 'CDP setting reset to auto.');
  }

  async function detectCdp(): Promise<void> {
    await perform(() => uiService.refreshCdp(), 'CDP detection refreshed.');
  }

  async function reconnect(): Promise<void> {
    await perform(() => uiService.reconnect(), 'Reconnecting...');
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
    cdpAvailable,
    daemonDraft,
    cdpDraft,
    daemonError,
    cdpError,
    notice,
    busy,
    daemonDefault: DEFAULT_DAEMON_URL,
    cdpDefault: DEFAULT_CDP_ENDPOINT,
    refresh,
    saveDaemon,
    resetDaemon,
    saveCdp,
    resetCdp,
    detectCdp,
    reconnect,
    openOptions,
  };
}
