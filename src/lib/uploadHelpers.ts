export interface HashedFileLike {
  hash?: string;
}

/**
 * Returns true when the provided hash already exists in the list of files.
 * The check is hash-based so duplicate filenames with different content are allowed.
 */
export function isDuplicateHash(
  files: HashedFileLike[] | undefined | null,
  hash: string
): boolean {
  if (!Array.isArray(files) || typeof hash !== "string" || hash.length === 0) {
    return false;
  }
  return files.some(
    (file) => file && typeof file.hash === "string" && file.hash === hash
  );
}

/**
 * Returns a status string describing whether free storage is healthy.
 */
export function getStorageStatus(
  freeGb: number | null | undefined,
  thresholdGb: number = 5
): "unknown" | "ok" | "low" {
  if (
    typeof thresholdGb !== "number" ||
    !Number.isFinite(thresholdGb) ||
    thresholdGb <= 0
  ) {
    thresholdGb = 5;
  }

  if (typeof freeGb !== "number" || !Number.isFinite(freeGb) || freeGb < 0) {
    return "unknown";
  }

  return freeGb < thresholdGb ? "low" : "ok";
}
