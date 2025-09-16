// Minimal secp256k1 operations using BigInt. Not constant-time.
// Provides scalar multiplication and compressed public key generation.

const P = BigInt('0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEFFFFFC2F')
const N = BigInt('0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141')
const Gx = BigInt('0x79BE667EF9DCBBAC55A06295CE870B07029BFCDB2DCE28D959F2815B16F81798')
const Gy = BigInt('0x483ADA7726A3C4655DA4FBFC0E1108A8FD17B448A68554199C47D08FFB10D4B8')

type Point = { x: bigint, y: bigint } | null // null = point at infinity

function mod(a: bigint, m: bigint = P): bigint { const r = a % m; return r >= 0n ? r : r + m }
function inv(a: bigint, m: bigint = P): bigint {
  // Extended Euclidean Algorithm
  let lm = 1n, hm = 0n, low = mod(a, m), high = m
  while (low > 1n) {
    const r = high / low
    ;[lm, hm] = [hm - lm * r, lm]
    ;[low, high] = [high - low * r, low]
  }
  return mod(lm, m)
}

function isInfinity(p: Point): p is null { return p === null }

function add(p: Point, q: Point): Point {
  if (isInfinity(p)) return q
  if (isInfinity(q)) return p
  if (p!.x === q!.x) {
    if (p!.y !== q!.y) return null
    // point doubling
    const s = mod((3n * p!.x * p!.x) * inv(2n * p!.y))
    const rx = mod(s * s - 2n * p!.x)
    const ry = mod(s * (p!.x - rx) - p!.y)
    return { x: rx, y: ry }
  } else {
    const s = mod((q!.y - p!.y) * inv(q!.x - p!.x))
    const rx = mod(s * s - p!.x - q!.x)
    const ry = mod(s * (p!.x - rx) - p!.y)
    return { x: rx, y: ry }
  }
}

function scalarMult(k: bigint, p: Point = { x: Gx, y: Gy }): Point {
  if (k === 0n || isInfinity(p)) return null
  let n = mod(k, N)
  let Q: Point = null
  let R: Point = p
  while (n > 0n) {
    if (n & 1n) Q = add(Q, R)
    R = add(R, R)
    n >>= 1n
  }
  return Q
}

export function privateKeyToPublicKeyCompressed(priv32: Uint8Array): Uint8Array {
  const k = BigInt('0x' + Array.from(priv32).map(b => b.toString(16).padStart(2, '0')).join(''))
  if (k <= 0n || k >= N) throw new Error('Invalid private key')
  const Pnt = scalarMult(k)
  if (!Pnt) throw new Error('Invalid point')
  const x = Pnt.x
  const y = Pnt.y
  const xHex = x.toString(16).padStart(64, '0')
  const prefix = (y & 1n) === 0n ? 0x02 : 0x03
  const out = new Uint8Array(33)
  out[0] = prefix
  for (let i = 0; i < 32; i++) out[1 + i] = parseInt(xHex.slice(i * 2, i * 2 + 2), 16)
  return out
}

export const CURVE_ORDER_N = N

