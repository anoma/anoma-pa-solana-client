// Cursor over Borsh-encoded bytes, shared by the account and event decoders.
// Walks the schema field by field; no hardcoded offsets.

import { HASH_LEN } from "./constants.js";

export class TruncatedError extends Error {
  constructor(readonly field: string) {
    super(`truncated while reading ${field}`);
    this.name = "TruncatedError";
  }
}

export class Cursor {
  private pos: number;

  constructor(
    private readonly data: Uint8Array,
    start = 0,
  ) {
    this.pos = start;
  }

  /** Bytes not yet consumed. */
  remaining(): number {
    return Math.max(this.data.length - this.pos, 0);
  }

  take(len: number, field: string): Uint8Array {
    if (this.pos + len > this.data.length) {
      throw new TruncatedError(field);
    }
    const slice = this.data.slice(this.pos, this.pos + len);
    this.pos += len;
    return slice;
  }

  u8(field: string): number {
    return this.take(1, field)[0]!;
  }

  u32Le(field: string): number {
    const b = this.take(4, field);
    return new DataView(b.buffer, b.byteOffset, 4).getUint32(0, true);
  }

  u64Le(field: string): bigint {
    const b = this.take(8, field);
    return new DataView(b.buffer, b.byteOffset, 8).getBigUint64(0, true);
  }

  array32(field: string): Uint8Array {
    return this.take(HASH_LEN, field);
  }

  /** Borsh `Vec<u8>`: u32 LE length, then the bytes. */
  vecU8(field: string): Uint8Array {
    return this.take(this.u32Le(field), field);
  }

  /** Borsh `Vec<[u8; 32]>`: u32 LE length, then the arrays. */
  vecArray32(field: string): Uint8Array[] {
    const len = this.u32Le(field);
    const out: Uint8Array[] = [];
    for (let i = 0; i < len; i++) {
      out.push(this.array32(field));
    }
    return out;
  }
}
