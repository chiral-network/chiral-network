export interface HashedFileLike {
  hash?: string;
}

export declare function isDuplicateHash(files: HashedFileLike[] | undefined | null, hash: string): boolean;
export declare function getStorageStatus(freeGb: number | null | undefined, thresholdGb?: number): "unknown" | "ok" | "low";
