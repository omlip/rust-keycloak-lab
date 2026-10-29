<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { ApiError, request, type Dashboard, type Selection } from '../api';
import { money, total } from '../format';
import AccountMenu from '../components/AccountMenu.vue';
import AlertDialog from '../components/AlertDialog.vue';
import { session, login, logout } from '../auth';

const route = useRoute();
const router = useRouter();
const dashboard = ref<Dashboard | null>(null);
const loading = ref(true);
const busy = ref(false);
const error = ref(typeof route.query.error === 'string' ? route.query.error : '');
const selected = ref<Selection | null>(null);
const monthlyTotal = computed(() => total(dashboard.value?.selections.map(s => s.monthly_total_cents) ?? []));

async function load() {
  if (!session.authenticated) { dashboard.value = null; selected.value = null; return; }
  dashboard.value = await request<Dashboard>('/api/dashboard');
}
async function perform(operation: () => Promise<void>, clearError = true) {
  if (busy.value) return;
  busy.value = true;
  if (clearError) error.value = '';
  try { await operation(); }
  catch (cause) {
    error.value = cause instanceof Error ? cause.message : 'Cost Monitor est temporairement indisponible.';
    if (cause instanceof ApiError && cause.status === 401) { dashboard.value = null; selected.value = null; }
  } finally { busy.value = false; loading.value = false; }
}
async function refresh(force = false) {
  await perform(async () => {
    if (force) dashboard.value = await request<Dashboard>('/api/catalog/refresh', {});
    else await load();
  });
}
async function disconnect() {
  await perform(async () => { await request('/api/catalog/disconnect', {}); await load(); });
}
async function save(threshold?: string) {
  if (!selected.value) return;
  const path = `/api/alerts/${selected.value.id}${threshold === undefined ? '/delete' : ''}`;
  await perform(async () => {
    await request(path, threshold === undefined ? {} : { threshold });
    selected.value = null;
    await load();
  });
}
function open(selection: Selection) { error.value = ''; selected.value = selection; }
async function connect() {
  await perform(async () => {
    const { url } = await request<{ url: string }>('/api/catalog/connect', {});
    // Only Rust constructs this URL from its trusted provider discovery.
    window.location.assign(url);
  });
}
watch(() => [session.authenticated, session.subject], () => {
  dashboard.value = null; selected.value = null;
  if (session.authenticated) void perform(load);
});
onMounted(async () => {
  if (route.query.error || route.query.catalog_return) await router.replace({ path: '/', query: {} });
  await perform(async () => {
    if (session.authenticated) await request('/api/catalog/complete', {});
    await load();
  }, false);
});
</script>

<template>
  <main :aria-busy="busy">
    <header class="topbar">
      <div class="brand"><img src="/logo.svg" alt=""><div><h1>Cost Monitor</h1><p>Suivi des coûts</p></div></div>
      <AccountMenu v-if="session.authenticated && dashboard" :dashboard="dashboard" :busy="busy" @refresh="refresh(true)" @disconnect="disconnect" @logout="logout" />
    </header>
    <p v-if="error && !selected" class="error" role="alert">{{ error }}</p>
    <p v-if="session.error" class="error" role="alert">{{ session.error }}</p>
    <p v-if="loading" role="status">Chargement de Cost Monitor…</p>
    <div v-else-if="!session.authenticated" class="welcome-main">
      <section class="hero" aria-labelledby="welcome-title">
        <div class="welcome-icon" aria-hidden="true"><img src="/logo.svg" width="64" height="64" alt=""></div>
        <h2 id="welcome-title">Cost Monitor</h2>
        <p class="lead">Connecte-toi avec Keycloak pour suivre le coût de tes sélections de services.</p>
        <button class="button" @click="login">Se connecter <span aria-hidden="true">→</span></button>
        <p class="hint">Tu seras redirigé vers la page de connexion.</p>
        <button v-if="error" class="secondary" :disabled="busy" @click="refresh()">Réessayer</button>
      </section>
    </div>
    <template v-else-if="dashboard">
      <p v-if="dashboard.message" class="error" role="alert">{{ dashboard.message }}</p>
      <section v-if="!dashboard.connected" class="catalog-connect" aria-labelledby="catalog-connect-title">
        <h2 id="catalog-connect-title">Suivre mes sélections</h2>
        <p>{{ dashboard.has_catalog ? 'Reconnecte ton catalogue pour retrouver tes sélections.' : 'Connecte ton catalogue pour suivre le coût de tes sélections.' }}</p>
        <div class="connect-form"><button :disabled="busy" @click="connect">{{ dashboard.has_catalog ? 'Reconnecter mon catalogue' : 'Connecter mon catalogue' }}</button></div>
      </section>
      <template v-if="dashboard.connected && !dashboard.message">
        <div class="section-toolbar"><h2>Mes sélections</h2><button class="secondary" :disabled="busy" @click="refresh()">Actualiser les coûts</button></div>
        <section v-if="!dashboard.selections.length">Aucune sélection. Crée tes sélections dans l’application catalogue, puis actualise cette page.</section>
        <div class="grid">
          <article v-for="selection in dashboard.selections" :key="selection.id">
            <div class="selection-card-heading">
              <h3>{{ selection.name }}</h3>
              <button type="button" class="bell-button" :class="{ 'bell-configured': selection.threshold_cents !== null }" :aria-label="selection.threshold_cents !== null ? `Modifier l’alerte de ${selection.name}` : `Configurer une alerte pour ${selection.name}`" aria-haspopup="dialog" :disabled="busy" @click="open(selection)">
                <svg aria-hidden="true" width="24" height="24" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M18 8a6 6 0 0 0-12 0c0 7-3 7-3 9h18c0-2-3-2-3-9Z" /><path class="bell-clapper" d="M10 21h4" /></svg>
              </button>
            </div>
            <p class="hint">{{ selection.services.length }} service(s)</p>
            <strong class="price">{{ money(selection.monthly_total_cents) }} / mois</strong>
            <template v-if="selection.threshold_cents !== null">
              <p class="alert-threshold">Seuil d’alerte : {{ money(selection.threshold_cents) }} / mois</p>
              <p v-if="BigInt(selection.monthly_total_cents) > BigInt(selection.threshold_cents)" class="budget-exceeded">Dépassement du seuil</p>
            </template>
            <ul><li v-for="(service, index) in selection.services" :key="index">{{ service.name }} — {{ money(service.monthly_price_cents) }} / mois</li></ul>
          </article>
        </div>
        <section class="total"><h2>Total mensuel des sélections</h2><strong>{{ monthlyTotal }}</strong><p class="hint">Un service présent dans plusieurs sélections est compté dans chacune.</p></section>
      </template>
      <AlertDialog v-if="selected" :selection="selected" :recipient="dashboard.recipient" :busy="busy" :error="error" @close="selected = null" @save="save" @remove="save()" />
    </template>
    <section v-else><p role="status">Le tableau de bord est indisponible.</p><button :disabled="busy" @click="refresh()">Réessayer</button></section>
  </main>
</template>
