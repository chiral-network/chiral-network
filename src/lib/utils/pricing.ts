export function bytesToMb(bytes: number): number {
  return bytes / (1024 * 1024);
}

export function costFromPricePerMb(options: {
  bytes: number;
  pricePerMb: number;
  minimumCost?: number;
}): number {
  const { bytes, pricePerMb, minimumCost = 0.0001 } = options;
  const mb = bytesToMb(bytes);
  const raw = mb * pricePerMb;
  const finalCost = Math.max(raw, minimumCost);
  return Number(finalCost.toFixed(8));
}

export function minPricePerMb(
  values: Array<number | null | undefined>,
): number | null {
  const filtered = values.filter(
    (v): v is number => typeof v === "number" && Number.isFinite(v) && v >= 0,
  );
  if (filtered.length === 0) return null;
  return Math.min(...filtered);
}

export function pickLowestPricePeer<
  T extends { selected: boolean; price_per_mb: number },
>(peers: T[]): T | null {
  const selected = peers.filter(
    (p) => p.selected && Number.isFinite(p.price_per_mb),
  );
  if (selected.length === 0) return null;
  return selected.reduce((best, cur) =>
    cur.price_per_mb < best.price_per_mb ? cur : best,
  );
}

export function weightedTotalCost(options: {
  bytes: number;
  peers: Array<{ selected: boolean; price_per_mb: number; percentage: number }>;
  minimumCost?: number;
}): number {
  const { bytes, peers, minimumCost = 0.0001 } = options;
  const mb = bytesToMb(bytes);
  const selected = peers.filter((p) => p.selected);
  if (selected.length === 0) return 0;

  const raw = selected.reduce((sum, p) => {
    const weight = (p.percentage || 0) / 100;
    return sum + mb * p.price_per_mb * weight;
  }, 0);

  return Number(Math.max(raw, minimumCost).toFixed(8));
}
