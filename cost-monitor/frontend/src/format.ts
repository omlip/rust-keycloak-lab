// JSON decimal strings and BigInt preserve the Rust u64 cent values exactly.
export function money(cents: string | bigint): string {
  const value = BigInt(cents);
  return `${value / 100n},${String(value % 100n).padStart(2, '0')} €`;
}

export function total(amounts: string[]): string {
  const value = amounts.reduce((sum, amount) => sum + BigInt(amount), 0n);
  return value <= 18446744073709551615n ? money(value) : 'Montant total non représentable';
}

export function decimal(cents: string | null): string {
  return cents === null ? '' : money(cents).replace(' €', '').replace(',', '.');
}

export function initials(name: string): string {
  const words = name.split(/\s+/u).map(word => Array.from(word).filter(c => /[\p{L}\p{N}]/u.test(c)))
    .filter(word => word.length > 0);
  const letters = words.length > 1 ? [words[0][0], words[words.length - 1][0]] : (words[0] ?? []).slice(0, 2);
  return Array.from(letters.join('').toUpperCase()).slice(0, 2).join('') || '?';
}
