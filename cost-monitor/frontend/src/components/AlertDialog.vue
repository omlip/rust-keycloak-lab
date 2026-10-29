<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue';
import type { Selection } from '../api';
import { decimal, money } from '../format';

const props = defineProps<{ selection: Selection; recipient: string | null; busy: boolean; error: string }>();
const emit = defineEmits<{ close: []; save: [threshold: string]; remove: [] }>();
const dialog = ref<HTMLDialogElement>();
const threshold = ref(decimal(props.selection.threshold_cents));
onMounted(() => dialog.value?.showModal());
onBeforeUnmount(() => dialog.value?.close());
</script>

<template>
  <dialog ref="dialog" class="alert-dialog" aria-labelledby="alert-title" @close="emit('close')" @cancel="emit('close')">
    <div class="dialog-body">
      <div class="dialog-heading">
        <h2 id="alert-title">Alerte — {{ selection.name }}</h2>
        <button type="button" class="secondary dialog-close" aria-label="Fermer la configuration d’alerte" :disabled="busy" @click="emit('close')">×</button>
      </div>
      <p>Coût mensuel actuel : <strong>{{ money(selection.monthly_total_cents) }}</strong></p>
      <p v-if="error" class="error" role="alert">{{ error }}</p>
      <form @submit.prevent="emit('save', threshold)">
        <label for="alert-value">Seuil mensuel (€)</label>
        <input id="alert-value" v-model="threshold" name="threshold" type="text" inputmode="decimal" pattern="[0-9]+([.,][0-9]{1,2})?" placeholder="250,00" required autofocus :disabled="busy">
        <p v-if="recipient" class="hint">Notifications envoyées à {{ recipient }} : dépassement puis retour au seuil ou en dessous, sans rappels. Contrôle chaque minute.</p>
        <p v-else class="error">Une adresse e-mail autorisée est nécessaire. Vérifie ton profil Keycloak puis reconnecte le catalogue.</p>
        <div class="dialog-actions">
          <button :disabled="busy || !recipient">Enregistrer</button>
          <button type="button" class="secondary" :disabled="busy" @click="emit('close')">Annuler</button>
        </div>
      </form>
      <div v-if="selection.threshold_cents !== null" class="alert-delete">
        <button type="button" class="secondary danger" :disabled="busy" @click="emit('remove')">Supprimer l’alerte</button>
      </div>
    </div>
  </dialog>
</template>
