<script setup lang="ts">
import { watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { canAccessUser, session } from './auth';
import AccountMenu from './components/AccountMenu.vue';
import AccountIdentity from './components/AccountIdentity.vue';

const route = useRoute();
const router = useRouter();
watch(() => [session.authenticated, session.roles.join(',')], () => {
  if (!session.authenticated && route.meta.requiresAuth) {
    void router.replace('/');
  } else if (session.authenticated && route.meta.requiresAuth && !canAccessUser()) {
    void router.replace('/denied');
  }
});
</script>

<template>
  <div class="shell">
    <header class="topbar">
      <div class="brand">
        <img src="/logo.svg" width="56" height="56" alt="" />
        <div class="brand-copy">
          <p class="brand-title" title="Catalog Services">Catalog Services</p>
          <p class="brand-subtitle">Catalogue de services</p>
        </div>
      </div>
      <div v-if="session.authenticated" class="account-area">
        <AccountIdentity />
        <AccountMenu />
      </div>
    </header>
    <p v-if="session.error" class="alert error" role="alert">{{ session.error }}</p>
    <main id="main" :class="{ 'welcome-main': route.name === 'home' }"><RouterView /></main>
    <footer v-if="route.name !== 'home'">Vue + Rust · Identités et sessions gérées exclusivement par Keycloak · Usage local uniquement</footer>
  </div>
</template>
