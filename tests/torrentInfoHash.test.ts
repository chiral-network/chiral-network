import { describe, it, expect } from "vitest";
import { extractInfoHashFromTorrentBytes } from "../src/lib/utils/torrentInfoHash";
import { createHash } from "node:crypto";

describe("extractInfoHashFromTorrentBytes", () => {
  it("should extract the infohash by hashing the raw bencoded info dict", async () => {
    // Minimal valid torrent: root dict containing only "info".
    const infoDict = "d4:name8:testfile6:lengthi123e12:piece lengthi16384e6:pieces20:aaaaaaaaaaaaaaaaaaaae";
    const torrent = `d4:info${infoDict}e`;

    const bytes = new TextEncoder().encode(torrent);

    // info dict starts right after "d4:info"
    const prefixLen = "d4:info".length;
    const infoBytes = bytes.subarray(prefixLen, prefixLen + infoDict.length);
    const expected = createHash("sha1").update(infoBytes).digest("hex");

    const actual = await extractInfoHashFromTorrentBytes(bytes);
    expect(actual).toBe(expected);
  });

  it("should throw on non-bencoded input", async () => {
    await expect(extractInfoHashFromTorrentBytes(new TextEncoder().encode("hello"))).rejects.toThrow(
      /expected bencoded dict/i
    );
  });
});


