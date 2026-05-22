import { describe, expect, it } from "vitest";

import {
  base64WrapDigest,
  buildWrapMessage,
  WRAP_MESSAGE_LEN,
} from "./wrapMessage.js";

const fixture = () => ({
  forwarderId: new Uint8Array(32).fill(1),
  tokenMint: new Uint8Array(32).fill(2),
  amount: 1_000_000n,
  nonce: 7n,
  deadline: 1_700_000_000n,
  actionTreeRoot: new Uint8Array(32).fill(3),
});

describe("WrapMessage", () => {
  it("serializes to 120 bytes", () => {
    expect(buildWrapMessage(fixture()).length).toBe(WRAP_MESSAGE_LEN);
  });

  it("places fields at the expected offsets", () => {
    const bytes = buildWrapMessage(fixture());
    expect(Array.from(bytes.slice(0, 32))).toEqual(Array(32).fill(1));
    expect(Array.from(bytes.slice(32, 64))).toEqual(Array(32).fill(2));
    expect(new DataView(bytes.buffer).getBigUint64(64, true)).toBe(1_000_000n);
    expect(new DataView(bytes.buffer).getBigUint64(72, true)).toBe(7n);
    expect(new DataView(bytes.buffer).getBigInt64(80, true)).toBe(1_700_000_000n);
    expect(Array.from(bytes.slice(88, 120))).toEqual(Array(32).fill(3));
  });

  it("serializes negative deadlines correctly (i64, not u64)", () => {
    const bytes = buildWrapMessage({ ...fixture(), deadline: -1n });
    // -1 as i64 LE is all 0xFF
    expect(Array.from(bytes.slice(80, 88))).toEqual(Array(8).fill(0xff));
  });

  it("rejects amounts above u64::MAX", () => {
    expect(() =>
      buildWrapMessage({ ...fixture(), amount: 1n << 64n }),
    ).toThrow(RangeError);
  });

  it("base64 digest is 44 characters", () => {
    expect(base64WrapDigest(fixture()).length).toBe(44);
  });
});
