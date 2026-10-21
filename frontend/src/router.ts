import { createRouter, createWebHistory } from 'vue-router';
import { canAccessUser, defaultPath, hasRole, refreshSession, rememberDestination, session, takeDestination } from './auth';
import HomeView from './views/HomeView.vue';
import SettingsView from './views/SettingsView.vue';
import DeniedView from './views/DeniedView.vue';
import CatalogView from './views/CatalogView.vue';

export const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: '/', name: 'home', component: HomeView },
    { path: '/settings', name: 'settings', component: SettingsView, meta: { requiresAuth: true } },
    // Keep old bookmarks working; the guard preserves the admin-only legacy URL.
    { path: '/user', name: 'user', component: SettingsView, meta: { requiresAuth: true } },
    { path: '/admin', name: 'admin', component: SettingsView, meta: { requiresAuth: true } },
    { path: '/catalog', name: 'catalog', component: CatalogView, meta: { requiresAuth: true } },
    { path: '/denied', name: 'denied', component: DeniedView },
    { path: '/:pathMatch(.*)*', redirect: '/' },
  ],
});

router.beforeEach(async (to) => {
  const protectedPage = Boolean(to.meta.requiresAuth);
  if (session.authenticated) await refreshSession();
  if (protectedPage && !session.authenticated) {
    rememberDestination(to.path);
    return '/';
  }
  if (to.name === 'home' && session.authenticated) {
    return takeDestination() ?? defaultPath();
  }
  if (to.name === 'admin' && !hasRole('admin')) return '/denied';
  if (protectedPage && !canAccessUser()) return '/denied';
  if (to.name === 'user' || to.name === 'admin') return '/settings';
});
