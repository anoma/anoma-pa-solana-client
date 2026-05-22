import { describe, expect, it } from "vitest";

import { fromHex, toHex } from "./codecs.js";

describe("hex codecs", () => {
  it("round-trips arbitrary bytes", () => {
    const bytes = new Uint8Array([0x00, 0x01, 0xab, 0xcd, 0xff]);
    expect(fromHex(toHex(bytes))).toEqual(bytes);
  });

  it("accepts hex with and without 0x prefix", () => {
    expect(fromHex("0xdeadbeef")).toEqual(new Uint8Array([0xde, 0xad, 0xbe, 0xef]));
    expect(fromHex("deadbeef")).toEqual(new Uint8Array([0xde, 0xad, 0xbe, 0xef]));
  });

  it("rejects odd-length hex", () => {
    expect(() => fromHex("abc")).toThrow();
  });

  it("rejects non-hex characters", () => {
    expect(() => fromHex("zz")).toThrow();
  });
});
