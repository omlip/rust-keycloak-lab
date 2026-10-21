<script setup lang="ts">
import { computed } from 'vue';
import { accountUrl, hasRole, keycloakBaseUrl, nativeAction, realm, session } from '../auth';

const hasAdminRole = computed(() => hasRole('admin'));
</script>

<template>
  <section class="space" :class="hasAdminRole ? 'admin-space' : 'user-space'">
    <div class="space-heading">
      <span class="badge">{{ hasAdminRole ? 'ADMIN' : 'USER' }}</span>
      <h1>Paramètres</h1>
      <p class="lead">Bonjour, <strong>{{ session.name }}</strong>.</p>
    </div>
    <div class="space-content settings-content">
      <article class="card">
        <h2>Sécurité de mon compte</h2>
        <p>Ces actions ouvrent les formulaires natifs Keycloak, puis reviennent dans l’application.</p>
        <div class="actions">
          <button @click="nativeAction('UPDATE_PASSWORD')">Changer mon mot de passe</button>
          <button class="secondary" @click="nativeAction('CONFIGURE_TOTP')">Configurer mon OTP</button>
        </div>
        <p class="hint">L’OTP est facultatif. Une fois configuré, il sera demandé aux prochaines authentifications.</p>
        <a class="text-link" :href="accountUrl()">Editer mon profil →</a>
      </article>
      <article v-if="hasAdminRole" class="card">
        <h2>Administration du realm</h2>
        <p>Gère les utilisateurs et leurs réinitialisations de mot de passe dans la console native Keycloak.</p>
        <a class="button" :href="`${keycloakBaseUrl}/admin/${realm}/console/`" target="_blank" rel="noopener noreferrer">Ouvrir la console Keycloak ↗</a>
        <p class="hint">Ce compte possède explicitement les droits d’administration du realm olaninc, indépendamment du rôle applicatif admin.</p>
      </article>
    </div>
  </section>
</template>
