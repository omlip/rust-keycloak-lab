<script setup lang="ts">
import { computed } from 'vue';
import { session } from '../auth';

const name = computed(() => session.username || session.name || 'Utilisateur');
const initials = computed(() => {
  const words = name.value.trim().split(/\s+/u)
    .map(word => word.match(/[\p{L}\p{N}]/gu) ?? [])
    .filter(word => word.length);
  const letters = words.length > 1
    ? `${words[0][0]}${words[words.length - 1][0]}`
    : (words[0] ?? []).slice(0, 2).join('');
  return Array.from(letters.toUpperCase()).slice(0, 2).join('') || '?';
});
</script>

<template>
  <div class="account-identity" role="group" :aria-label="`Connecté en tant que ${name}`" :title="name">
    <span class="account-avatar" aria-hidden="true">{{ initials }}</span>
    <span class="identity-name" aria-hidden="true">{{ name }}</span>
  </div>
</template>
