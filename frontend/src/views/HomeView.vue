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
      <img src="/logo.svg" width="64" height="64" alt="" />
    </div>
    <h1 id="welcome-title">Catalogue de services</h1>
    <p class="lead">Connecte-toi avec Keycloak pour découvrir le catalogue de services.</p>
    <p v-if="session.expired" class="alert" role="status">Session expirée. Reconnecte-toi pour continuer.</p>
    <button @click="connectToCatalog">Se connecter <span aria-hidden="true">→</span></button>
    <p class="hint">Tu seras redirigé vers la page de connexion.</p>
  </section>
</template>
