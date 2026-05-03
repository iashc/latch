<script setup lang="ts">
import type { Bookmark } from "../../../shared/types";

interface Props {
  bookmarks: Bookmark[];
  isLoading: boolean;
}

defineProps<Props>();

const emit = defineEmits<{
  open: [bookmark: Bookmark];
  copy: [bookmark: Bookmark];
  delete: [bookmark: Bookmark];
}>();

function displayTitle(bookmark: Bookmark) {
  return bookmark.title || bookmark.url;
}

function hostname(bookmark: Bookmark) {
  try {
    return new URL(bookmark.url).hostname;
  } catch {
    return bookmark.url;
  }
}
</script>

<template>
  <div class="bookmark-list">
    <div v-if="isLoading" class="empty-state">Loading</div>
    <div v-else-if="bookmarks.length === 0" class="empty-state">No matching bookmarks</div>
    <template v-else>
      <article v-for="bookmark in bookmarks" :key="bookmark.id" class="bookmark-item">
        <div class="bookmark-main">
          <h3>{{ displayTitle(bookmark) }}</h3>
          <p>{{ hostname(bookmark) }}</p>
        </div>
        <div v-if="bookmark.tags.length > 0" class="tag-row">
          <span v-for="tag in bookmark.tags" :key="tag" class="tag">{{ tag }}</span>
        </div>
        <div class="item-actions">
          <button type="button" @click="emit('open', bookmark)">Open</button>
          <button type="button" @click="emit('copy', bookmark)">Copy</button>
          <button type="button" class="danger-button" @click="emit('delete', bookmark)">Delete</button>
        </div>
      </article>
    </template>
  </div>
</template>
