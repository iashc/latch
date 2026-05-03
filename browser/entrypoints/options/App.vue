<script setup lang="ts">
import { computed, onMounted, ref } from "vue";

import ConnectionStatus from "@/src/components/ConnectionStatus.vue";
import { createLatchApi } from "@/src/composables/useLatchApi";
import { DEFAULT_SERVER_URL, normalizeServerUrl, useSettings } from "@/src/composables/useSettings";
import { getErrorMessage } from "@/src/lib/errors";

const { settings, isLoading, loadSettings, saveSettings } = useSettings();

const serverUrlInput = ref(DEFAULT_SERVER_URL);
const status = ref<"checking" | "online" | "offline">("checking");
const message = ref("");
const isSaving = ref(false);
const isTesting = ref(false);

const canSubmit = computed(() => !isLoading.value && !isSaving.value);

async function testConnection(serverUrl = serverUrlInput.value) {
  status.value = "checking";
  message.value = "";
  isTesting.value = true;
  try {
    await createLatchApi(serverUrl).health();
    status.value = "online";
    message.value = "连接正常";
  } catch (error) {
    status.value = "offline";
    message.value = getErrorMessage(error);
  } finally {
    isTesting.value = false;
  }
}

async function handleSave() {
  if (!canSubmit.value) {
    return;
  }

  isSaving.value = true;
  try {
    const serverUrl = normalizeServerUrl(serverUrlInput.value);
    await saveSettings({ serverUrl });
    serverUrlInput.value = serverUrl;
    await testConnection(serverUrl);
  } finally {
    isSaving.value = false;
  }
}

onMounted(async () => {
  await loadSettings();
  serverUrlInput.value = settings.value.serverUrl;
  await testConnection(settings.value.serverUrl);
});
</script>

<template>
  <main class="options-shell">
    <header class="page-header">
      <div>
        <p class="eyebrow">Latch</p>
        <h1>设置</h1>
      </div>
      <ConnectionStatus :status="status" />
    </header>

    <form class="settings-panel" @submit.prevent="handleSave">
      <label class="field">
        <span>服务地址</span>
        <input v-model="serverUrlInput" type="url" spellcheck="false" />
      </label>

      <div class="button-row">
        <button class="primary-button" type="submit" :disabled="!canSubmit">
          {{ isSaving ? "保存中" : "保存" }}
        </button>
        <button type="button" :disabled="isTesting" @click="testConnection()">
          {{ isTesting ? "检查中" : "测试连接" }}
        </button>
      </div>

      <p v-if="message" class="message" :class="{ 'message--error': status === 'offline' }">
        {{ message }}
      </p>
    </form>
  </main>
</template>
