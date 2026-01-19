import { describe, it, expect } from "vitest";
import { extractInfoHashFromMagnet } from "../src/lib/utils/magnetInfoHash";

function hexToBytes(hex: string): Uint8Array {
  const clean = hex.toLowerCase();
  const out = new Uint8Array(clean.length / 2);
  for (let i = 0; i < out.length; i++) {
    out[i] = parseInt(clean.slice(i * 2, i * 2 + 2), 16);
  }
  return out;
}

function base32Encode(bytes: Uint8Array): string {
  const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
  let bits = 0;
  let value = 0;
  let out = "";
  for (let i = 0; i < bytes.length; i++) {
    value = (value << 8) | bytes[i]!;
    bits += 8;
    while (bits >= 5) {
      bits -= 5;
      out += alphabet[(value >>> bits) & 31]!;
    }
  }
  if (bits > 0) {
    out += alphabet[(value << (5 - bits)) & 31]!;
  }
  return out;
}

describe("extractInfoHashFromMagnet", () => {
  it("should return hex btih as lowercase", () => {
    const hex = "0123456789abcdef0123456789abcdef01234567";
    const magnet = `magnet:?xt=urn:btih:${hex}&dn=test`;
    expect(extractInfoHashFromMagnet(magnet)).toBe(hex);
  });

  it("should decode base32 btih to hex", () => {
    const hex = "0123456789abcdef0123456789abcdef01234567";
    const base32 = base32Encode(hexToBytes(hex));
    const magnet = `magnet:?xt=urn:btih:${base32}&dn=test`;
    expect(extractInfoHashFromMagnet(magnet)).toBe(hex);
  });

  it("should prefer btih xt if multiple xt params exist", () => {
    const hex = "0123456789abcdef0123456789abcdef01234567";
    const magnet = `magnet:?xt=urn:sha1:deadbeef&xt=urn:btih:${hex}`;
    expect(extractInfoHashFromMagnet(magnet)).toBe(hex);
  });
});


