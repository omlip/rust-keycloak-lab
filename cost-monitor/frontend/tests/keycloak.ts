// Protocol-level browser stub: tests execute the real keycloak-js adapter.
// Fake unsigned tokens are accepted only by the mocked API, never by Rust.
import type { Page } from '@playwright/test';

export async function mockKeycloak(page: Page, authenticated = true) {
  let online = authenticated;
  const nonces = new Map<string, string>();
  const jwt = (claims: Record<string, unknown>) => [
    Buffer.from(JSON.stringify({ alg: 'RS256', typ: 'JWT' })).toString('base64url'),
    Buffer.from(JSON.stringify(claims)).toString('base64url'), 'test-signature',
  ].join('.');
  await page.route('http://localhost:8081/**', async route => {
    const url = new URL(route.request().url());
    if (url.pathname.includes('/3p-cookies/')) {
      return route.fulfill({ contentType: 'text/html', body: '<script>parent.postMessage("supported", "*")</script>' });
    }
    if (url.pathname.endsWith('/auth')) {
      const redirect = new URL(url.searchParams.get('redirect_uri')!);
      const state = url.searchParams.get('state')!;
      const result = new URLSearchParams({ state });
      if (!online && url.searchParams.get('prompt') === 'none') result.set('error', 'login_required');
      else {
        online = true;
        result.set('code', state);
        result.set('session_state', 'test-session');
        nonces.set(state, url.searchParams.get('nonce')!);
      }
      redirect.hash = result.toString();
      return route.fulfill({ status: 302, headers: { location: redirect.href } });
    }
    if (url.pathname.endsWith('/token')) {
      const form = new URLSearchParams(route.request().postData() ?? '');
      const now = Math.floor(Date.now() / 1000);
      const common = { iss: 'http://localhost:8081/realms/olaninc', sub: 'alice', sid: 'test-session',
        azp: 'cost-monitor-login', iat: now, exp: now + 300, preferred_username: '<Alice>' };
      return route.fulfill({
        headers: { 'access-control-allow-origin': route.request().headers().origin ?? '*', 'access-control-allow-credentials': 'true', 'content-type': 'application/json' },
        body: JSON.stringify({ access_token: jwt({ ...common, aud: 'cost-monitor-api', typ: 'Bearer' }),
          id_token: jwt({ ...common, aud: 'cost-monitor-login', nonce: nonces.get(form.get('code')!) }),
          refresh_token: jwt({ ...common, typ: 'Refresh' }), expires_in: 300, token_type: 'Bearer' }),
      });
    }
    if (url.pathname.endsWith('/logout')) {
      online = false;
      return route.fulfill({ status: 302, headers: { location: url.searchParams.get('post_logout_redirect_uri')! } });
    }
    return route.abort();
  });
  await page.route('**/api/catalog/complete', route => route.fulfill({ status: 204 }));
}
