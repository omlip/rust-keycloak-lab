<script setup lang="ts">
import { computed, onMounted, onUnmounted, reactive, ref, watch } from 'vue';
import { useRouter } from 'vue-router';
import { AccessDeniedError, hasRole } from '../auth';
import { categories, categoryLabel, deleteService, listServices, saveService, type Category, type DatacenterService } from '../catalog';

const router = useRouter();
const isAdmin = computed(() => hasRole('admin'));
const services = ref<DatacenterService[]>([]);
const loading = ref(false);
const saving = ref(false);
const error = ref('');
const notice = ref('');
const formOpen = ref(false);
const editingId = ref<number | null>(null);
const form = reactive({ name: '', category: 'compute' as Category, description: '', monthlyPrice: 0 });
const euros = new Intl.NumberFormat('fr-FR', { style: 'currency', currency: 'EUR' });
let active = true;

onUnmounted(() => { active = false; });
watch(isAdmin, admin => { if (!admin) formOpen.value = false; });

async function handleError(cause: unknown) {
  if (!active) return;
  if (cause instanceof AccessDeniedError) await router.replace('/denied');
  else error.value = cause instanceof Error ? cause.message : 'Appel API impossible.';
}

async function load() {
  loading.value = true;
  error.value = '';
  try { services.value = await listServices(); }
  catch (cause) { await handleError(cause); }
  finally { loading.value = false; }
}

function openForm(service?: DatacenterService) {
  editingId.value = service?.id ?? null;
  Object.assign(form, {
    name: service?.name ?? '',
    category: service?.category ?? 'compute',
    description: service?.description ?? '',
    monthlyPrice: (service?.monthly_price_cents ?? 0) / 100,
  });
  error.value = '';
  notice.value = '';
  formOpen.value = true;
}

async function submit() {
  saving.value = true;
  error.value = '';
  notice.value = '';
  try {
    const wasEditing = editingId.value !== null;
    await saveService({
      name: form.name,
      category: form.category,
      description: form.description,
      monthly_price_cents: Math.round(Number(form.monthlyPrice) * 100),
    }, editingId.value);
    formOpen.value = false;
    notice.value = wasEditing ? 'Service modifié.' : 'Service ajouté.';
    await load();
  } catch (cause) { await handleError(cause); }
  finally { saving.value = false; }
}

async function remove(service: DatacenterService) {
  if (!window.confirm(`Supprimer le service « ${service.name} » ?`)) return;
  saving.value = true;
  error.value = '';
  notice.value = '';
  try {
    await deleteService(service.id);
    if (editingId.value === service.id) formOpen.value = false;
    notice.value = 'Service supprimé.';
    await load();
  } catch (cause) { await handleError(cause); }
  finally { saving.value = false; }
}

onMounted(load);
</script>

<template>
  <section class="space catalog-space" :class="isAdmin ? 'admin-space' : 'user-space'">
    <div class="space-heading">
      <span class="badge">{{ isAdmin ? 'ADMIN · GESTION' : 'USER · CONSULTATION' }}</span>
      <h1>Catalogue de services</h1>
      <p class="lead">Des services fictifs de datacenter, un même catalogue pour tous.</p>
      <p class="hint">Tarifs mensuels indicatifs en euros. Aucune souscription ni infrastructure réelle. Les données sont réinitialisées au redémarrage du backend.</p>
    </div>
    <div class="catalog-content">
      <div class="catalog-toolbar">
        <p>{{ services.length }} service(s)</p>
        <button v-if="isAdmin" :disabled="loading || saving" @click="openForm()">Ajouter un service</button>
      </div>
      <p v-if="error" class="alert error" role="alert">{{ error }}</p>
      <p v-if="notice" class="alert" role="status">{{ notice }}</p>

      <form v-if="isAdmin && formOpen" class="card catalog-form" @submit.prevent="submit">
        <h2>{{ editingId === null ? 'Ajouter un service' : 'Modifier le service' }}</h2>
        <fieldset :disabled="saving">
          <label for="service-name">Nom</label>
          <input id="service-name" v-model="form.name" type="text" />
          <label for="service-category">Catégorie</label>
          <select id="service-category" v-model="form.category">
            <option v-for="category in categories" :key="category.value" :value="category.value">{{ category.label }}</option>
          </select>
          <label for="service-description">Description</label>
          <textarea id="service-description" v-model="form.description" rows="3"></textarea>
          <label for="service-price">Tarif mensuel indicatif (€)</label>
          <input id="service-price" v-model.number="form.monthlyPrice" type="number" step="0.01" min="0" />
          <div class="catalog-buttons">
            <button type="submit">{{ saving ? 'Enregistrement…' : 'Enregistrer' }}</button>
            <button type="button" class="secondary" @click="formOpen = false">Annuler</button>
          </div>
        </fieldset>
      </form>

      <p v-if="loading" role="status">Chargement du catalogue…</p>
      <p v-else-if="!services.length && !error" class="card">Aucun service pour l’instant.</p>
      <div v-else class="catalog-grid" :aria-busy="loading">
        <article v-for="service in services" :key="service.id" class="card service-card">
          <span class="badge">{{ categoryLabel(service.category) }}</span>
          <h2>{{ service.name }}</h2>
          <p class="service-description">{{ service.description }}</p>
          <p class="service-price">{{ euros.format(service.monthly_price_cents / 100) }} <span>/ mois</span></p>
          <p class="hint">Référence #{{ service.id }}</p>
          <div v-if="isAdmin" class="catalog-buttons">
            <button class="secondary" :disabled="saving || loading" :aria-label="`Modifier ${service.name}`" @click="openForm(service)">Modifier</button>
            <button class="secondary danger" :disabled="saving || loading" :aria-label="`Supprimer ${service.name}`" @click="remove(service)">Supprimer</button>
          </div>
        </article>
      </div>
    </div>
  </section>
</template>
