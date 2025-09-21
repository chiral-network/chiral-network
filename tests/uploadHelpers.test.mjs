import test from 'node:test';
import assert from 'node:assert/strict';
import { getStorageStatus, isDuplicateHash } from '../src/lib/uploadHelpers.js';

test('isDuplicateHash returns false when list empty', () => {
  assert.equal(isDuplicateHash([], 'abc'), false);
});

test('isDuplicateHash handles missing hash values', () => {
  const files = [{ id: '1' }, { id: '2', hash: 'xyz' }];
  assert.equal(isDuplicateHash(files, 'xyz'), true);
  assert.equal(isDuplicateHash(files, 'abc'), false);
});

test('isDuplicateHash ignores non-array inputs', () => {
  assert.equal(isDuplicateHash(undefined, 'abc'), false);
  assert.equal(isDuplicateHash(null, 'abc'), false);
});

test('isDuplicateHash ignores empty hash', () => {
  const files = [{ hash: 'value' }];
  assert.equal(isDuplicateHash(files, ''), false);
});

test('getStorageStatus returns unknown for invalid input', () => {
  assert.equal(getStorageStatus(undefined), 'unknown');
  assert.equal(getStorageStatus(NaN), 'unknown');
  assert.equal(getStorageStatus(-1), 'unknown');
});

test('getStorageStatus flags low storage when below threshold', () => {
  assert.equal(getStorageStatus(4.9, 5), 'low');
  assert.equal(getStorageStatus(10, 11), 'low');
});

test('getStorageStatus reports ok when sufficient space', () => {
  assert.equal(getStorageStatus(10, 5), 'ok');
  assert.equal(getStorageStatus(5, 5), 'ok');
});
