import { accessToken, expireSession } from './auth';

export interface Service {
  name: string;
  monthly_price_cents: string;
}

export interface Selection {
  id: string;
  name: string;
  monthly_total_cents: string;
  threshold_cents: string | null;
  services: Service[];
}

export interface Dashboard {
  name: string;
  connected: boolean;
  has_catalog: boolean;
  recipient: string | null;
  message: string | null;
  selections: Selection[];
}

export class ApiError extends Error {
  constructor(public readonly status: number, message: string) { super(message); }
}

// Only the SPA's ONLINE access token is sent to this API. Offline credentials
// stay in Rust; the browser binding cookie cannot authenticate any API call.
export async function request<T>(path: string, form?: Record<string, string>): Promise<T> {
  const token = await accessToken();
  if (!token) throw new ApiError(401, 'Session expirée. Reconnecte-toi.');
  const response = await fetch(path, {
    method: form ? 'POST' : 'GET',
    credentials: 'same-origin',
    cache: 'no-store',
    headers: { Accept: 'application/json', Authorization: `Bearer ${token}` },
    ...(form ? { body: new URLSearchParams(form) } : {}),
  });
  if (!response.ok) {
    if (response.status === 401) expireSession();
    const body = await response.json().catch(() => null);
    throw new ApiError(response.status, body?.message ?? 'La requête a échoué. Réessaie dans un instant.');
  }
  if (response.status === 204) return undefined as T;
  return response.json() as Promise<T>;
}
