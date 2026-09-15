import { describe, expect, it } from "vitest";

import { decodePaState, PA_STATE_SCHEMA_VERSION, PAStateDecodeError } from "./accounts.js";

// V2 `PAStateAccount` layout (state.rs): schema_version first, then bump,
// authority, verifier_router, proof_selector, kind_table_commitment,
// pending_authority, lifecycle, root, next_index, current_depth, frontier,
// min/max expiry. Byte-identical to the Rust crate's test fixture.
function buildFixture(pendingSome: boolean, schemaVersion = PA_STATE_SCHEMA_VERSION): Uint8Array {
  const parts: number[] = [];
  const push = (...bytes: number[]) => parts.push(...bytes);
  const u64le = (v: bigint) => {
    const b = new Uint8Array(8);
    new DataView(b.buffer).setBigUint64(0, v, true);
    push(...b);
  };
  push(...new Array(8).fill(9)); // discriminator
  push(schemaVersion); // schema_version
  push(255); // bump
  push(...new Array(32).fill(1)); // authority
  push(...new Array(32).fill(2)); // verifier_router
  push(0xab, 0xcd, 0xef, 0x12); // proof_selector
  push(...new Array(32).fill(8)); // kind_table_commitment
  if (pendingSome) {
    push(1, ...new Array(32).fill(3));
  } else {
    push(0);
  }
  push(0); // lifecycle = Running
  push(...new Array(32).fill(4)); // root
  u64le(100n); // next_index
  push(3); // current_depth
  push(3, 0, 0, 0); // frontier len
  push(...new Array(32).fill(5), ...new Array(32).fill(6), ...new Array(32).fill(7));
  u64le(100n); // min_expiry_slots
  u64le(216_000n); // max_expiry_slots
  return Uint8Array.from(parts);
}

describe("decodePaState (V2 layout)", () => {
  it("decodes every field with no pending authority", () => {
    const s = decodePaState(buildFixture(false));
    expect(s.schemaVersion).toBe(PA_STATE_SCHEMA_VERSION);
    expect(s.bump).toBe(255);
    expect(s.authority).toEqual(Uint8Array.from(new Array(32).fill(1)));
    expect(s.proofSelector).toEqual(Uint8Array.from([0xab, 0xcd, 0xef, 0x12]));
    expect(s.kindTableCommitment).toEqual(Uint8Array.from(new Array(32).fill(8)));
    expect(s.pendingAuthority).toBeNull();
    expect(s.root).toEqual(Uint8Array.from(new Array(32).fill(4)));
    expect(s.nextIndex).toBe(100n);
    expect(s.currentDepth).toBe(3);
    expect(s.frontier).toHaveLength(3);
    expect(s.maxExpirySlots).toBe(216_000n);
  });

  it("decodes a pending authority", () => {
    const s = decodePaState(buildFixture(true));
    expect(s.pendingAuthority).toEqual(Uint8Array.from(new Array(32).fill(3)));
  });

  it("rejects an invalid Option tag", () => {
    const data = buildFixture(false);
    // pending_authority tag at offset 8+1+1+32+32+4+32 = 110
    data[110] = 2;
    expect(() => decodePaState(data)).toThrow(PAStateDecodeError);
    expect(() => decodePaState(data)).toThrow(/invalid Option tag 2/);
  });

  it("rejects an unsupported schema version", () => {
    // The PA refuses every instruction on an account whose layout number is
    // not its own; a client reading another layout would misparse every field
    // after byte 8, so it must refuse too.
    expect(() => decodePaState(buildFixture(false, 2))).toThrow(/schema version 2/);
  });

  it("rejects truncated data", () => {
    expect(() => decodePaState(buildFixture(false).slice(0, 50))).toThrow(/truncated/);
  });
});
