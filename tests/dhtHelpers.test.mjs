import test from "node:test";
import assert from "node:assert/strict";

// Copy of function from dhtHelpers.ts for testing
function resetConnectionAttempts(attempts, connectionSuccessful) {
  return connectionSuccessful ? 0 : attempts;
}

test("resetConnectionAttempts returns zero on success", () => {
  const result = resetConnectionAttempts(5, true);
  assert.equal(result, 0);
});

test("resetConnectionAttempts keeps attempts on failure", () => {
  const result = resetConnectionAttempts(3, false);
  assert.equal(result, 3);
});
