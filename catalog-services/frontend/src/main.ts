import { createApp } from 'vue';
import App from './App.vue';
import { initializeAuth } from './auth';
import './style.css';

async function start() {
  try {
    // Keycloak must consume the OIDC callback before the router reads the URL.
    await initializeAuth();
    const { router } = await import('./router');
    const app = createApp(App);
    app.use(router);
    await router.isReady();
    app.mount('#app');
  } catch {
    const root = document.querySelector('#app');
    if (root) {
      const panel = document.createElement('main');
      panel.className = 'startup-error';
      const heading = document.createElement('h1');
      heading.textContent = 'Connexion à Keycloak indisponible';
      const text = document.createElement('p');
      text.textContent = `Vérifie les services Docker, puis recharge la page. Adresse attendue : ${window.location.origin}.`;
      const retry = document.createElement('button');
      retry.textContent = 'Réessayer';
      retry.onclick = () => window.location.assign('/');
      panel.append(heading, text, retry);
      root.replaceChildren(panel);
    }
  }
}

void start();
