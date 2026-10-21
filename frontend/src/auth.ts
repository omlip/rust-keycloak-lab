import Keycloak from 'keycloak-js';
import { reactive } from 'vue';

export const keycloakBaseUrl = 'http://localhost:8081';
export const realm = 'olaninc';
export const callbackUrl = `${window.location.origin}/`;
const destinationKey = 'olaninc.returnPath';

const keycloak = new Keycloak({
  url: keycloakBaseUrl,
  realm,
  clientId: 'olaninc-spa',
});

export const session = reactive({
  authenticated: false,
  name: '',
  roles: [] as string[],
  expired: false,
  error: '',
});

function syncSession() {
  session.authenticated = Boolean(keycloak.authenticated);
  session.roles = [...(keycloak.realmAccess?.roles ?? [])];
  session.name = keycloak.tokenParsed?.name
    ?? keycloak.tokenParsed?.preferred_username
    ?? 'Utilisateur';
  if (session.authenticated) session.expired = false;
}

export function hasRole(role: string): boolean {
  return session.roles.includes(role);
}

export function canAccessUser(): boolean {
  return hasRole('users') || hasRole('admin');
}

export function defaultPath(): string {
  return canAccessUser() ? '/catalog' : '/denied';
}

export function expireSession() {
  // clearToken calls onAuthLogout synchronously: clear the reactive state first.
  session.authenticated = false;
  session.name = '';
  session.roles = [];
  session.expired = true;
  if (keycloak.authenticated) keycloak.clearToken();
}

keycloak.onAuthSuccess = syncSession;
keycloak.onAuthRefreshSuccess = syncSession;
keycloak.onAuthRefreshError = expireSession;
keycloak.onAuthLogout = expireSession;
keycloak.onTokenExpired = () => { void refreshSession(); };

export async function initializeAuth() {
  let timeout: ReturnType<typeof setTimeout> | undefined;
  try {
    await Promise.race([
      keycloak.init({
        onLoad: 'check-sso',
        silentCheckSsoRedirectUri: `${window.location.origin}/silent-check-sso.html`,
        silentCheckSsoFallback: false,
        checkLoginIframe: false,
        flow: 'standard',
        pkceMethod: 'S256',
        redirectUri: callbackUrl,
        scope: 'openid profile email',
      }),
      new Promise<never>((_, reject) => {
        timeout = setTimeout(() => reject(new Error('Keycloak initialization timed out')), 15000);
      }),
    ]);
  } finally {
    clearTimeout(timeout);
  }
  syncSession();
}

export async function refreshSession(): Promise<boolean> {
  if (!keycloak.authenticated) return false;
  try {
    await keycloak.updateToken(30);
    syncSession();
    return true;
  } catch {
    expireSession();
    return false;
  }
}

export function rememberDestination(path: string) {
  // Only navigation state is stored, never access, ID or refresh tokens.
  if (path === '/settings' || path === '/user' || path === '/admin' || path === '/catalog') {
    sessionStorage.setItem(destinationKey, path);
  }
}

export function takeDestination(): string | null {
  const path = sessionStorage.getItem(destinationKey);
  sessionStorage.removeItem(destinationKey);
  return path === '/settings' || path === '/user' || path === '/admin' || path === '/catalog' ? path : null;
}

async function redirect(action: () => Promise<unknown>) {
  session.error = '';
  try {
    await action();
  } catch {
    session.error = 'Impossible de joindre Keycloak. Vérifie que les services sont démarrés.';
  }
}

export function login() {
  return redirect(() => keycloak.login({ redirectUri: callbackUrl, locale: 'fr' }));
}

export function logout() {
  sessionStorage.removeItem(destinationKey);
  return redirect(() => keycloak.logout({ redirectUri: callbackUrl }));
}

export function nativeAction(action: 'UPDATE_PASSWORD' | 'CONFIGURE_TOTP') {
  rememberDestination(window.location.pathname);
  return redirect(() => keycloak.login({ action, redirectUri: callbackUrl, locale: 'fr' }));
}

export function accountUrl(): string {
  return keycloak.createAccountUrl({ redirectUri: callbackUrl });
}

export async function apiRequest<T>(path: string, options: RequestInit = {}): Promise<T> {
  if (!await refreshSession() || !keycloak.token) {
    throw new Error('Session expirée. Reconnecte-toi pour continuer.');
  }
  const headers = new Headers(options.headers);
  headers.set('Authorization', `Bearer ${keycloak.token}`);
  headers.set('Accept', 'application/json');
  const response = await fetch(path, {
    ...options,
    headers,
    cache: 'no-store',
  });
  if (response.status === 401) {
    expireSession();
    throw new Error('Session expirée. Reconnecte-toi pour continuer.');
  }
  if (response.status === 403) throw new AccessDeniedError();
  if (!response.ok) {
    const data = await response.json().catch(() => null);
    throw new Error(data?.message ?? `Appel API impossible (HTTP ${response.status}).`);
  }
  if (response.status === 204) return undefined as T;
  return response.json();
}

export class AccessDeniedError extends Error {
  constructor() { super('Accès refusé'); }
}
