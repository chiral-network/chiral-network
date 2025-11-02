export type ProxyStatus = "online" | "offline" | "connecting" | "error";

export type WeightInput = {
  latencyMs?: number;      // lower is better
  uptimePct?: number;      // 0..100
  status?: ProxyStatus;
  recentFailures?: number; // optional penalty
};

export function computeProxyWeight(input: WeightInput): number {
  const latencyMs = input.latencyMs ?? 250;
  const uptime = Math.max(0, Math.min(100, input.uptimePct ?? 0)) / 100;

  // Latency score: 1 at 0ms, ~0.5 at 100ms, ~0.2 at 400ms
  const latencyScore = 1 / (1 + latencyMs / 100);

  // Status penalty
  let statusPenalty = 0;
  switch (input.status) {
    case "online": statusPenalty = 0; break;
    case "connecting": statusPenalty = 0.15; break;
    case "offline": statusPenalty = 0.35; break;
    case "error": statusPenalty = 0.5; break;
    default: statusPenalty = 0.25;
  }

  // Recent failure penalty (soft cap)
  const failures = Math.max(0, input.recentFailures ?? 0);
  const failurePenalty = Math.min(0.3, failures * 0.05);

  // Blend reliability and latency; subtract penalties
  let weight = 0.6 * uptime + 0.4 * latencyScore;
  weight = Math.max(0, weight - statusPenalty - failurePenalty);

  return Math.max(0, Math.min(1, weight));
}