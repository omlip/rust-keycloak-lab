import Keycloak from 'keycloak-js';
import { reactive } from 'vue';

const callbackUrl = `${window.location.origin}/`;
const keycloak = new Keycloak({ url: 'http://localhost:8081', realm: 'olaninc', clientId: 'cost-monitor-login' });
export const session = reactive({ authenticated: false, subject: '', error: '', expired: false });
function sync() {
  session.authenticated = Boolean(keycloak.authenticated);
  session.subject = keycloak.tokenParsed?.sub ?? '';
  if (session.authenticated) { session.expired = false; session.error = ''; }
}
export function expireSession() {
  session.authenticated = false; session.subject = ''; session.expired = true;
  if (keycloak.authenticated) keycloak.clearToken();
}
keycloak.onAuthSuccess = sync;
keycloak.onAuthRefreshSuccess = sync;
keycloak.onAuthRefreshError = expireSession;
keycloak.onAuthLogout = expireSession;
keycloak.onTokenExpired = () => { void accessToken(); };

export async function initializeAuth() {
  let timeout: ReturnType<typeof setTimeout> | undefined;
  try {
    await Promise.race([
      keycloak.init({
        onLoad: 'check-sso', silentCheckSsoRedirectUri: `${window.location.origin}/silent-check-sso.html`,
        // The catalogue round trip loses in-memory SPA tokens. Restore this
        // online session by redirect if third-party iframes are unavailable.
        silentCheckSsoFallback: new URLSearchParams(window.location.search).has('catalog_return'),
        checkLoginIframe: false, flow: 'standard', pkceMethod: 'S256',
        redirectUri: callbackUrl, scope: 'openid profile',
      }),
      new Promise<never>((_, reject) => { timeout = setTimeout(() => reject(new Error('Keycloak est temporairement indisponible.')), 15000); }),
    ]);
    sync();
  } finally { clearTimeout(timeout); }
}
export async function accessToken(): Promise<string | null> {
  if (!keycloak.authenticated) return null;
  try { await keycloak.updateToken(30); sync(); return keycloak.token ?? null; }
  catch { expireSession(); return null; }
}
export async function login() {
  try { await keycloak.login({ redirectUri: callbackUrl, locale: 'fr' }); }
  catch { session.error = 'Impossible de joindre Keycloak.'; }
}
export async function logout() {
  // Best effort cancellation does not prevent provider logout during an API outage.
  // Keep the ID token in keycloak-js until it has built its native logout URL.
  try {
    if (keycloak.token) await fetch('/api/logout', {
      method: 'POST', credentials: 'same-origin', signal: AbortSignal.timeout(5000),
      headers: { Authorization: `Bearer ${keycloak.token}`, Accept: 'application/json' },
    });
  } catch { /* The provider must still end the browser SSO session. */ }
  session.authenticated = false; session.subject = '';
  try { await keycloak.logout({ redirectUri: callbackUrl }); }
  catch { expireSession(); session.error = 'Déconnexion Keycloak interrompue. Réessaie de te connecter.'; }
}
