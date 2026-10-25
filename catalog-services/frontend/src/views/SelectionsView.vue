<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { AccessDeniedError, hasRole } from '../auth';
import { categoryLabel, listServices, type DatacenterService } from '../catalog';
import { createSelection, deleteSelection, getSelection, listSelections, renameSelection, setSelectedService, type Selection } from '../selections';

const route = useRoute();
const router = useRouter();
const selections = ref<Selection[]>([]);
const selection = ref<Selection | null>(null);
const catalog = ref<DatacenterService[]>([]);
const name = ref('');
const loading = ref(false);
const saving = ref(false);
const error = ref('');
const notice = ref('');
const renaming = ref(false);
const detail = computed(() => route.params.id !== undefined);
const euros = new Intl.NumberFormat('fr-FR', { style: 'currency', currency: 'EUR' });
const selectedIds = computed(() => new Set(selection.value?.services.map(service => service.id) ?? []));
let generation = 0;
let active = true;
onUnmounted(() => { active = false; generation++; });

async function handleError(cause: unknown) {
  if (!active) return;
  if (cause instanceof AccessDeniedError) await router.replace('/denied');
  else error.value = cause instanceof Error ? cause.message : 'Appel API impossible.';
}

async function load() {
  const current = ++generation;
  loading.value = true;
  error.value = '';
  notice.value = '';
  name.value = '';
  renaming.value = false;
  selection.value = null;
  selections.value = [];
  catalog.value = [];
  try {
    if (detail.value) {
      const [item, services] = await Promise.all([getSelection(String(route.params.id)), listServices()]);
      if (current !== generation) return;
      selection.value = item;
      catalog.value = services;
    } else {
      const items = await listSelections();
      if (current !== generation) return;
      selections.value = items;
    }
  } catch (cause) { if (current === generation) await handleError(cause); }
  finally { if (current === generation) loading.value = false; }
}

watch(() => route.fullPath, load, { immediate: true });

async function create() {
  saving.value = true;
  error.value = '';
  try {
    const item = await createSelection(name.value);
    if (active) await router.push(`/selections/${item.id}`);
  } catch (cause) { await handleError(cause); }
  finally { saving.value = false; }
}

async function rename() {
  if (!selection.value) return;
  const current = generation;
  saving.value = true;
  error.value = '';
  notice.value = '';
  try {
    const item = await renameSelection(selection.value.id, name.value);
    if (current !== generation) return;
    selection.value = item;
    renaming.value = false;
    notice.value = 'Sélection renommée.';
  } catch (cause) { if (current === generation) await handleError(cause); }
  finally { saving.value = false; }
}

async function remove() {
  if (!selection.value || !window.confirm(`Supprimer la sélection « ${selection.value.name} » ? Les services du catalogue seront conservés.`)) return;
  const current = generation;
  saving.value = true;
  error.value = '';
  try {
    await deleteSelection(selection.value.id);
    if (active && current === generation) await router.push('/selections');
  } catch (cause) { if (current === generation) await handleError(cause); }
  finally { saving.value = false; }
}

async function toggle(service: DatacenterService, selected: boolean) {
  if (!selection.value) return;
  const current = generation;
  saving.value = true;
  error.value = '';
  notice.value = '';
  try {
    const item = await setSelectedService(selection.value.id, service.id, selected);
    if (current !== generation) return;
    selection.value = item;
    notice.value = selected ? 'Service ajouté.' : 'Service retiré.';
  } catch (cause) { if (current === generation) await handleError(cause); }
  finally { saving.value = false; }
}
</script>

