<script setup lang="ts">
import { computed, watchEffect } from 'vue';
import { useI18n } from 'vue-i18n';
import {
  NAlert,
  NButton,
  NCard,
  NDescriptions,
  NDescriptionsItem,
  NDivider,
  NInput,
  NSpace,
  NTag,
  NText,
} from 'naive-ui';
import { changeLocale, type LocaleCode } from '../shared/i18n';
import { useSessionViewModel } from '../shared/viewmodels/session';

const { t, locale } = useI18n({ useScope: 'global' });
const {
  state,
  status,
  connected,
  loading,
  error,
  tokenSaved,
  browser,
  baseDraft,
  tokenDraft,
  baseError,
  notice,
  busy,
  baseDefault,
  refresh,
  saveAndConnect,
  connect,
  disconnect,
} = useSessionViewModel();

const statusLabel = computed(() => {
  const key = 'status.' + status.value;
  const translated = String(t(key));
  return translated === key ? status.value : translated;
});
const statusType = computed<'success' | 'warning' | 'error' | 'default'>(() => {
  if (connected.value) return 'success';
  if (status.value === 'connecting') return 'warning';
  return status.value === 'error' ? 'error' : 'default';
});
const browserLabel = computed(() => {
  const info = browser.value;
  if (!info) return String(t('common.notConnected'));
  return info.family + '/' + info.version;
});
const baseErrorText = computed(() => {
  if (!baseError.value) return '';
  const key = baseError.value;
  const translated = String(t(key));
  return translated === key ? baseError.value : translated;
});
const versionLabel = computed(() => String(t('app.version', { version: state.value?.version || '' })));

async function selectLocale(next: LocaleCode): Promise<void> {
  await changeLocale(next);
}

watchEffect(() => {
  document.title = String(t('app.optionsTitle'));
});
</script>

<template>
  <div class="page">
    <n-card :title="t('app.optionsTitle')">
      <template #header-extra>
        <n-tag :type="statusType">{{ statusLabel }}</n-tag>
      </template>

      <n-space vertical :size="16">
        <n-alert v-if="error" type="error" :show-icon="false">{{ error }}</n-alert>
        <n-alert v-if="notice" type="success" :show-icon="false">{{ t(notice) }}</n-alert>

        <section>
          <h2>{{ t('common.language') }}</h2>
          <n-space>
            <n-button
              size="small"
              :type="locale === 'en' ? 'primary' : 'default'"
              data-testid="locale-en"
              @click="selectLocale('en')"
            >
              {{ t('common.english') }}
            </n-button>
            <n-button
              size="small"
              :type="locale === 'zh-CN' ? 'primary' : 'default'"
              data-testid="locale-zh-CN"
              @click="selectLocale('zh-CN')"
            >
              {{ t('common.chinese') }}
            </n-button>
          </n-space>
        </section>

        <n-divider />

        <section>
          <h2>{{ t('options.pairing') }}</h2>
          <n-text depth="3"
            >{{ t('options.pairingHelp') }} <n-text code>{{ baseDefault }}</n-text></n-text
          >
          <n-space vertical :size="8" class="form">
            <n-input
              v-model:value="baseDraft"
              :placeholder="baseDefault"
              :status="baseError ? 'error' : undefined"
              @keyup.enter="saveAndConnect"
            />
            <n-input
              v-model:value="tokenDraft"
              type="password"
              show-password-on="click"
              :placeholder="tokenSaved ? t('options.tokenSaved') : t('options.tokenPlaceholder')"
              :status="baseError === 'validation.token' ? 'error' : undefined"
              autocomplete="off"
              @keyup.enter="saveAndConnect"
            />
            <n-text v-if="baseErrorText" type="error">{{ baseErrorText }}</n-text>
            <n-space>
              <n-button type="primary" :loading="busy" @click="saveAndConnect">{{
                t('common.saveAndConnect')
              }}</n-button>
              <n-button :disabled="busy" @click="connect">{{ t('common.connect') }}</n-button>
              <n-button :disabled="busy" @click="disconnect">{{ t('common.disconnect') }}</n-button>
            </n-space>
          </n-space>
        </section>

        <n-divider />

        <section>
          <h2>{{ t('options.status') }}</h2>
          <n-descriptions label-placement="left" :column="1" size="small" bordered>
            <n-descriptions-item :label="t('options.baseUrl')">
              <n-text code>{{ state?.base_url || baseDefault }}</n-text>
            </n-descriptions-item>
            <n-descriptions-item :label="t('options.browser')">{{ browserLabel }}</n-descriptions-item>
            <n-descriptions-item :label="t('options.token')">
              {{ tokenSaved ? t('options.tokenPresent') : t('options.tokenMissing') }}
            </n-descriptions-item>
            <n-descriptions-item :label="t('options.service')">
              {{ state?.provider_id || t('common.notConnected') }}
            </n-descriptions-item>
            <n-descriptions-item :label="t('options.lastError')">
              {{ state?.last_error ? state.last_error.message : t('common.none') }}
            </n-descriptions-item>
          </n-descriptions>
        </section>

        <n-space justify="space-between">
          <n-text depth="3" style="font-size: 12px">{{ versionLabel }}</n-text>
          <n-button size="small" secondary :loading="loading" @click="refresh">{{
            t('common.refreshStatus')
          }}</n-button>
        </n-space>
      </n-space>
    </n-card>
  </div>
</template>

<style scoped>
.page {
  max-width: 720px;
  margin: 0 auto;
  padding: 24px 16px;
}
h2 {
  margin: 0 0 6px;
  font-size: 15px;
  font-weight: 600;
}
.form {
  margin-top: 10px;
}
</style>
