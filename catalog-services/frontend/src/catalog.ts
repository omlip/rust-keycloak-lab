import { apiRequest } from './auth';

export const categories = [
  { value: 'compute', label: 'Calcul' },
  { value: 'storage', label: 'Stockage' },
  { value: 'backup', label: 'Sauvegarde' },
  { value: 'hosting', label: 'Hébergement' },
  { value: 'network', label: 'Réseau' },
] as const;

export type Category = typeof categories[number]['value'];

export interface ServiceInput {
  name: string;
  category: Category;
  description: string;
  monthly_price_cents: number;
}

export interface DatacenterService extends ServiceInput {
  id: number;
}

const servicesUrl = '/api/catalog/services';

export function listServices() {
  return apiRequest<DatacenterService[]>(servicesUrl);
}

export function saveService(input: ServiceInput, id: number | null) {
  return apiRequest<DatacenterService>(id === null ? servicesUrl : `${servicesUrl}/${id}`, {
    method: id === null ? 'POST' : 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(input),
  });
}

export function deleteService(id: number) {
  return apiRequest<void>(`${servicesUrl}/${id}`, { method: 'DELETE' });
}

export function categoryLabel(category: Category) {
  return categories.find(item => item.value === category)?.label ?? category;
}
