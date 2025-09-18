import test from 'node:test';
import assert from 'node:assert/strict';
import { isDuplicateHash } from '../src/lib/uploadHelpers.js';

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
