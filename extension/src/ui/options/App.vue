<script setup lang="ts">
import { computed } from 'vue';
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
import { useSessionViewModel } from '../shared/viewmodels/session';

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
const statusType = computed<'success' | 'warning' | 'default'>(() => {
  if (connected.value) return 'success';
  return status.value === 'connecting' ? 'warning' : 'default';
});
const familyLabel = computed(() => {
  if (!cdp.value) return 'not detected';
  if (cdp.value.familyMatch === null) return 'unknown';
  return cdp.value.familyMatch ? 'match' : 'mismatch';
});
</script>

<template>
  <div class="page">
    <n-card title="Corvus settings">
      <template #header-extra>
        <n-tag :type="statusType">{{ status }}</n-tag>
      </template>

      <n-space vertical :size="16">
        <n-alert v-if="error" type="error" :show-icon="false">{{ error }}</n-alert>
        <n-alert v-if="notice" type="success" :show-icon="false">{{ notice }}</n-alert>

        <section>
          <h2>Daemon connection</h2>
          <n-text depth="3">
            The offscreen document connects to this WebSocket URL. The local default is
            <n-text code>{{ daemonDefault }}</n-text>.
          </n-text>
          <n-space vertical :size="8" class="form">
            <n-input
              v-model:value="daemonDraft"
              placeholder="ws://127.0.0.1:7330"
              :status="daemonError ? 'error' : undefined"
              @keyup.enter="saveDaemon"
            />
            <n-text v-if="daemonError" type="error">{{ daemonError }}</n-text>
            <n-space>
              <n-button type="primary" :loading="busy" @click="saveDaemon">Save</n-button>
              <n-button :disabled="busy" @click="resetDaemon">Reset to default</n-button>
              <n-button :disabled="busy" @click="reconnect">Reconnect</n-button>
            </n-space>
          </n-space>
        </section>

        <n-divider />

        <section>
          <h2>Browser CDP endpoint</h2>
          <n-text depth="3">
            Leave empty or use <n-text code>auto</n-text> to probe the local ports 9222, 9223 and 9333. Detection only
            reads <n-text code>/json/version</n-text>; it never attaches the debugger.
          </n-text>
          <n-space vertical :size="8" class="form">
            <n-input
              v-model:value="cdpDraft"
              :placeholder="cdpDefault"
              :status="cdpError ? 'error' : undefined"
              @keyup.enter="saveCdp"
            />
            <n-text v-if="cdpError" type="error">{{ cdpError }}</n-text>
            <n-space>
              <n-button type="primary" :loading="busy" @click="saveCdp">Save</n-button>
              <n-button :disabled="busy" @click="resetCdp">Reset to auto</n-button>
              <n-button :disabled="busy" :loading="busy" @click="detectCdp">Detect now</n-button>
            </n-space>
          </n-space>

          <n-descriptions v-if="cdp" class="detected" label-placement="left" :column="1" size="small" bordered>
            <n-descriptions-item label="Available">
              <n-tag size="small" :type="cdpAvailable ? 'success' : 'default'">
                {{ cdpAvailable ? 'yes' : 'no' }}
              </n-tag>
            </n-descriptions-item>
            <n-descriptions-item label="Endpoint">{{ cdp.endpoint || 'none' }}</n-descriptions-item>
            <n-descriptions-item label="Browser">{{ cdp.browser || 'unknown' }}</n-descriptions-item>
            <n-descriptions-item label="Protocol">
              {{ cdp.protocolVersion || 'unknown' }}
            </n-descriptions-item>
            <n-descriptions-item label="Source">{{ cdp.source }}</n-descriptions-item>
            <n-descriptions-item label="Browser family">
              {{ cdp.browserFamily || 'unknown' }} / expected {{ cdp.expectedBrowserFamily || 'unknown' }} ({{
                familyLabel
              }})
            </n-descriptions-item>
            <n-descriptions-item label="Reason">{{ cdp.reason || 'none' }}</n-descriptions-item>
          </n-descriptions>
        </section>

        <n-divider />

        <section>
          <h2>Status</h2>
          <n-descriptions label-placement="left" :column="1" size="small" bordered>
            <n-descriptions-item label="Daemon URL">
              <n-text code>{{ state?.wsUrl || daemonDefault }}</n-text>
            </n-descriptions-item>
            <n-descriptions-item label="Browser id">
              {{ state?.browserId || 'not connected' }}
            </n-descriptions-item>
            <n-descriptions-item label="Daemon version">
              {{ state?.daemonVersion || 'unknown' }}
            </n-descriptions-item>
            <n-descriptions-item label="Capabilities">
              {{ capabilities.length ? capabilities.join(', ') : 'none' }}
            </n-descriptions-item>
            <n-descriptions-item label="Last error">
              {{ state?.lastError || 'none' }}
            </n-descriptions-item>
          </n-descriptions>
        </section>

        <n-space justify="space-between">
          <n-text depth="3" style="font-size: 12px">Corvus {{ state?.version || '' }}</n-text>
          <n-button size="small" secondary :loading="loading" @click="detectCdp">Refresh status</n-button>
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
