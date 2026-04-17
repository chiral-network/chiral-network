import { describe, expect, it } from 'vitest';
import { sortSeeders, BASE_ELO } from '../src/lib/utils/seederSort';

type S = { peerId: string; priceWei: string };

/// Build a Map<peerId, elo> helper so each test can specify reputation per row.
function mkGetElo(elos: Record<string, number>) {
  return (s: S) => elos[s.peerId] ?? BASE_ELO;
}

describe('sortSeeders', () => {
  const a: S = { peerId: 'A', priceWei: '100' };
  const b: S = { peerId: 'B', priceWei: '50' };
  const c: S = { peerId: 'C', priceWei: '200' };

  it('does not mutate the input array', () => {
    const input = [a, b, c];
    const snapshot = [...input];
    sortSeeders(input, 'best', mkGetElo({ A: 60, B: 40, C: 80 }));
    expect(input).toEqual(snapshot);
  });

  it('best: ranks by Elo desc with price tiebreaker (cheaper wins)', () => {
    const elo = mkGetElo({ A: 80, B: 80, C: 70 });
    // A and B tie at Elo 80 → cheaper price (B) wins the tie. C trails.
    expect(sortSeeders([a, b, c], 'best', elo).map((s) => s.peerId)).toEqual(['B', 'A', 'C']);
  });

  it('best: Elo dominates price', () => {
    // C has highest Elo but highest price — should still come first.
    const elo = mkGetElo({ A: 50, B: 50, C: 99 });
    expect(sortSeeders([a, b, c], 'best', elo).map((s) => s.peerId)).toEqual(['C', 'B', 'A']);
  });

  it('elo: pure Elo desc, price ignored', () => {
    const elo = mkGetElo({ A: 70, B: 90, C: 80 });
    expect(sortSeeders([a, b, c], 'elo', elo).map((s) => s.peerId)).toEqual(['B', 'C', 'A']);
  });

  it('price: cheapest first, Elo tiebreaker', () => {
    // B=50, A=100, C=200. No Elo tie.
    const elo = mkGetElo({ A: 50, B: 50, C: 50 });
    expect(sortSeeders([a, b, c], 'price', elo).map((s) => s.peerId)).toEqual(['B', 'A', 'C']);
  });

  it('price: ties broken by higher Elo', () => {
    const x = { peerId: 'X', priceWei: '100' };
    const y = { peerId: 'Y', priceWei: '100' };
    // Same price → Y (higher Elo) wins tiebreak
    const elo = mkGetElo({ X: 40, Y: 70 });
    expect(sortSeeders([x, y], 'price', elo).map((s) => s.peerId)).toEqual(['Y', 'X']);
  });

  it('handles empty priceWei gracefully (treated as 0)', () => {
    const free = { peerId: 'FREE', priceWei: '' };
    const paid = { peerId: 'PAID', priceWei: '100' };
    const elo = mkGetElo({ FREE: 50, PAID: 50 });
    expect(sortSeeders([paid, free], 'price', elo).map((s) => s.peerId)).toEqual(['FREE', 'PAID']);
  });

  it('handles non-numeric priceWei without crashing (treats as 0)', () => {
    const bad = { peerId: 'BAD', priceWei: 'not-a-number' };
    const good = { peerId: 'GOOD', priceWei: '100' };
    const elo = mkGetElo({ BAD: 50, GOOD: 50 });
    expect(sortSeeders([good, bad], 'price', elo).map((s) => s.peerId)).toEqual(['BAD', 'GOOD']);
  });

  it('handles very large priceWei values beyond Number.MAX_SAFE_INTEGER', () => {
    // wei values for anything past ~9 CHI exceed Number.MAX_SAFE_INTEGER.
    // This test would fail if we used parseInt/Number instead of BigInt.
    const huge = { peerId: 'HUGE', priceWei: '1000000000000000000000000' }; // 1e24
    const small = { peerId: 'SMALL', priceWei: '1' };
    const elo = mkGetElo({ HUGE: 50, SMALL: 50 });
    expect(sortSeeders([huge, small], 'price', elo).map((s) => s.peerId)).toEqual(['SMALL', 'HUGE']);
  });

  it('returns empty array for empty input', () => {
    expect(sortSeeders([], 'best', mkGetElo({}))).toEqual([]);
  });

  it('single-element list is returned as-is', () => {
    expect(sortSeeders([a], 'best', mkGetElo({ A: 50 })).map((s) => s.peerId)).toEqual(['A']);
  });

  it('falls back to BASE_ELO when getElo returns undefined-adjacent values', () => {
    // Two seeders, no Elo data → both get BASE_ELO, price decides.
    expect(sortSeeders([a, b], 'best', () => BASE_ELO).map((s) => s.peerId)).toEqual(['B', 'A']);
  });
});
