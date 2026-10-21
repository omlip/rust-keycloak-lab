<script setup lang="ts">
import { canAccessUser, session } from '../auth';
</script>

<template>
  <section class="denied card">
    <span class="eyebrow">403 / AUTORISATION</span>
    <h1>Accès refusé</h1>
    <p>Ton compte ne possède pas le rôle nécessaire pour accéder à cet espace.</p>
    <RouterLink v-if="session.authenticated && canAccessUser()" class="button" to="/catalog">Revenir au catalogue</RouterLink>
    <RouterLink v-else class="button" to="/">Revenir à l’accueil</RouterLink>
    <p class="hint">L’API contrôle les permissions indépendamment de la navigation de cette interface.</p>
  </section>
</template>
