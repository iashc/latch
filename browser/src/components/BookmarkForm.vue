<script setup lang="ts">
import { computed, ref, watch } from "vue";

import { formatTags, parseTags } from "@/src/lib/tags";

interface BookmarkFormValue {
  title: string;
  description: string;
  tags: string[];
}

interface Props {
  initialTitle: string;
  initialDescription?: string;
  initialTags?: string[];
  submitLabel: string;
  isSubmitting?: boolean;
}

const props = withDefaults(defineProps<Props>(), {
  initialDescription: "",
  initialTags: () => [],
  isSubmitting: false
});

const emit = defineEmits<{
  submit: [value: BookmarkFormValue];
}>();

const title = ref(props.initialTitle);
const description = ref(props.initialDescription);
const tagsText = ref(formatTags(props.initialTags));

const canSubmit = computed(() => !props.isSubmitting);

watch(
  () => [props.initialTitle, props.initialDescription, props.initialTags] as const,
  ([nextTitle, nextDescription, nextTags]) => {
    title.value = nextTitle;
    description.value = nextDescription || "";
    tagsText.value = formatTags(nextTags || []);
  }
);

function handleSubmit() {
  if (!canSubmit.value) {
    return;
  }

  emit("submit", {
    title: title.value.trim(),
    description: description.value.trim(),
    tags: parseTags(tagsText.value)
  });
}
</script>

<template>
  <form class="bookmark-form" @submit.prevent="handleSubmit">
    <label class="field">
      <span>标题</span>
      <input v-model="title" type="text" autocomplete="off" />
    </label>
    <label class="field">
      <span>描述</span>
      <textarea v-model="description" rows="3" />
    </label>
    <label class="field">
      <span>标签</span>
      <input v-model="tagsText" type="text" autocomplete="off" placeholder="rust, docs" />
    </label>
    <button class="primary-button" type="submit" :disabled="!canSubmit">
      {{ isSubmitting ? "保存中" : submitLabel }}
    </button>
  </form>
</template>
