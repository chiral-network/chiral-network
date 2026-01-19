function bytesToHex(bytes: Uint8Array): string {
  let out = "";
  for (let i = 0; i < bytes.length; i++) {
    out += bytes[i]!.toString(16).padStart(2, "0");
  }
  return out;
}

// RFC 4648 base32 alphabet (used by many btih base32 encodings)
const BASE32_ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
const BASE32_LOOKUP: Record<string, number> = (() => {
  const map: Record<string, number> = {};
  for (let i = 0; i < BASE32_ALPHABET.length; i++) {
    map[BASE32_ALPHABET[i]!] = i;
  }
  return map;
})();

function base32ToBytes(input: string): Uint8Array {
  // Accept upper/lowercase; ignore padding.
  const s = input.toUpperCase().replace(/=+$/g, "");

  let bits = 0;
  let value = 0;
  const out: number[] = [];

  for (let i = 0; i < s.length; i++) {
    const ch = s[i]!;
    const v = BASE32_LOOKUP[ch];
    if (v === undefined) {
      throw new Error(`Invalid base32 character: ${ch}`);
    }
    value = (value << 5) | v;
    bits += 5;
    if (bits >= 8) {
      bits -= 8;
      out.push((value >>> bits) & 0xff);
    }
  }

  return new Uint8Array(out);
}

function normalizeBtihToHex(btih: string): string | null {
  const raw = btih.trim().replace(/^urn:btih:/i, "");
  if (!raw) return null;

  // Hex infohash (v1): 40 hex chars (20 bytes)
  if (/^[a-f0-9]{40}$/i.test(raw)) {
    return raw.toLowerCase();
  }

  // Base32 infohash (v1): typically 32 chars (no padding)
  if (/^[a-z2-7]{32,40}$/i.test(raw)) {
    const bytes = base32ToBytes(raw);
    // v1 infohash must be 20 bytes
    if (bytes.length === 20) {
      return bytesToHex(bytes).toLowerCase();
    }
  }

  return null;
}

/**
 * Extracts v1 infohash (hex) from a magnet URI's `xt=urn:btih:...` parameter.
 * Supports both hex and base32 btih representations.
 */
export function extractInfoHashFromMagnet(magnetUri: string): string | null {
  const idx = magnetUri.indexOf("?");
  if (idx === -1) return null;
  const qs = magnetUri.slice(idx + 1);
  const params = new URLSearchParams(qs);

  // Prefer the btih xt among potentially multiple xt params.
  const xts = params.getAll("xt");
  const xtBtih =
    xts.find((x) => /^urn:btih:/i.test(x)) ?? params.get("xt");
  if (!xtBtih) return null;

  return normalizeBtihToHex(xtBtih);
}


