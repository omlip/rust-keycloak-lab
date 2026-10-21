<script setup lang="ts">
import { login, rememberDestination, session } from '../auth';

function connectToCatalog() {
  rememberDestination('/catalog');
  return login();
}
</script>

<template>
  <section class="hero" aria-labelledby="welcome-title">
    <div class="welcome-icon" aria-hidden="true">
      <svg viewBox="0 0 48 48" width="48" height="48" fill="none" stroke="currentColor" stroke-width="2">
        <rect x="8" y="7" width="32" height="14" rx="3" />
        <rect x="8" y="27" width="32" height="14" rx="3" />
        <path d="M14 14h2m5 0h13M14 34h2m5 0h13M24 21v6" />
      </svg>
    </div>
    <h1 id="welcome-title">Catalogue de services</h1>
    <p class="lead">Connecte-toi avec Keycloak pour découvrir le catalogue de services.</p>
    <p v-if="session.expired" class="alert" role="status">Session expirée. Reconnecte-toi pour continuer.</p>
    <button @click="connectToCatalog">Se connecter <span aria-hidden="true">→</span></button>
    <p class="hint">Tu seras redirigé vers la page de connexion.</p>
  </section>
</template>
