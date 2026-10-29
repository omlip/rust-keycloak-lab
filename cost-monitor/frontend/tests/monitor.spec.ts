import { test, expect, type Page } from '@playwright/test';
import type { Dashboard } from '../src/api';
import { money, total, initials, decimal } from '../src/format';
import { mockKeycloak } from './keycloak';

function fixture(): Dashboard {
  return {
    name: '<Alice>', connected: true, has_catalog: true,
    recipient: 'alice@example.test', message: null,
    selections: [{ id: '1', name: '<img src=x onerror=alert(1)>', monthly_total_cents: '1500',
      threshold_cents: null, services: [{ name: '<script>bad()</script>', monthly_price_cents: '1500' }] }],
  };
}

async function mock(page: Page, state: Dashboard) {
  await mockKeycloak(page);
  await page.route('**/api/dashboard', route => {
    expect(route.request().headers().authorization).toMatch(/^Bearer /);
    return route.fulfill({ json: state });
  });
  await page.goto('/');
}

test('exact cent formatting, totals, overflow and Unicode initials', () => {
  expect(money('18446744073709551615')).toBe('184467440737095516,15 €');
  expect(total(['18446744073709551615', '1'])).toBe('Montant total non représentable');
  expect(total(['101', '202'])).toBe('3,03 €');
  expect(decimal('1500')).toBe('15.00');
  expect(initials(' Élodie Dupont ')).toBe('ÉD');
  expect(initials('<>')).toBe('?');
});

