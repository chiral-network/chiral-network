import { privateKeyToPublicKeyCompressed, CURVE_ORDER_N } from './secp256k1'

function bigIntFromBytes(b: Uint8Array): bigint {
  return BigInt('0x' + Array.from(b).map(x => x.toString(16).padStart(2, '0')).join(''))
}

function bytesFromBigInt(b: bigint, len = 32): Uint8Array {
  const hex = b.toString(16).padStart(len * 2, '0')
  const out = new Uint8Array(len)
  for (let i = 0; i < len; i++) out[i] = parseInt(hex.slice(i * 2, i * 2 + 2), 16)
  return out
}

function concat(a: Uint8Array, b: Uint8Array): Uint8Array {
  const out = new Uint8Array(a.length + b.length)
  out.set(a, 0); out.set(b, a.length)
  return out
}

async function hmacSHA512(key: Uint8Array, data: Uint8Array): Promise<Uint8Array> {
  const cryptoKey = await crypto.subtle.importKey('raw', key, { name: 'HMAC', hash: 'SHA-512' }, false, ['sign'])
  const sig = await crypto.subtle.sign('HMAC', cryptoKey, data)
  return new Uint8Array(sig)
}

export type HDNode = { privateKey: Uint8Array, chainCode: Uint8Array }

export async function fromSeed(seed: Uint8Array): Promise<HDNode> {
  const key = new TextEncoder().encode('Bitcoin seed')
  const I = await hmacSHA512(key, seed)
  const IL = I.slice(0, 32)
  const IR = I.slice(32)
  const k = bigIntFromBytes(IL)
  if (k === 0n || k >= CURVE_ORDER_N) throw new Error('Invalid master key')
  return { privateKey: IL, chainCode: IR }
}

function ser32(i: number): Uint8Array {
  const buf = new Uint8Array(4)
  const dv = new DataView(buf.buffer)
  dv.setUint32(0, i, false)
  return buf
}

function isHardened(idx: number): boolean { return (idx & 0x80000000) !== 0 }

export async function ckdPriv(parent: HDNode, index: number): Promise<HDNode> {
  const hard = isHardened(index)
  let data: Uint8Array
  if (hard) {
    data = concat(new Uint8Array([0x00]), parent.privateKey)
  } else {
    const pub = privateKeyToPublicKeyCompressed(parent.privateKey)
    data = pub
  }
  data = concat(data, ser32(index))
  const I = await hmacSHA512(parent.chainCode, data)
  const IL = I.slice(0, 32)
  const IR = I.slice(32)
  const ilNum = bigIntFromBytes(IL)
  if (ilNum >= CURVE_ORDER_N) throw new Error('Invalid child key')
  const pkNum = bigIntFromBytes(parent.privateKey)
  const childNum = (ilNum + pkNum) % CURVE_ORDER_N
  if (childNum === 0n) throw new Error('Invalid derived key (zero)')
  return { privateKey: bytesFromBigInt(childNum, 32), chainCode: IR }
}

export function parsePath(path: string): number[] {
  if (!path || path[0] !== 'm') throw new Error('Invalid path')
  if (path === 'm') return []
  return path.split('/').slice(1).map(seg => {
    const hardened = seg.endsWith("'")
    const n = parseInt(hardened ? seg.slice(0, -1) : seg, 10)
    if (!Number.isFinite(n)) throw new Error(`Invalid index: ${seg}`)
    return hardened ? (n | 0x80000000) : n
  })
}

export async function derivePath(root: HDNode, path: string): Promise<HDNode> {
  let node = root
  for (const idx of parsePath(path)) {
    node = await ckdPriv(node, idx)
  }
  return node
}

