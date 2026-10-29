import { createApp } from 'vue';
import { createRouter, createWebHistory } from 'vue-router';
import App from './App.vue';
import MonitorView from './views/MonitorView.vue';
import './styles.css';
import { initializeAuth, session } from './auth';

async function start() {
  // keycloak-js must consume its code/state before Vue Router touches the URL.
  try { await initializeAuth(); } catch (cause) { session.error = cause instanceof Error ? cause.message : 'Connexion Keycloak indisponible.'; }
  const router = createRouter({ history: createWebHistory(), routes: [
    { path: '/', component: MonitorView }, { path: '/:pathMatch(.*)*', redirect: '/' },
  ] });
  createApp(App).use(router).mount('#app');
}
void start();