test('public welcome and explicit keycloak-js login, without persistent token storage', async ({ page }) => {
  await mockKeycloak(page, false);
  await page.goto('/');
  await expect(page.getByRole('button', { name: 'Se connecter', exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Menu du compte' })).toHaveCount(0);
  expect(await page.evaluate(() => ({ local: localStorage.length, session: sessionStorage.length })))
    .toEqual({ local: 0, session: 0 });
});

test('Vue escapes identities, selection names and services', async ({ page }) => {
  const state = fixture();
  await mock(page, state);
  await expect(page.getByRole('group', { name: 'Connecté en tant que <Alice>' })).toBeVisible();
  await expect(page.getByRole('heading', { name: state.selections[0].name, exact: true })).toBeVisible();
  await expect(page.getByText('<script>bad()</script> — 15,00 € / mois')).toBeVisible();
  await expect(page.locator('article img, article script')).toHaveCount(0);
  await expect(page.getByRole('heading', { name: 'Total mensuel des sélections' })).toBeVisible();
});

test('alert dialog supports keyboard, Bearer-protected save, edit and delete', async ({ page }) => {
  const state = fixture();
  const submitted: Record<string, string>[] = [];
  await page.route('**/api/alerts/**', async route => {
    const form = Object.fromEntries(new URLSearchParams(route.request().postData() ?? ''));
    submitted.push(form);
    expect(route.request().headers().accept).toBe('application/json');
    expect(route.request().headers().authorization).toMatch(/^Bearer /);
    expect(form.csrf).toBeUndefined();
    state.selections[0].threshold_cents = route.request().url().endsWith('/delete') ? null : '1000';
    await route.fulfill({ status: 204 });
  });
  await mock(page, state);
  const bell = page.getByRole('button', { name: `Configurer une alerte pour ${state.selections[0].name}` });
  await bell.click();
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();
  await expect(dialog.getByLabel('Seuil mensuel (€)')).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
  await bell.click();
  await dialog.getByLabel('Seuil mensuel (€)').fill('10,00');
  await dialog.getByRole('button', { name: 'Enregistrer', exact: true }).click();
  await expect(dialog).toHaveCount(0);
  await expect(page.getByText('Seuil d’alerte : 10,00 € / mois')).toBeVisible();
  await expect(page.getByText('Dépassement du seuil', { exact: true })).toBeVisible();
  const edit = page.getByRole('button', { name: `Modifier l’alerte de ${state.selections[0].name}` });
  await expect(edit).toHaveClass(/bell-configured/);
  await edit.click();
  await expect(dialog.getByLabel('Seuil mensuel (€)')).toHaveValue('10.00');
  await dialog.getByRole('button', { name: 'Supprimer l’alerte' }).click();
  await expect(bell).toBeVisible();
  expect(submitted).toEqual([{ threshold: '10,00' }, {}]);
});

test('mobile dialog fits, and unapproved email cannot configure an alert', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const state = { ...fixture(), recipient: null };
  await mock(page, state);
  await page.getByRole('button', { name: /^Configurer une alerte/ }).click();
  const dialog = page.getByRole('dialog');
  await expect(dialog.getByRole('button', { name: 'Enregistrer', exact: true })).toBeDisabled();
  const box = await dialog.boundingBox();
  expect(box!.x).toBeGreaterThanOrEqual(0);
  expect(box!.x + box!.width).toBeLessThanOrEqual(390);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
});

test('renew and disconnect use Bearer; keycloak-js logout sends the ID token hint', async ({ page }) => {
  const state = fixture();
  await page.route('**/api/catalog/refresh', async route => {
    expect(route.request().headers().authorization).toMatch(/^Bearer /);
    await route.fulfill({ json: state });
  });
  await page.route('**/api/catalog/disconnect', async route => {
    expect(route.request().headers().authorization).toMatch(/^Bearer /);
    state.connected = false; state.has_catalog = false; state.selections = [];
    await route.fulfill({ status: 204 });
  });
  await mock(page, state);
  await page.getByRole('button', { name: 'Menu du compte' }).click();
  await page.getByRole('button', { name: 'Renouveler maintenant' }).click();
  await expect(page.getByRole('heading', { name: 'Mes sélections', exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Menu du compte' }).click();
  await page.getByRole('button', { name: 'Déconnecter le catalogue', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Connecter mon catalogue', exact: true })).toBeVisible();
  await page.route('**/api/logout', route => {
    expect(route.request().headers().authorization).toMatch(/^Bearer /);
    return route.fulfill({ status: 204 });
  });
  await page.getByRole('button', { name: 'Menu du compte' }).click();
  const providerLogout = page.waitForRequest(r => r.url().includes('/protocol/openid-connect/logout'));
  await page.getByRole('button', { name: 'Se déconnecter de Cost Monitor', exact: true }).click();
  const logout = new URL((await providerLogout).url());
  expect(logout.searchParams.has('id_token_hint')).toBe(true);
  expect(logout.searchParams.get('post_logout_redirect_uri')).toBe('http://127.0.0.1:4173/');
  await expect(page.getByRole('button', { name: 'Se connecter', exact: true })).toBeVisible();
});

test('expired session clears private dashboard and dialog after an action', async ({ page }) => {
  await page.route('**/api/alerts/**', route => route.fulfill({ status: 401, json: { message: 'Session expirée.' } }));
  await mock(page, fixture());
  await page.getByRole('button', { name: /^Configurer une alerte/ }).click();
  await page.getByRole('dialog').getByLabel('Seuil mensuel (€)').fill('10');
  await page.getByRole('button', { name: 'Enregistrer', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Se connecter', exact: true })).toBeVisible();
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await expect(page.locator('article')).toHaveCount(0);
});

test('callback errors are text, query is removed and access can be retried', async ({ page }) => {
  await mockKeycloak(page, false);
  await page.goto('/?error=%3Cscript%3Ebad()%3C/script%3E');
  await expect(page.getByRole('alert')).toHaveText('<script>bad()</script>');
  await expect(page.locator('main script')).toHaveCount(0);
  await expect(page).toHaveURL('/');
  await mockKeycloak(page);
  await page.route('**/api/dashboard', route => route.fulfill({ status: 503, json: { message: 'Indisponible.' } }));
  await page.reload();
  await expect(page.getByRole('alert')).toContainText('Indisponible.');
  await expect(page.getByRole('button', { name: 'Réessayer' })).toBeVisible();
  await expect(page).toHaveURL('/');
});

test('catalogue round trip restores online auth before completing the server grant', async ({ page }) => {
  await mockKeycloak(page);
  const sequence: string[] = [];
  await page.route('**/api/catalog/complete', route => {
    expect(route.request().headers().authorization).toMatch(/^Bearer /);
    sequence.push('complete'); return route.fulfill({ status: 204 });
  });
  await page.route('**/api/dashboard', route => {
    sequence.push('dashboard'); return route.fulfill({ json: fixture() });
  });
  await page.goto('/?catalog_return=1');
  await expect(page.getByRole('heading', { name: 'Mes sélections', exact: true })).toBeVisible();
  expect(sequence).toEqual(['complete', 'dashboard']);
  await expect(page).toHaveURL('/');
});

test('API outage cannot prevent provider logout with its ID token hint', async ({ page }) => {
  await mock(page, fixture());
  await page.route('**/api/logout', route => route.abort());
  await page.getByRole('button', { name: 'Menu du compte' }).click();
  const request = page.waitForRequest(r => r.url().includes('/protocol/openid-connect/logout'));
  await page.getByRole('button', { name: 'Se déconnecter de Cost Monitor', exact: true }).click();
  expect(new URL((await request).url()).searchParams.has('id_token_hint')).toBe(true);
  await expect(page.getByRole('button', { name: 'Se connecter', exact: true })).toBeVisible();
});
