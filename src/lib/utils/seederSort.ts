/// Ranking helpers for the seeder selector on the Download page.
///
/// Kept standalone (no Svelte imports) so the logic can be unit-tested without
/// mounting a component, and so the same ordering can be reused by any future
/// surface that needs to compare seeders (e.g. a CLI `chiral download` flow).

export type SeederSort = 'best' | 'elo' | 'price';

export interface SortableSeeder {
  peerId: string;
  priceWei: string;
}

/// Baseline Elo used when a seeder has no reputation record yet. Matches the
/// value Download.svelte assigns via `BASE_ELO` so sort fallbacks line up.
export const BASE_ELO = 50;

/// Sort seeders by the chosen mode. Returns a new array — does not mutate
/// the input.
///
/// - `best`: Elo desc, ties broken by cheaper `priceWei`. Matches what a user
///   asking "higher reputation for the lowest cost" would expect.
/// - `elo`:  Elo desc only (price-blind).
/// - `price`: `priceWei` asc, ties broken by higher Elo.
export function sortSeeders<T extends SortableSeeder>(
  list: T[],
  mode: SeederSort,
  getElo: (seeder: T) => number,
): T[] {
  const decorated = list.map((s) => ({
    s,
    elo: getElo(s),
    price: parsePriceWei(s.priceWei),
  }));
  switch (mode) {
    case 'elo':
      decorated.sort((a, b) => b.elo - a.elo);
      break;
    case 'price':
      decorated.sort(
        (a, b) =>
          (a.price < b.price ? -1 : a.price > b.price ? 1 : 0) || b.elo - a.elo,
      );
      break;
    case 'best':
    default:
      decorated.sort(
        (a, b) =>
          b.elo - a.elo ||
          (a.price < b.price ? -1 : a.price > b.price ? 1 : 0),
      );
  }
  return decorated.map((d) => d.s);
}

function parsePriceWei(raw: string | undefined): bigint {
  if (!raw) return 0n;
  try {
    return BigInt(raw);
  } catch {
    return 0n;
  }
}
