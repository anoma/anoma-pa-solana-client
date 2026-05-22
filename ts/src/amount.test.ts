import { describe, expect, it } from "vitest";

import { assertSplTokenAmount, MAX_SPL_TOKEN_AMOUNT } from "./amount.js";

describe("SPL amount guard", () => {
  it("accepts 0 and u64::MAX", () => {
    expect(() => assertSplTokenAmount(0n)).not.toThrow();
    expect(() => assertSplTokenAmount(MAX_SPL_TOKEN_AMOUNT)).not.toThrow();
  });

  it("rejects negative", () => {
    expect(() => assertSplTokenAmount(-1n)).toThrow(RangeError);
  });

  it("rejects values above u64::MAX", () => {
    expect(() => assertSplTokenAmount(MAX_SPL_TOKEN_AMOUNT + 1n)).toThrow(RangeError);
  });

  it("includes the label in the error message", () => {
    expect(() => assertSplTokenAmount(-1n, "wrap amount")).toThrow(/wrap amount/);
  });
});
