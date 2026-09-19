import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

import { FORWARDER_PROGRAM_ID, PA_PROGRAM_ID } from "./programIds.js";

describe("program ids", () => {
  it("PA_PROGRAM_ID is the devnet V2 adapter", () => {
    expect(PA_PROGRAM_ID.toBase58()).toBe("28Hvr1YFv2ouGN2fS99aF3ZzYXzkncJVVaHcZNhquLFT");
  });

  it("PA_PROGRAM_ID matches the vendored IDL", () => {
    // The IDL is regenerated from the paired adapter commit; the constant and
    // the IDL's `address` must name the same deployment.
    const idl = JSON.parse(
      readFileSync(fileURLToPath(new URL("../../idl/protocol_adapter.json", import.meta.url)), "utf8"),
    ) as { address: string };
    expect(idl.address).toBe(PA_PROGRAM_ID.toBase58());
  });

  it("FORWARDER_PROGRAM_ID matches the vendored IDL", () => {
    const idl = JSON.parse(
      readFileSync(fileURLToPath(new URL("../../idl/spl_token_forwarder.json", import.meta.url)), "utf8"),
    ) as { address: string };
    expect(idl.address).toBe(FORWARDER_PROGRAM_ID.toBase58());
  });
});
