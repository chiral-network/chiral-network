import test from 'node:test';
import assert from 'node:assert/strict';
import { resetConnectionAttempts } from '../src/lib/dhtHelpers.js';

test('resetConnectionAttempts returns zero on success', () => {
  const result = resetConnectionAttempts(5, true);
  assert.equal(result, 0);
});

test('resetConnectionAttempts keeps attempts on failure', () => {
  const result = resetConnectionAttempts(3, false);
  assert.equal(result, 3);
});
