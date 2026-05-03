<script setup lang="ts">
interface Props {
  status: "checking" | "online" | "offline";
}

defineProps<Props>();
</script>

<template>
  <span class="connection-status" :class="`connection-status--${status}`">
    {{ status === "online" ? "已连接" : status === "checking" ? "检查中" : "未连接" }}
  </span>
</template>
