import { describe, expect, it } from "vitest";

import { Cursor } from "./cursor.js";

describe("Cursor.u256Le", () => {
  it("reads 32 little-endian bytes, the least significant first", () => {
    const bytes = new Uint8Array(32);
    bytes.set([0x04, 0x03, 0x02, 0x01]);
    bytes[31] = 0x80;
    expect(new Cursor(bytes, 0).u256Le("index")).toBe(0x01020304n + (0x80n << 248n));
  });

  it("refuses fewer than 32 bytes", () => {
    expect(() => new Cursor(new Uint8Array(31), 0).u256Le("index")).toThrow(/index/);
  });
});
