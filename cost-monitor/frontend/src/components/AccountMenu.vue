<script setup lang="ts">
import { ref } from 'vue';
import type { Dashboard } from '../api';
import { initials } from '../format';

defineProps<{ dashboard: Dashboard; busy: boolean }>();
const emit = defineEmits<{ refresh: []; disconnect: []; logout: [] }>();
const panel = ref<HTMLElement>();
function action(name: 'refresh' | 'disconnect') {
  panel.value?.hidePopover();
  if (name === 'refresh') emit('refresh'); else emit('disconnect');
}
</script>

<template>
  <div class="account-area">
    <div class="account-identity" role="group" :aria-label="`Connecté en tant que ${dashboard.name}`" :title="dashboard.name ?? ''">
      <span class="account-avatar" aria-hidden="true">{{ initials(dashboard.name ?? '') }}</span>
      <span class="identity-name" aria-hidden="true">{{ dashboard.name }}</span>
    </div>
    <div class="account-menu">
      <button type="button" class="secondary account-menu-toggle" popovertarget="account-menu-panel" aria-controls="account-menu-panel" aria-label="Menu du compte">
        <svg aria-hidden="true" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M4 6h16M4 12h16M4 18h16" /></svg>
      </button>
      <div id="account-menu-panel" ref="panel" class="account-menu-panel" popover="auto">
        <div class="menu-identity"><p>Connecté en tant que</p><strong>{{ dashboard.name }}</strong></div>
        <nav aria-label="Actions du compte">
          <button v-if="dashboard.connected" :disabled="busy" @click="action('refresh')">Renouveler maintenant</button>
          <button v-if="dashboard.has_catalog" :disabled="busy" @click="action('disconnect')">Déconnecter le catalogue</button>
          <div class="logout-action">
            <button :disabled="busy" @click="emit('logout')">Se déconnecter de Cost Monitor</button>
          </div>
        </nav>
      </div>
    </div>
  </div>
</template>
