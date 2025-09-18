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
