/** Read-only Pinia projection of the provider service worker state. */

import { defineStore } from 'pinia';
import type { ProviderUiState } from '../../../shared/contracts/ui';

interface SessionState {
  snapshot: ProviderUiState | null;
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
    baseUrl: (state): string => state.snapshot?.base_url || 'ws://127.0.0.1:7330',
    browser: (state): ProviderUiState['browser'] => state.snapshot?.browser ?? null,
    tokenSaved: (state): boolean => state.snapshot?.token_saved === true,
    capabilities: (state): ProviderUiState['capabilities'] => state.snapshot?.capabilities || [],
    version: (state): string => state.snapshot?.version || '',
  },
  actions: {
    setSnapshot(snapshot: ProviderUiState): void {
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
