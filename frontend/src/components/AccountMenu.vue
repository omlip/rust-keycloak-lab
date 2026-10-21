<script setup lang="ts">
import { onMounted, onUnmounted, ref, watch } from 'vue';
import { useRoute } from 'vue-router';
import { logout } from '../auth';

const route = useRoute();
const open = ref(false);
const container = ref<HTMLElement | null>(null);
const toggle = ref<HTMLButtonElement | null>(null);

function close() {
  open.value = false;
}

function closeAndFocus() {
  close();
  toggle.value?.focus();
}

function onOutsidePointerDown(event: PointerEvent) {
  if (event.target instanceof Node && !container.value?.contains(event.target)) close();
}

function onFocusOut(event: FocusEvent) {
  if (!(event.relatedTarget instanceof Node) || !container.value?.contains(event.relatedTarget)) close();
}

function signOut() {
  close();
  void logout();
}

watch(() => route.fullPath, close);
onMounted(() => document.addEventListener('pointerdown', onOutsidePointerDown));
onUnmounted(() => document.removeEventListener('pointerdown', onOutsidePointerDown));
</script>

<template>
  <div ref="container" class="account-menu" @keydown.esc.prevent="closeAndFocus" @focusout="onFocusOut">
    <button
      ref="toggle"
      type="button"
      class="secondary account-menu-toggle"
      aria-label="Menu du compte"
      aria-controls="account-menu-panel"
      :aria-expanded="open"
      @click="open = !open"
    >
      <svg aria-hidden="true" viewBox="0 0 24 24" width="24" height="24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
        <path d="M4 6h16M4 12h16M4 18h16" />
      </svg>
    </button>
    <div v-if="open" id="account-menu-panel" class="account-menu-panel">
      <RouterLink to="/settings" @click="close">Paramètres</RouterLink>
      <button type="button" @click="signOut">Déconnexion</button>
    </div>
  </div>
</template>
