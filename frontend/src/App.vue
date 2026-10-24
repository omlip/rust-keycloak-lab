<script setup lang="ts">
import { watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { canAccessUser, hasRole, session } from './auth';
import AccountMenu from './components/AccountMenu.vue';

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
    <header v-if="session.authenticated" class="topbar" :class="hasRole('admin') ? 'admin-space' : 'user-space'">
      <nav class="secure-navigation" aria-label="Navigation principale">
        <div v-if="canAccessUser()" class="primary-links">
          <RouterLink to="/catalog" class="catalog-link"><img src="/logo.svg" width="36" height="36" alt="" />Catalogue de services</RouterLink>
          <RouterLink to="/selections">Mes sélections</RouterLink>
        </div>
        <AccountMenu />
      </nav>
    </header>
    <p v-if="session.error" class="alert error" role="alert">{{ session.error }}</p>
    <main id="main" :class="{ 'welcome-main': route.name === 'home' }"><RouterView /></main>
    <footer v-if="route.name !== 'home'">Vue + Rust · Identités et sessions gérées exclusivement par Keycloak · Usage local uniquement</footer>
  </div>
</template>
