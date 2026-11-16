<script setup lang="ts">
import { computed, watchEffect } from 'vue';
import { useI18n } from 'vue-i18n';
import { NAlert, NButton, NCard, NDescriptions, NDescriptionsItem, NSpace, NTag, NText } from 'naive-ui';
import { useSessionViewModel } from '../shared/viewmodels/session';

const { t } = useI18n({ useScope: 'global' });
const { state, status, connected, loading, error, browser, refresh, openOptions } = useSessionViewModel();

const statusLabel = computed(() => {
  const key = 'status.' + status.value;
  const translated = String(t(key));
  return translated === key ? status.value : translated;
});
const baseUrl = computed(() => state.value?.base_url || 'ws://127.0.0.1:7330');
const browserLabel = computed(() => {
  const info = browser.value;
  if (!info) return String(t('common.notConnected'));
  return info.family + '/' + info.version;
});
const errorText = computed(() => {
  if (error.value) return error.value;
  return state.value?.last_error ? state.value.last_error.message : '';
});
const versionLabel = computed(() => String(t('app.version', { version: state.value?.version || '' })));
const statusType = computed<'success' | 'warning' | 'error' | 'default'>(() => {
  if (connected.value) return 'success';
  if (status.value === 'connecting') return 'warning';
  return status.value === 'error' ? 'error' : 'default';
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
        <n-descriptions-item :label="t('options.baseUrl')">
          <n-text code>{{ baseUrl }}</n-text>
        </n-descriptions-item>
        <n-descriptions-item :label="t('options.browser')">{{ browserLabel }}</n-descriptions-item>
        <n-descriptions-item :label="t('options.token')">
          {{ state?.token_saved ? t('options.tokenPresent') : t('options.tokenMissing') }}
        </n-descriptions-item>
      </n-descriptions>

      <n-alert v-if="errorText" type="warning" :show-icon="false">{{ errorText }}</n-alert>

      <n-space justify="space-between">
        <n-button size="small" @click="openOptions">{{ t('common.settings') }}</n-button>
        <n-button size="small" secondary :loading="loading" @click="refresh">{{ t('common.refresh') }}</n-button>
      </n-space>

      <n-text depth="3" style="font-size: 11px">{{ versionLabel }}</n-text>
    </n-space>
  </n-card>
</template>
