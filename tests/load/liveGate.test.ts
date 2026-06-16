import { describe, expect, it } from "vitest";
import {
  LIVE_LOAD_TESTS_ENV,
  liveLoadSkipTitle,
  liveLoadTestsEnabled,
} from "./liveGate";

describe("live load test gate", () => {
  it("is disabled unless the explicit env flag is truthy", () => {
    expect(liveLoadTestsEnabled({})).toBe(false);
    expect(liveLoadTestsEnabled({ [LIVE_LOAD_TESTS_ENV]: "0" })).toBe(false);
    expect(liveLoadTestsEnabled({ [LIVE_LOAD_TESTS_ENV]: "false" })).toBe(false);
    expect(liveLoadTestsEnabled({ [LIVE_LOAD_TESTS_ENV]: "1" })).toBe(true);
    expect(liveLoadTestsEnabled({ [LIVE_LOAD_TESTS_ENV]: "true" })).toBe(true);
    expect(liveLoadTestsEnabled({ [LIVE_LOAD_TESTS_ENV]: "YES" })).toBe(true);
  });

  it("puts the opt-in flag in skipped suite titles", () => {
    expect(liveLoadSkipTitle("Gateway Drive")).toContain(LIVE_LOAD_TESTS_ENV);
  });
});