<template>
  <section class="space" :class="hasRole('admin') ? 'admin-space' : 'user-space'">
    <div class="space-heading">
      <span class="badge">{{ hasRole('admin') ? 'ADMIN' : 'USER' }} · PRIVÉ</span>
      <h1>{{ detail ? 'Ma sélection' : 'Mes sélections' }}</h1>
      <p class="lead">Prépare tes besoins dans un espace personnel. Aucun partage, même avec les administrateurs.</p>
      <p class="hint">Sans quantité ni souscription. Tarifs actuels du catalogue, actualisés à chaque lecture. Les sélections disparaissent au redémarrage du backend.</p>
    </div>
    <div class="catalog-content">
      <p v-if="detail"><RouterLink to="/selections">← Mes sélections</RouterLink></p>
      <p v-if="error" class="alert error" role="alert">{{ error }}</p>
      <p v-if="notice" class="alert" role="status">{{ notice }}</p>
      <p v-if="loading" role="status">Chargement des sélections…</p>

      <template v-else-if="!detail">
        <form class="card catalog-form" @submit.prevent="create">
          <h2>Nouvelle sélection</h2>
          <fieldset :disabled="saving">
            <label for="selection-name">Nom de la sélection</label>
            <input id="selection-name" v-model="name" type="text" required placeholder="Infrastructure Montréal" />
            <div class="catalog-buttons"><button type="submit" :disabled="!name.trim()">{{ saving ? 'Création…' : 'Créer une sélection' }}</button></div>
          </fieldset>
        </form>
        <p v-if="!selections.length && !error" class="card">Tu n’as pas encore de sélection. Crée ta première sélection ci-dessus.</p>
        <div class="catalog-grid">
          <article v-for="item in selections" :key="item.id" class="card service-card">
            <h2>{{ item.name }}</h2>
            <p>{{ item.services.length }} service(s)</p>
            <p class="service-price">{{ euros.format(item.monthly_total_cents / 100) }} <span>/ mois</span></p>
            <RouterLink class="button" :to="`/selections/${item.id}`" :aria-label="`Ouvrir la sélection ${item.name}`">Ouvrir</RouterLink>
          </article>
        </div>
      </template>

      <template v-else-if="selection">
        <article class="card selection-summary">
          <h2>{{ selection.name }}</h2>
          <p class="selection-total">Total mensuel : <strong>{{ euros.format(selection.monthly_total_cents / 100) }}</strong></p>
          <p>{{ selection.services.length }} service(s) sélectionné(s)</p>
          <div class="catalog-buttons">
            <button class="secondary" :disabled="saving" @click="renaming = true; name = selection.name">Renommer</button>
            <button class="secondary danger" :disabled="saving" @click="remove">Supprimer la sélection</button>
          </div>
        </article>
        <form v-if="renaming" class="card catalog-form" @submit.prevent="rename">
          <fieldset :disabled="saving">
            <label for="rename-selection">Nouveau nom</label>
            <input id="rename-selection" v-model="name" type="text" required />
            <div class="catalog-buttons">
              <button type="submit" :disabled="!name.trim()">Enregistrer</button>
              <button type="button" class="secondary" @click="renaming = false">Annuler</button>
            </div>
          </fieldset>
        </form>

        <h2>Services sélectionnés</h2>
        <p v-if="!selection.services.length">Cette sélection est vide.</p>
        <div class="catalog-grid">
          <article v-for="service in selection.services" :key="service.id" class="card service-card">
            <span class="badge">{{ categoryLabel(service.category) }}</span>
            <h3>{{ service.name }}</h3>
            <p>{{ euros.format(service.monthly_price_cents / 100) }} / mois</p>
            <button class="secondary" :disabled="saving" :aria-label="`Retirer ${service.name} de la sélection`" @click="toggle(service, false)">Retirer</button>
          </article>
        </div>

        <h2>Ajouter depuis le catalogue</h2>
        <p v-if="!catalog.length">Aucun service disponible. Les services supprimés du catalogue sont automatiquement retirés des sélections.</p>
        <div class="catalog-grid">
          <article v-for="service in catalog" :key="service.id" class="card service-card">
            <span class="badge">{{ categoryLabel(service.category) }}</span>
            <h3>{{ service.name }}</h3>
            <p class="service-description">{{ service.description }}</p>
            <p>{{ euros.format(service.monthly_price_cents / 100) }} / mois</p>
            <button :disabled="saving || selectedIds.has(service.id)" :aria-label="`Ajouter ${service.name} à la sélection`" @click="toggle(service, true)">{{ selectedIds.has(service.id) ? 'Déjà sélectionné' : 'Ajouter' }}</button>
          </article>
        </div>
      </template>
    </div>
  </section>
</template>
