export interface HashedFileLike {
  hash?: string;
}

export declare function isDuplicateHash(files: HashedFileLike[] | undefined | null, hash: string): boolean;
