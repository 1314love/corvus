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
import { localizedErrorText, policyDeniedText, type PolicyTranslate } from '../shared/policy-denied';
import { useSessionViewModel } from '../shared/viewmodels/session';

const { t, locale } = useI18n({ useScope: 'global' });
const {
  state,
  status,
  connected,
  cdpAvailable,
  loading,
  error,
  daemonDraft,
  cdpDraft,
  daemonError,
  cdpError,
  notice,
  busy,
  daemonDefault,
  cdpDefault,
  saveDaemon,
  resetDaemon,
  saveCdp,
  resetCdp,
  detectCdp,
  reconnect,
} = useSessionViewModel();

const cdp = computed(() => state.value?.cdp || null);
const capabilities = computed(() => state.value?.capabilities || []);
const statusLabel = computed(() => String(t('status.' + status.value)));
const statusType = computed<'success' | 'warning' | 'default'>(() => {
  if (connected.value) return 'success';
  return status.value === 'connecting' ? 'warning' : 'default';
});
const familyLabel = computed(() => {
  if (!cdp.value) return String(t('family.unknown'));
  if (cdp.value.familyMatch === null) return String(t('family.unknown'));
  return String(t(cdp.value.familyMatch ? 'family.match' : 'family.mismatch'));
});
const sourceLabel = computed(() => {
  const source = cdp.value?.source;
  if (!source) return String(t('source.unknown'));
  const key = 'source.' + source;
  const translated = String(t(key));
  return translated === key ? source : translated;
});
const versionLabel = computed(() => String(t('app.version', { version: state.value?.version || '' })));
const translate = t as unknown as PolicyTranslate;
const policyDeniedMessage = computed(() => policyDeniedText(state.value?.lastPolicyDenied, translate));
const errorText = computed(() => localizedErrorText(error.value, translate));

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
        <n-alert v-if="policyDeniedMessage" type="error" :show-icon="false">
          {{ policyDeniedMessage }}
        </n-alert>
        <n-alert v-if="errorText" type="error" :show-icon="false">{{ errorText }}</n-alert>
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
          <h2>{{ t('options.daemonConnection') }}</h2>
          <n-text depth="3">
            {{ t('options.daemonHelpBefore') }}
            <n-text code>{{ daemonDefault }}</n-text>
            {{ t('options.daemonHelpAfter') }}
          </n-text>
          <n-space vertical :size="8" class="form">
            <n-input
              v-model:value="daemonDraft"
              placeholder="ws://127.0.0.1:7330"
              :status="daemonError ? 'error' : undefined"
              @keyup.enter="saveDaemon"
            />
            <n-text v-if="daemonError" type="error">{{ t(daemonError) }}</n-text>
            <n-space>
              <n-button type="primary" :loading="busy" @click="saveDaemon">{{ t('common.save') }}</n-button>
              <n-button :disabled="busy" @click="resetDaemon">{{ t('common.resetToDefault') }}</n-button>
              <n-button :disabled="busy" @click="reconnect">{{ t('common.reconnect') }}</n-button>
            </n-space>
          </n-space>
        </section>

        <n-divider />

        <section>
          <h2>{{ t('options.browserCdpEndpoint') }}</h2>
          <n-text depth="3">
            {{ t('options.cdpHelpBefore') }}
            <n-text code>auto</n-text>
            {{ t('options.cdpHelpMid') }}
            <n-text code>/json/version</n-text>
            {{ t('options.cdpHelpAfter') }}
          </n-text>
          <n-space vertical :size="8" class="form">
            <n-input
              v-model:value="cdpDraft"
              :placeholder="cdpDefault"
              :status="cdpError ? 'error' : undefined"
              @keyup.enter="saveCdp"
            />
            <n-text v-if="cdpError" type="error">{{ t(cdpError) }}</n-text>
            <n-space>
              <n-button type="primary" :loading="busy" @click="saveCdp">{{ t('common.save') }}</n-button>
              <n-button :disabled="busy" @click="resetCdp">{{ t('common.resetToAuto') }}</n-button>
              <n-button :disabled="busy" :loading="busy" @click="detectCdp">{{ t('common.detectNow') }}</n-button>
            </n-space>
          </n-space>

          <n-descriptions v-if="cdp" class="detected" label-placement="left" :column="1" size="small" bordered>
            <n-descriptions-item :label="t('common.available')">
              <n-tag size="small" :type="cdpAvailable ? 'success' : 'default'">
                {{ cdpAvailable ? t('common.yes') : t('common.no') }}
              </n-tag>
            </n-descriptions-item>
            <n-descriptions-item :label="t('options.endpoint')">
              {{ cdp.endpoint || t('common.none') }}
            </n-descriptions-item>
            <n-descriptions-item :label="t('common.browser')">
              {{ cdp.browser || t('common.unknown') }}
            </n-descriptions-item>
            <n-descriptions-item :label="t('options.protocol')">
              {{ cdp.protocolVersion || t('common.unknown') }}
            </n-descriptions-item>
            <n-descriptions-item :label="t('options.source')">{{ sourceLabel }}</n-descriptions-item>
            <n-descriptions-item :label="t('options.browserFamily')">
              {{ cdp.browserFamily || t('common.unknown') }} / {{ t('common.expected') }}
              {{ cdp.expectedBrowserFamily || t('common.unknown') }} ({{ familyLabel }})
            </n-descriptions-item>
            <n-descriptions-item :label="t('options.reason')">
              {{ cdp.reason || t('common.none') }}
            </n-descriptions-item>
          </n-descriptions>
        </section>

        <n-divider />

        <section>
          <h2>{{ t('options.status') }}</h2>
          <n-descriptions label-placement="left" :column="1" size="small" bordered>
            <n-descriptions-item :label="t('options.daemonUrl')">
              <n-text code>{{ state?.wsUrl || daemonDefault }}</n-text>
            </n-descriptions-item>
            <n-descriptions-item :label="t('options.browserId')">
              {{ state?.browserId || t('common.notConnected') }}
            </n-descriptions-item>
            <n-descriptions-item :label="t('options.daemonVersion')">
              {{ state?.daemonVersion || t('common.unknown') }}
            </n-descriptions-item>
            <n-descriptions-item :label="t('options.capabilities')">
              {{ capabilities.length ? capabilities.join(', ') : t('common.none') }}
            </n-descriptions-item>
            <n-descriptions-item :label="t('options.lastError')">
              {{ state?.lastError || t('common.none') }}
            </n-descriptions-item>
          </n-descriptions>
        </section>

        <n-space justify="space-between">
          <n-text depth="3" style="font-size: 12px">{{ versionLabel }}</n-text>
          <n-button size="small" secondary :loading="loading" @click="detectCdp">
            {{ t('common.refreshStatus') }}
          </n-button>
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
.detected {
  margin-top: 12px;
}
</style>
