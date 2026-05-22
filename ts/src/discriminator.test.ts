import { describe, expect, it } from "vitest";

import { anchorEventDisc, anchorInstructionDisc } from "./discriminator.js";

describe("anchor discriminator", () => {
  it("matches the PA's forward_call constant", () => {
    // PA's external_calls/cpi.rs uses 0x9faae00afd696cde as the forward_call discriminator.
    expect(Array.from(anchorInstructionDisc("forward_call"))).toEqual([
      0x9f, 0xaa, 0xe0, 0x0a, 0xfd, 0x69, 0x6c, 0xde,
    ]);
  });

  it("is deterministic", () => {
    expect(anchorInstructionDisc("settle")).toEqual(anchorInstructionDisc("settle"));
  });

  it("differs between instruction and event namespaces", () => {
    expect(anchorInstructionDisc("settle")).not.toEqual(anchorEventDisc("settle"));
  });
});
