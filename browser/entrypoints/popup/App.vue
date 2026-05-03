<script setup lang="ts">
import type { Bookmark } from "../../../shared/types";

import { computed, onMounted, ref, watch } from "vue";
import { browser } from "wxt/browser";

import BookmarkForm from "@/src/components/BookmarkForm.vue";
import BookmarkList from "@/src/components/BookmarkList.vue";
import ConnectionStatus from "@/src/components/ConnectionStatus.vue";
import { useCurrentTab } from "@/src/composables/useCurrentTab";
import { createLatchApi, type LatchApi } from "@/src/composables/useLatchApi";
import { useSettings } from "@/src/composables/useSettings";
import { getErrorMessage } from "@/src/lib/errors";

type ViewMode = "current" | "search";

interface BookmarkFormValue {
  title: string;
  description: string;
  tags: string[];
}

const { settings, loadSettings } = useSettings();
const { currentTab, canSaveCurrentTab, isLoading: isTabLoading, loadCurrentTab } = useCurrentTab();

const mode = ref<ViewMode>("current");
const connectionStatus = ref<"checking" | "online" | "offline">("checking");
const currentBookmark = ref<Bookmark | null>(null);
const searchResults = ref<Bookmark[]>([]);
const searchQuery = ref("");
const message = ref("");
const isBooting = ref(true);
const isSubmitting = ref(false);
const isSearching = ref(false);

const api = computed<LatchApi>(() => createLatchApi(settings.value.serverUrl));

const initialTitle = computed(() => currentBookmark.value?.title || currentTab.value?.title || "");
const initialDescription = computed(() => currentBookmark.value?.description || "");
const initialTags = computed(() => currentBookmark.value?.tags || []);
const submitLabel = computed(() => (currentBookmark.value ? "更新书签" : "保存书签"));
const currentUrl = computed(() => currentTab.value?.url || "");

async function bootstrap() {
  isBooting.value = true;
  message.value = "";
  await loadSettings();
  await loadCurrentTab();
  await refreshConnection();
  await refreshCurrentBookmark();
  await searchBookmarks();
  isBooting.value = false;
}

async function refreshConnection() {
  connectionStatus.value = "checking";
  try {
    await api.value.health();
    connectionStatus.value = "online";
  } catch (error) {
    connectionStatus.value = "offline";
    message.value = getErrorMessage(error);
  }
}

async function refreshCurrentBookmark() {
  currentBookmark.value = null;
  if (!canSaveCurrentTab.value || connectionStatus.value !== "online") {
    return;
  }

  try {
    const response = await api.value.listBookmarks({ url: currentUrl.value, limit: 1 });
    currentBookmark.value = response.data[0] ?? null;
  } catch (error) {
    message.value = getErrorMessage(error);
  }
}

async function handleSubmit(value: BookmarkFormValue) {
  if (!canSaveCurrentTab.value) {
    message.value = "当前页面不能保存";
    return;
  }

  isSubmitting.value = true;
  message.value = "";
  try {
    currentBookmark.value = currentBookmark.value
      ? await api.value.updateBookmark(currentBookmark.value.id, value)
      : await api.value.createBookmark({
          url: currentUrl.value,
          ...value
        });
    message.value = currentBookmark.value ? "已保存" : "";
    await searchBookmarks();
  } catch (error) {
    message.value = getErrorMessage(error);
  } finally {
    isSubmitting.value = false;
  }
}

async function deleteBookmark(bookmark: Bookmark) {
  const confirmed = window.confirm(`删除「${bookmark.title || bookmark.url}」？`);
  if (!confirmed) {
    return;
  }

  try {
    await api.value.deleteBookmark(bookmark.id);
    if (currentBookmark.value?.id === bookmark.id) {
      currentBookmark.value = null;
    }
    await searchBookmarks();
    message.value = "已删除";
  } catch (error) {
    message.value = getErrorMessage(error);
  }
}

async function searchBookmarks() {
  if (connectionStatus.value !== "online") {
    searchResults.value = [];
    return;
  }

  isSearching.value = true;
  try {
    const response = await api.value.listBookmarks({
      q: searchQuery.value.trim() || undefined,
      limit: 50
    });
    searchResults.value = response.data;
  } catch (error) {
    searchResults.value = [];
    message.value = getErrorMessage(error);
  } finally {
    isSearching.value = false;
  }
}

async function openBookmark(bookmark: Bookmark) {
  try {
    await api.value.recordBookmarkOpen(bookmark.id);
  } catch {
    // Opening the page is more important than updating frecency.
  }
  await browser.tabs.create({ url: bookmark.url });
  await searchBookmarks();
}

async function copyBookmark(bookmark: Bookmark) {
  try {
    await navigator.clipboard.writeText(bookmark.url);
    message.value = "已复制";
  } catch (error) {
    message.value = getErrorMessage(error);
  }
}

let searchTimer: number | undefined;
watch(searchQuery, () => {
  window.clearTimeout(searchTimer);
  searchTimer = window.setTimeout(() => {
    void searchBookmarks();
  }, 180);
});

onMounted(() => {
  void bootstrap();
});
</script>

<template>
  <main class="popup-shell">
    <header class="page-header">
      <div>
        <p class="eyebrow">Latch</p>
        <h1>书签</h1>
      </div>
      <ConnectionStatus :status="connectionStatus" />
    </header>

    <nav class="segmented-control">
      <button
        type="button"
        :class="{ active: mode === 'current' }"
        @click="mode = 'current'"
      >
        当前页
      </button>
      <button type="button" :class="{ active: mode === 'search' }" @click="mode = 'search'">
        搜索
      </button>
    </nav>

    <p v-if="message" class="message" :class="{ 'message--error': connectionStatus === 'offline' }">
      {{ message }}
    </p>

    <section v-if="mode === 'current'" class="view-stack">
      <div v-if="isBooting || isTabLoading" class="empty-state">加载中</div>
      <div v-else-if="!canSaveCurrentTab" class="empty-state">当前页面不能保存</div>
      <template v-else>
        <div class="current-url">
          <strong>{{ currentTab?.title || currentUrl }}</strong>
          <span>{{ currentUrl }}</span>
        </div>
        <BookmarkForm
          :initial-title="initialTitle"
          :initial-description="initialDescription"
          :initial-tags="initialTags"
          :submit-label="submitLabel"
          :is-submitting="isSubmitting"
          @submit="handleSubmit"
        />
        <button
          v-if="currentBookmark"
          type="button"
          class="danger-button full-width"
          @click="deleteBookmark(currentBookmark)"
        >
          删除书签
        </button>
      </template>
    </section>

    <section v-else class="view-stack">
      <label class="field">
        <span>搜索</span>
        <input v-model="searchQuery" type="search" autocomplete="off" />
      </label>
      <BookmarkList
        :bookmarks="searchResults"
        :is-loading="isSearching"
        @open="openBookmark"
        @copy="copyBookmark"
        @delete="deleteBookmark"
      />
    </section>
  </main>
</template>
