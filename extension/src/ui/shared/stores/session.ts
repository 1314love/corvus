/** Read-only Pinia projection of the background worker state. */

import { defineStore } from 'pinia';
import type { CdpUiState, UiState } from '../../../shared/contracts/ui';

interface SessionState {
  snapshot: UiState | null;
  loading: boolean;
  error: string | null;
}

export const useSessionStore = defineStore('session', {
  state: (): SessionState => ({
    snapshot: null,
    loading: false,
    error: null,
  }),
  getters: {
    status: (state): string => state.snapshot?.status || 'unknown',
    connected: (state): boolean => state.snapshot?.connected === true,
    daemonUrl: (state): string => state.snapshot?.wsUrl || 'ws://127.0.0.1:7330',
    browserId: (state): string | null => state.snapshot?.browserId ?? null,
    daemonVersion: (state): string | null => state.snapshot?.daemonVersion ?? null,
    cdp: (state): CdpUiState | null => state.snapshot?.cdp ?? null,
    cdpAvailable: (state): boolean => state.snapshot?.cdp?.available === true,
    capabilities: (state): string[] => state.snapshot?.capabilities || [],
    version: (state): string => state.snapshot?.version || '',
  },
  actions: {
    setSnapshot(snapshot: UiState): void {
      this.snapshot = snapshot;
      this.error = null;
    },
    setLoading(loading: boolean): void {
      this.loading = loading;
    },
    setError(error: string | null): void {
      this.error = error;
    },
  },
});
