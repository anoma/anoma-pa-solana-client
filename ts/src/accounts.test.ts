import { describe, expect, it } from "vitest";

import { decodePaState, PA_STATE_SCHEMA_VERSION, PAStateDecodeError } from "./accounts.js";

// The schema-4 `PAStateAccount` layout (state.rs): schema_version, bump,
// owner, verifier_router, proof_selector, kind_table_commitment, paused, root,
// next_index, current_depth, frontier, min/max expiry, then the denylists for
// consumed and for created resources. Byte-identical to the Rust crate's test
// fixture.
function buildFixture(
  schemaVersion: number,
  paused: number,
  deniedConsumed: number[][],
  deniedCreated: number[][],
): Uint8Array {
  const parts: number[] = [];
  const push = (...bytes: number[]) => parts.push(...bytes);
  const u64le = (v: bigint) => {
    const b = new Uint8Array(8);
    new DataView(b.buffer).setBigUint64(0, v, true);
    push(...b);
  };
  push(...new Array(8).fill(9)); // discriminator
  push(schemaVersion);
  push(255); // bump
  push(...new Array(32).fill(1)); // owner
  push(...new Array(32).fill(2)); // verifier_router
  push(0xab, 0xcd, 0xef, 0x12); // proof_selector
  push(...new Array(32).fill(8)); // kind_table_commitment
  push(paused);
  push(...new Array(32).fill(4)); // root
  u64le(100n); // next_index
  push(3); // current_depth
  push(3, 0, 0, 0); // frontier len
  push(...new Array(32).fill(5), ...new Array(32).fill(6), ...new Array(32).fill(7));
  u64le(100n); // min_expiry_slots
  u64le(216_000n); // max_expiry_slots
  for (const denylist of [deniedConsumed, deniedCreated]) {
    push(denylist.length, 0, 0, 0);
    for (const r of denylist) push(...r);
  }
  return Uint8Array.from(parts);
}

const filled = (n: number) => Uint8Array.from(new Array(32).fill(n));

describe("decodePaState (schema 4)", () => {
  it("decodes every field", () => {
    const s = decodePaState(
      buildFixture(PA_STATE_SCHEMA_VERSION, 1, [new Array(32).fill(0xdd)], [new Array(32).fill(0xee)]),
    );
    expect(s.schemaVersion).toBe(PA_STATE_SCHEMA_VERSION);
    expect(s.bump).toBe(255);
    expect(s.owner).toEqual(filled(1));
    expect(s.verifierRouter).toEqual(filled(2));
    expect(s.proofSelector).toEqual(Uint8Array.from([0xab, 0xcd, 0xef, 0x12]));
    expect(s.kindTableCommitment).toEqual(filled(8));
    expect(s.paused).toBe(true);
    expect(s.root).toEqual(filled(4));
    expect(s.nextIndex).toBe(100n);
    expect(s.currentDepth).toBe(3);
    expect(s.frontier).toEqual([filled(5), filled(6), filled(7)]);
    expect(s.minExpirySlots).toBe(100n);
    expect(s.maxExpirySlots).toBe(216_000n);
    expect(s.deniedConsumedLogicRefs).toEqual([filled(0xdd)]);
    expect(s.deniedCreatedLogicRefs).toEqual([filled(0xee)]);
  });

  it("decodes an unpaused state with no denied refs", () => {
    const s = decodePaState(buildFixture(PA_STATE_SCHEMA_VERSION, 0, [], []));
    expect(s.paused).toBe(false);
    expect(s.deniedConsumedLogicRefs).toEqual([]);
    expect(s.deniedCreatedLogicRefs).toEqual([]);
  });

  it("rejects an invalid paused byte", () => {
    const data = buildFixture(PA_STATE_SCHEMA_VERSION, 2, [], []);
    expect(() => decodePaState(data)).toThrow(PAStateDecodeError);
    expect(() => decodePaState(data)).toThrow(/invalid bool byte 2 for field paused/);
  });

  it("rejects an unsupported schema version", () => {
    // The PA refuses every instruction on an account whose layout number is
    // not its own; a client reading another layout would misparse every field
    // after byte 8, so it must refuse too.
    expect(() => decodePaState(buildFixture(3, 0, [], []))).toThrow(/schema version 3/);
  });

  it("rejects truncated data", () => {
    const data = buildFixture(PA_STATE_SCHEMA_VERSION, 0, [], [new Array(32).fill(0xdd)]);
    expect(() => decodePaState(data.slice(0, data.length - 1))).toThrow(/truncated.*denied_created_logic_refs/);
  });
});
