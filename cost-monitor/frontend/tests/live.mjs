// Opt-in local integration test. Creates and removes only its own catalogue data.
// Requires the AD demo accounts, Keycloak, both apps and Mailpit on default ports.
import assert from 'node:assert/strict';
import { chromium } from '@playwright/test';

const browser = await chromium.launch({ headless: true });
const marker = `Vue Cost Monitor ${Date.now()}`;
const errors = [];
let admin, owner, service, selection;
async function credentials(page, user) {
  await page.locator('#username').fill(user);
  await page.locator('#password').fill(`${user}-12345!`);
  await page.locator('#kc-login').click();
}
async function source(user) {
  const context = await browser.newContext();
  const page = await context.newPage();
  let authorization;
  page.on('request', request => {
    if (request.url().includes('localhost:8080/api/')) authorization = request.headers().authorization ?? authorization;
  });
  await page.goto('http://localhost:8080/');
  await page.getByRole('button', { name: 'Se connecter', exact: true }).click();
  await credentials(page, user);
  await page.getByRole('group', { name: `Connecté en tant que ${user}`, exact: true }).waitFor();
  const api = (method, path, data) => context.request.fetch(`http://localhost:8080${path}`, {
    method, headers: { Authorization: authorization }, ...(data === undefined ? {} : { data }),
  });
  return { context, page, api };
}
async function menu(page, action) {
  await page.getByRole('button', { name: 'Menu du compte', exact: true }).click();
  await page.getByRole('button', { name: action, exact: true }).click();
}
async function logout(page) {
  const provider = page.waitForRequest(r => r.url().includes('/protocol/openid-connect/logout'));
  await menu(page, 'Se déconnecter de Cost Monitor');
  const url = new URL((await provider).url());
  assert.ok(url.searchParams.has('id_token_hint'));
  assert.equal(url.searchParams.get('post_logout_redirect_uri'), 'http://localhost:8082/');
  await page.waitForURL('http://localhost:8082/');
  await page.getByRole('button', { name: 'Se connecter', exact: true }).waitFor();
  assert.equal(await page.locator('#kc-logout').count(), 0);
}
async function login(page, user) {
  await page.waitForFunction(() => document.querySelector('[aria-label="Menu du compte"]') || [...document.querySelectorAll('button')].some(b => b.textContent?.trim().startsWith('Se connecter')));
  if (await page.getByRole('button', { name: 'Se connecter', exact: true }).count()) {
    await page.getByRole('button', { name: 'Se connecter', exact: true }).click();
    if (user) await credentials(page, user);
  }
  await page.getByRole('button', { name: 'Menu du compte', exact: true }).waitFor();
}
async function email(subject) {
  const deadline = Date.now() + 85000;
  while (Date.now() < deadline) {
    const listing = await (await fetch('http://localhost:8025/api/v1/messages?limit=100')).json();
    for (const item of listing.messages.filter(m => m.Subject === subject)) {
      const message = await (await fetch(`http://localhost:8025/api/v1/message/${item.ID}`)).json();
      if (message.Text?.includes(marker)) return message;
    }
    await new Promise(resolve => setTimeout(resolve, 2000));
  }
  throw new Error(`Mailpit: notification not received: ${subject}`);
}
try {
  admin = await source('admin1');
  owner = await source('user4');
  let response = await admin.api('POST', '/api/catalog/services', {
    name: marker, category: 'compute', description: 'Temporary Vue migration regression test', monthly_price_cents: 1500,
  });
  assert.equal(response.status(), 201); service = await response.json();
  response = await owner.api('POST', '/api/selections', { name: marker });
  assert.equal(response.status(), 201); selection = await response.json();
  response = await owner.api('PUT', `/api/selections/${selection.id}/services/${service.id}`);
  assert.equal(response.status(), 200);
  const page = await owner.context.newPage();
  let authorization;
  page.on('request', r => { if (r.url().includes('localhost:8082/api/')) authorization = r.headers().authorization ?? authorization; });
  page.on('pageerror', e => errors.push(e.message));
  page.on('console', message => { if (message.type() === 'error' && /Content Security Policy|Refused to/i.test(message.text())) errors.push(message.text()); });
  await page.goto('http://localhost:8082/');
  assert.equal(await page.locator('#app').count(), 1);
  await login(page); // Explicit click reuses the catalogue SSO without credentials.
  const connect = page.getByRole('button', { name: /^(Connecter|Reconnecter) mon catalogue$/ });
  if (await connect.count()) {
    await connect.click();
    await page.locator('#kc-oauth').waitFor();
    await page.locator('#kc-login').click();
  }
  const bell = page.getByRole('button', { name: `Configurer une alerte pour ${marker}`, exact: true });
  await bell.waitFor();
  await page.setViewportSize({ width: 390, height: 844 });
  await bell.click();
  await page.getByRole('dialog').getByLabel('Seuil mensuel (€)', { exact: true }).fill('10,00');
  await page.getByRole('dialog').getByRole('button', { name: 'Enregistrer', exact: true }).click();
  await page.getByText('Seuil d’alerte : 10,00 € / mois', { exact: true }).waitFor();
  assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
  const dashboard = await (await owner.context.request.get('http://localhost:8082/api/dashboard', { headers: { Authorization: authorization } })).json();
  assert.equal(dashboard.name, 'user4');
  assert.equal(dashboard.selections.find(s => s.id === String(selection.id)).threshold_cents, '1000');
  assert.ok(!JSON.stringify(dashboard).match(/access_token|refresh_token|id_token|client_secret/));
  assert.ok(!(await owner.context.cookies()).some(c => c.name === 'cm_session'));
  const forbidden = await owner.context.request.post(`http://localhost:8082/api/alerts/${selection.id}`, {
    headers: { Origin: 'http://evil.invalid', Accept: 'application/json', Authorization: authorization }, form: { threshold: '1' },
  });
  assert.equal(forbidden.status(), 403);
  const cookieOnly = await owner.context.request.get('http://localhost:8082/api/dashboard');
  assert.equal(cookieOnly.status(), 401);
  const wrongAudience = await owner.context.request.get('http://localhost:8080/api/selections', { headers: { Authorization: authorization } });
  assert.equal(wrongAudience.status(), 401);
  const persisted = await page.evaluate(() => JSON.stringify([Object.values(localStorage), Object.values(sessionStorage)]));
  assert.ok(!persisted.includes('access_token') && !persisted.includes('refresh_token') && !persisted.includes('id_token'));
  await logout(page);
  const exceeded = await email('[Cost Monitor] Seuil de coût dépassé');
  assert.ok(exceeded.To.some(to => to.Address === 'user4@ad.olaninc.test'));
  assert.ok(exceeded.Text.includes('15,00 €') && exceeded.Text.includes('10,00 €'));
  console.log('keycloak-js en production : Bearer, audiences séparées, délégation catalogue, logout sans confirmation et e-mail offline validés.');
  await login(page, 'user4');
  await page.getByRole('button', { name: `Modifier l’alerte de ${marker}`, exact: true }).waitFor();
  await menu(page, 'Renouveler maintenant');
  await page.getByText('Seuil d’alerte : 10,00 € / mois', { exact: true }).waitFor();
  response = await admin.api('PUT', `/api/catalog/services/${service.id}`, { ...service, monthly_price_cents: 1000 });
  assert.equal(response.status(), 200);
  await logout(page);
  const recovery = await email('[Cost Monitor] Retour à la normale');
  assert.ok(recovery.To.some(to => to.Address === 'user4@ad.olaninc.test'));
  assert.ok(recovery.Text.includes('10,00 €'));
  await login(page, 'user4');
  await page.getByRole('button', { name: `Modifier l’alerte de ${marker}`, exact: true }).click();
  await page.getByRole('button', { name: 'Supprimer l’alerte', exact: true }).click();
  await bell.waitFor();
  await logout(page);
  await login(page, 'user2');
  await page.getByRole('group', { name: 'Connecté en tant que user2', exact: true }).waitFor();
  assert.equal(await page.getByRole('heading', { name: marker, exact: true }).count(), 0);
  await logout(page);
  assert.deepEqual(errors, []);
  console.log('Restauration du compte, renouvellement offline, retour à la normale à égalité, suppression et isolation user2 validés.');
} finally {
  try {
    if (selection && owner) assert.equal((await owner.api('DELETE', `/api/selections/${selection.id}`)).status(), 204);
    if (service && admin) assert.equal((await admin.api('DELETE', `/api/catalog/services/${service.id}`)).status(), 204);
  } finally { await browser.close(); }
}
