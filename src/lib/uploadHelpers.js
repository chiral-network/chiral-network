/**
 * Returns true when the provided hash already exists in the list of files.
 * The check is hash-based so duplicate filenames with different content are allowed.
 *
 * @param {Array<{hash?: string}>} files
 * @param {string} hash
 */
export function isDuplicateHash(files, hash) {
  if (!Array.isArray(files) || typeof hash !== 'string' || hash.length === 0) {
    return false;
  }
  return files.some((file) => file && typeof file.hash === 'string' && file.hash === hash);
}

/**
 * Returns a status string describing whether free storage is healthy.
 * @param {number|null|undefined} freeGb
 * @param {number} [thresholdGb=5]
 * @returns {"unknown"|"ok"|"low"}
 */
export function getStorageStatus(freeGb, thresholdGb = 5) {
  if (typeof thresholdGb !== 'number' || !Number.isFinite(thresholdGb) || thresholdGb <= 0) {
    thresholdGb = 5;
  }

  if (typeof freeGb !== 'number' || !Number.isFinite(freeGb) || freeGb < 0) {
    return 'unknown';
  }

  return freeGb < thresholdGb ? 'low' : 'ok';
}
