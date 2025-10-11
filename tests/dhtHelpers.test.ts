import { describe, it, expect } from "vitest";
import { resetConnectionAttempts } from "../src/lib/dhtHelpers";

describe("resetConnectionAttempts", () => {
  it("returns zero on success", () => {
    const result = resetConnectionAttempts(5, true);
    expect(result).toBe(0);
  });

  it("keeps attempts on failure", () => {
    const result = resetConnectionAttempts(3, false);
    expect(result).toBe(3);
  });
});
