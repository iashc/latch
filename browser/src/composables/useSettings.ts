import { ref } from "vue";
import { browser } from "wxt/browser";

export const DEFAULT_SERVER_URL = "http://127.0.0.1:52525";

const SERVER_URL_KEY = "serverUrl";

export interface LatchSettings {
  serverUrl: string;
}

export function normalizeServerUrl(value: string) {
  const trimmed = value.trim();
  return (trimmed || DEFAULT_SERVER_URL).replace(/\/+$/, "");
}

export function useSettings() {
  const settings = ref<LatchSettings>({ serverUrl: DEFAULT_SERVER_URL });
  const isLoading = ref(false);

  async function loadSettings() {
    isLoading.value = true;
    try {
      const stored = await browser.storage.local.get(SERVER_URL_KEY);
      settings.value = {
        serverUrl: normalizeServerUrl(String(stored[SERVER_URL_KEY] || DEFAULT_SERVER_URL))
      };
    } finally {
      isLoading.value = false;
    }
  }

  async function saveSettings(nextSettings: LatchSettings) {
    const serverUrl = normalizeServerUrl(nextSettings.serverUrl);
    await browser.storage.local.set({ [SERVER_URL_KEY]: serverUrl });
    settings.value = { serverUrl };
  }

  return {
    settings,
    isLoading,
    loadSettings,
    saveSettings
  };
}
