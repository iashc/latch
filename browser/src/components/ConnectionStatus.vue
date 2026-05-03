<script setup lang="ts">
interface Props {
  status: "checking" | "online" | "offline";
}

defineProps<Props>();
</script>

<template>
  <span class="connection-status" :class="`connection-status--${status}`">
    {{ status === "online" ? "Connected" : status === "checking" ? "Checking" : "Disconnected" }}
  </span>
</template>
