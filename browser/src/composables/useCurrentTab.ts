import { computed, ref } from "vue";
import { browser } from "wxt/browser";

export interface CurrentTab {
  id?: number;
  title: string;
  url: string;
}

export function useCurrentTab() {
  const currentTab = ref<CurrentTab | null>(null);
  const isLoading = ref(false);

  const canSaveCurrentTab = computed(() => {
    const url = currentTab.value?.url;
    return Boolean(url && /^https?:\/\//i.test(url));
  });

  async function loadCurrentTab() {
    isLoading.value = true;
    try {
      const [tab] = await browser.tabs.query({ active: true, currentWindow: true });
      currentTab.value = tab
        ? {
            id: tab.id,
            title: tab.title || "",
            url: tab.url || ""
          }
        : null;
    } finally {
      isLoading.value = false;
    }
  }

  return {
    currentTab,
    canSaveCurrentTab,
    isLoading,
    loadCurrentTab
  };
}
