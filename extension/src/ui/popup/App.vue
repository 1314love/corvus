<script setup lang="ts">
import { computed, watchEffect } from 'vue';
import { useI18n } from 'vue-i18n';
import { NAlert, NButton, NCard, NDescriptions, NDescriptionsItem, NSpace, NTag, NText } from 'naive-ui';
import { DEFAULT_DAEMON_URL } from '../../shared/storage/daemon-url';
import { useSessionViewModel } from '../shared/viewmodels/session';

const { t } = useI18n({ useScope: 'global' });
const { state, status, connected, loading, error, refresh, openOptions } = useSessionViewModel();

const statusLabel = computed(() => String(t('status.' + status.value)));
const daemonUrl = computed(() => state.value?.wsUrl || DEFAULT_DAEMON_URL);
const browserLabel = computed(() => state.value?.browserId || String(t('common.notConnected')));
const cdpLabel = computed(() => {
  const cdp = state.value?.cdp;
  if (!cdp) return String(t('common.notDetected'));
  return cdp.available
    ? cdp.browser || cdp.endpoint || String(t('common.available'))
    : cdp.reason || String(t('common.unavailable'));
});
const versionLabel = computed(() => String(t('app.version', { version: state.value?.version || '' })));
const statusType = computed<'success' | 'warning' | 'default'>(() => {
  if (connected.value) return 'success';
  return status.value === 'connecting' ? 'warning' : 'default';
});

watchEffect(() => {
  document.title = String(t('app.popupTitle'));
});
</script>

<template>
  <n-card size="small" :title="t('app.title')">
    <template #header-extra>
      <n-tag size="small" :type="statusType">{{ statusLabel }}</n-tag>
    </template>

    <n-space vertical :size="10">
      <n-descriptions label-placement="left" :column="1" size="small" bordered>
        <n-descriptions-item :label="t('common.daemon')">
          <n-text code>{{ daemonUrl }}</n-text>
        </n-descriptions-item>
        <n-descriptions-item :label="t('common.browser')">{{ browserLabel }}</n-descriptions-item>
        <n-descriptions-item :label="t('common.cdp')">{{ cdpLabel }}</n-descriptions-item>
      </n-descriptions>

      <n-alert v-if="error || state?.lastError" type="warning" :show-icon="false">
        {{ error || state?.lastError }}
      </n-alert>

      <n-space justify="space-between">
        <n-button size="small" @click="openOptions">{{ t('common.settings') }}</n-button>
        <n-button size="small" secondary :loading="loading" @click="refresh">{{ t('common.refresh') }}</n-button>
      </n-space>

      <n-text depth="3" style="font-size: 11px">{{ versionLabel }}</n-text>
    </n-space>
  </n-card>
</template>
