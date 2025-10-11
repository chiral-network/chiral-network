import { describe, it, expect } from "vitest";
import {
  isDuplicateHash,
  getStorageStatus,
  type HashedFileLike,
} from "../src/lib/uploadHelpers";

describe("Upload Helpers", () => {
  describe("isDuplicateHash", () => {
    it("returns false when list empty", () => {
      expect(isDuplicateHash([], "abc")).toBe(false);
    });

    it("handles missing hash values", () => {
      const files: HashedFileLike[] = [{}, { hash: "xyz" }];
      expect(isDuplicateHash(files, "xyz")).toBe(true);
      expect(isDuplicateHash(files, "abc")).toBe(false);
    });

    it("ignores non-array inputs", () => {
      expect(isDuplicateHash(undefined, "abc")).toBe(false);
      expect(isDuplicateHash(null, "abc")).toBe(false);
    });

    it("ignores empty hash", () => {
      const files: HashedFileLike[] = [{ hash: "value" }];
      expect(isDuplicateHash(files, "")).toBe(false);
    });
  });

  describe("getStorageStatus", () => {
    it("returns unknown for invalid input", () => {
      expect(getStorageStatus(undefined)).toBe("unknown");
      expect(getStorageStatus(NaN)).toBe("unknown");
      expect(getStorageStatus(-1)).toBe("unknown");
    });

    it("flags low storage when below threshold", () => {
      expect(getStorageStatus(4.9, 5)).toBe("low");
      expect(getStorageStatus(10, 11)).toBe("low");
    });

    it("reports ok when sufficient space", () => {
      expect(getStorageStatus(10, 5)).toBe("ok");
      expect(getStorageStatus(5, 5)).toBe("ok");
    });
  });
});
