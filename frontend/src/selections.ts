import { apiRequest } from './auth';
import type { DatacenterService } from './catalog';

export interface Selection {
  id: number;
  name: string;
  services: DatacenterService[];
  monthly_total_cents: number;
}

const url = '/api/selections';

export function listSelections() { return apiRequest<Selection[]>(url); }
export function getSelection(id: string) { return apiRequest<Selection>(`${url}/${encodeURIComponent(id)}`); }

export function createSelection(name: string) {
  return apiRequest<Selection>(url, {
    method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ name }),
  });
}

export function renameSelection(id: number, name: string) {
  return apiRequest<Selection>(`${url}/${id}`, {
    method: 'PATCH', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ name }),
  });
}

export function deleteSelection(id: number) { return apiRequest<void>(`${url}/${id}`, { method: 'DELETE' }); }

export function setSelectedService(id: number, serviceId: number, selected: boolean) {
  return apiRequest<Selection>(`${url}/${id}/services/${serviceId}`, { method: selected ? 'PUT' : 'DELETE' });
}
