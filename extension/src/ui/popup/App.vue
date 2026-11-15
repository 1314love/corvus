<script setup lang="ts">
import { computed } from 'vue';
import { NAlert, NButton, NCard, NDescriptions, NDescriptionsItem, NSpace, NTag, NText } from 'naive-ui';
import { useSessionViewModel } from '../shared/viewmodels/session';

const { state, status, connected, loading, error, refresh, openOptions } = useSessionViewModel();

const daemonUrl = computed(() => state.value?.wsUrl || 'ws://127.0.0.1:7330');
const browserLabel = computed(() => state.value?.browserId || 'not connected');
const cdpLabel = computed(() => {
  const cdp = state.value?.cdp;
  if (!cdp) return 'Not detected';
  return cdp.available ? cdp.browser || cdp.endpoint || 'Available' : cdp.reason || 'Unavailable';
});
const statusType = computed<'success' | 'warning' | 'default'>(() => {
  if (connected.value) return 'success';
  return status.value === 'connecting' ? 'warning' : 'default';
});
</script>

<template>
  <n-card size="small" title="Corvus">
    <template #header-extra>
      <n-tag size="small" :type="statusType">{{ status }}</n-tag>
    </template>

    <n-space vertical :size="10">
      <n-descriptions label-placement="left" :column="1" size="small" bordered>
        <n-descriptions-item label="Daemon">
          <n-text code>{{ daemonUrl }}</n-text>
        </n-descriptions-item>
        <n-descriptions-item label="Browser">{{ browserLabel }}</n-descriptions-item>
        <n-descriptions-item label="CDP">{{ cdpLabel }}</n-descriptions-item>
      </n-descriptions>

      <n-alert v-if="error || state?.lastError" type="warning" :show-icon="false">
        {{ error || state?.lastError }}
      </n-alert>

      <n-space justify="space-between">
        <n-button size="small" @click="openOptions">Settings</n-button>
        <n-button size="small" secondary :loading="loading" @click="refresh">Refresh</n-button>
      </n-space>

      <n-text depth="3" style="font-size: 11px">Corvus {{ state?.version || '' }}</n-text>
    </n-space>
  </n-card>
</template>
