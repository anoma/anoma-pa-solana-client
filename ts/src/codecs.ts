// Hex byte-array codecs. The WASM ARM library emits `[u8; 32]` as hex strings;
// the rest of the Solana stack uses base58 or raw bytes. These helpers convert
// at the boundary.
//
// `Uint8Array<ArrayBuffer>` (rather than `Uint8Array<ArrayBufferLike>`) is the
// narrower return type expected by integrators on TS 5.7+ in strict mode.

/** Convert a hex string (with optional `0x` prefix) to a Uint8Array. */
export function fromHex(hex: string): Uint8Array<ArrayBuffer> {
  const stripped = hex.startsWith("0x") || hex.startsWith("0X") ? hex.slice(2) : hex;
  if (stripped.length % 2 !== 0) {
    throw new Error(`hex string has odd length: ${hex}`);
  }
  const out = new Uint8Array(new ArrayBuffer(stripped.length / 2));
  for (let i = 0; i < out.length; i++) {
    const byte = parseInt(stripped.slice(i * 2, i * 2 + 2), 16);
    if (Number.isNaN(byte)) {
      throw new Error(`invalid hex byte at index ${i}: ${hex}`);
    }
    out[i] = byte;
  }
  return out;
}

/** Convert a byte array to a `0x`-prefixed lowercase hex string. */
export function toHex(bytes: Uint8Array<ArrayBufferLike>): string {
  let out = "0x";
  for (const b of bytes) {
    out += b.toString(16).padStart(2, "0");
  }
  return out;
}
