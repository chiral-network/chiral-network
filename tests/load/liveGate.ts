import { describe } from "vitest";

export const LIVE_LOAD_TESTS_ENV = "CHIRAL_RUN_LIVE_LOAD_TESTS";

type Env = Record<string, string | undefined>;
type SuiteFactory = () => void;

function isTruthy(value: string | undefined): boolean {
  return ["1", "true", "yes", "on"].includes(value?.trim().toLowerCase() ?? "");
}

export function liveLoadTestsEnabled(env: Env = process.env): boolean {
  return isTruthy(env[LIVE_LOAD_TESTS_ENV]);
}

export function liveLoadSkipTitle(title: string): string {
  return `${title} (skipped: set ${LIVE_LOAD_TESTS_ENV}=1 to run live relay/chain load tests)`;
}

export function describeLiveLoad(title: string, factory: SuiteFactory) {
  const enabled = liveLoadTestsEnabled();
  return (enabled ? describe : describe.skip)(
    enabled ? title : liveLoadSkipTitle(title),
    factory,
  );
}
