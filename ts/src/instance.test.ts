import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { sha256 } from "@noble/hashes/sha2";
import {
  AccountRole,
  address,
  createSolanaRpc,
  getAddressDecoder,
  isSignerRole,
  isWritableRole,
  type AccountMeta,
  type Address,
  type Instruction,
} from "@solana/kit";
import { describe, expect, it } from "vitest";

import { coder } from "./coder.js";
import { MOCK_SELECTOR, PADDING_LEAF, TXDATA_CHUNK_SIZE } from "./constants.js";
import type { DeploymentRecord } from "./deployments.js";
import { idl } from "./idl.js";
import {
  capacity,
  emptySubtreeRoots,
  InstanceError,
  isLogicRefDenied,
  ProtocolAdapter,
  recordedDeployment,
  returnedBool,
  type DeniedLogicRef,
  type SettlementAccounts,
} from "./instance.js";

const DEVNET_GENESIS = "EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG";
const MAINNET_GENESIS = "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d";
const ZERO_ADDRESS = address("11111111111111111111111111111111");

/** The address of 32 bytes `n`, as the Rust crate's tests' `key(n)`. */
const key = (n: number): Address =>
  getAddressDecoder().decode(new Uint8Array(32).fill(n));

/** What `f` throws, or a failure when it returns. */
function thrown(f: () => unknown): InstanceError {
  try {
    f();
  } catch (e) {
    if (e instanceof InstanceError) return e;
    throw e;
  }
  throw new Error("returned a deployment");
}

describe("recordedDeployment", () => {
  it("finds the recorded staging devnet adapter not deployed yet", () => {
    const e = thrown(() => recordedDeployment("staging", DEVNET_GENESIS));
    console.log(e.message);
    expect(e.detail).toEqual({
      kind: "NoDeployment",
      chainId: "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1",
    });
  });

  it("finds production recording nothing on mainnet-beta", () => {
    const e = thrown(() => recordedDeployment("production", MAINNET_GENESIS));
    console.log(e.message);
    expect(e.detail).toEqual({
      kind: "NoDeployment",
      chainId: "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp",
    });
  });

  // A cluster no environment records, such as a local validator, is no
  // deployment, under the chain id its genesis hash gives.
  it("finds a genesis hash of no recorded cluster no deployment on its chain", () => {
    for (const environment of ["staging", "production"] as const) {
      const e = thrown(() => recordedDeployment(environment, ZERO_ADDRESS));
      console.log(`${environment}: ${e.message}`);
      expect(e.detail).toEqual({
        kind: "NoDeployment",
        chainId: "solana:11111111111111111111111111111111",
      });
    }
  });

  it("is the cluster's deployed entry", () => {
    const record: DeploymentRecord = {
      staging: [
        {
          chainId: "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1",
          programId: "2gH5DnTwDLLFk3oYxnbrceSFdTMtvaxQz29x5W8Etmpx",
          deploymentSlot: 4242,
        },
      ],
      production: [],
    };
    const found = recordedDeployment("staging", DEVNET_GENESIS, record);
    console.log(found);
    expect(found).toEqual({
      chainId: "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1",
      programId: address(idl.address),
      deploymentSlot: 4242,
    });
    expect(
      thrown(() => recordedDeployment("production", DEVNET_GENESIS, record))
        .detail.kind,
    ).toBe("NoDeployment");
  });
});

describe("the reads computed from the state", () => {
  it("gives a tree the capacity two to its depth", () => {
    for (const depth of [1, 2, 3, 31, 32, 33]) {
      console.log(`depth ${depth}: capacity ${capacity(depth)}`);
      expect(capacity(depth)).toBe(2n ** BigInt(depth));
    }
  });

  it('has the padding leaf sha256("EMPTY"), as arm-risc0 and pa-evm do', () => {
    expect(PADDING_LEAF).toEqual(sha256(new TextEncoder().encode("EMPTY")));
  });

  // Each height's empty subtree root, up to the depth: the padding leaf, then
  // each the hash of the one below with itself.
  it("gives the empty subtree roots of every height up to the depth", () => {
    for (const depth of [1, 2, 5, 32, 33]) {
      const roots = emptySubtreeRoots(depth);
      console.log(`depth ${depth}: ${roots.length} roots`);
      expect(roots.length).toBe(depth + 1);
      expect(roots[0]).toEqual(Array.from(PADDING_LEAF));
      for (let h = 1; h <= depth; h++) {
        expect(roots[h], `height ${h}`).toEqual(
          Array.from(
            sha256(Uint8Array.from([...roots[h - 1]!, ...roots[h - 1]!])),
          ),
        );
      }
    }
  });

  it("denies a logic ref on the side whose list holds it", () => {
    const state = {
      denied_consumed_logic_refs: [new Array(32).fill(0xdd)],
      denied_created_logic_refs: [
        new Array(32).fill(0xee),
        new Array(32).fill(0xdd),
      ],
    };
    for (const [byte, consumed, denied] of [
      [0xdd, true, true],
      [0xdd, false, true],
      [0xee, true, false],
      [0xee, false, true],
      [0x01, true, false],
      [0x01, false, false],
    ] as const) {
      expect(
        isLogicRefDenied(state, new Uint8Array(32).fill(byte), consumed),
        `${byte} consumed ${consumed}`,
      ).toBe(denied);
    }
  });

  it("reads the bool the program returned", () => {
    const program = key(1);
    const data = (bytes: number[], from = program) => ({
      programId: from,
      data: [Buffer.from(bytes).toString("base64"), "base64"] as const,
    });
    expect(returnedBool(program, data([1]))).toBe(true);
    expect(returnedBool(program, data([0]))).toBe(false);
    for (const [what, returned] of [
      ["nothing", null],
      ["another program's", data([1], key(2))],
      ["a byte 2", data([2])],
      ["two bytes", data([1, 0])],
      ["no bytes", data([])],
    ] as const) {
      const e = thrown(() => returnedBool(program, returned));
      console.log(`${what}: ${e.message}`);
      expect(e.detail.kind, what).toBe("Decode");
    }
  });
});

describe("ProtocolAdapter's reads", () => {
  // Nothing listens on port 1, so every request fails.
  it("fail with Rpc when the RPC cannot answer, naming the account", async () => {
    const unreachable = ProtocolAdapter.at(
      createSolanaRpc("http://127.0.0.1:1"),
      key(9),
    );
    for (const [what, read] of [
      ["the adapter's state", () => unreachable.paused()],
      [
        "the nullifier marker",
        () => unreachable.isNullifierContained(new Uint8Array(32)),
      ],
    ] as const) {
      const e = await read().catch((error: unknown) => error);
      console.log(`${what}: ${String(e)}`);
      expect(e, what).toBeInstanceOf(InstanceError);
      expect(e, what).toMatchObject({
        detail: {
          kind: "Rpc",
          what: expect.stringMatching(new RegExp(`^${what} `)),
        },
      });
    }
  });
});

/** The accounts and data of `ix`, which every builder sets. */
function parts(ix: Instruction): {
  accounts: readonly AccountMeta[];
  data: Uint8Array;
} {
  if (ix.accounts === undefined || ix.data === undefined)
    throw new Error("the instruction has no accounts or data");
  return { accounts: ix.accounts, data: Uint8Array.from(ix.data) };
}

// The inputs of the Rust crate's `tests/instructions_fixture.rs`: the adapter
// `key(9)`, every other account `key(n)`; and inputs larger than 1000 bytes
// (byte i of a large input is i mod 256).
const pa = key(9);
const adapter = ProtocolAdapter.at(createSolanaRpc("http://127.0.0.1:1"), pa);
const encode = (s: string) => new TextEncoder().encode(s);
const large = (n: number) => Uint8Array.from({ length: n }, (_, i) => i % 256);
const entries: DeniedLogicRef[] = [
  { logicRef: new Uint8Array(32).fill(7), consumed: false },
  { logicRef: new Uint8Array(32).fill(8), consumed: true },
];
const manyEntries: DeniedLogicRef[] = Array.from({ length: 40 }, (_, i) => ({
  logicRef: new Uint8Array(32).fill(i),
  consumed: i % 2 === 1,
}));
const settlement = (
  newRootMarker: Address | null,
  remaining: boolean,
): SettlementAccounts => ({
  newRootMarker,
  verifierRouterProgram: key(11),
  router: key(12),
  verifierEntry: key(13),
  verifierProgram: key(14),
  remainingAccounts: remaining
    ? [
        { address: key(20), role: AccountRole.WRITABLE },
        { address: key(21), role: AccountRole.READONLY },
      ]
    : [],
});
const U64_MAX = 2n ** 64n - 1n;

/** The fixture entry: an instruction the Rust crate's builder built for the inputs above. */
interface FixtureInstruction {
  name: string;
  program: string;
  data_hex: string;
  accounts: { address: string; signer: boolean; writable: boolean }[];
}

/**
 * `fixtures/instructions_fixture.json`, which the Rust crate's
 * `tests/instructions_fixture.rs` builds and checks with the same inputs.
 */
const FIXTURE = JSON.parse(
  readFileSync(
    fileURLToPath(
      new URL("../../fixtures/instructions_fixture.json", import.meta.url),
    ),
    "utf8",
  ),
) as { instructions: FixtureInstruction[] };

/** `ix` as a fixture entry named `name`. */
function written(name: string, ix: Instruction): FixtureInstruction {
  const { accounts, data } = parts(ix);
  return {
    name,
    program: ix.programAddress,
    data_hex: Buffer.from(data).toString("hex"),
    accounts: accounts.map((m) => ({
      address: m.address,
      signer: isSignerRole(m.role),
      writable: isWritableRole(m.role),
    })),
  };
}

describe("ProtocolAdapter's builders, against the Rust crate's", () => {
  const built: [string, () => Promise<Instruction>][] = [
    [
      "initialize",
      () => adapter.initializeIx(key(1), key(2), key(3), MOCK_SELECTOR),
    ],
    ["pause", () => adapter.pauseIx(key(4))],
    ["unpause", () => adapter.unpauseIx(key(4))],
    ["deny_logic_refs", () => adapter.denyLogicRefsIx(key(4), entries)],
    [
      "deny_logic_refs_large",
      () => adapter.denyLogicRefsIx(key(4), manyEntries),
    ],
    [
      "set_kind_tree_root",
      () => adapter.setKindTreeRootIx(key(4), new Uint8Array(32).fill(7)),
    ],
    ["transfer_ownership", () => adapter.transferOwnershipIx(key(4), key(5))],
    ["renounce_ownership", () => adapter.renounceOwnershipIx(key(4))],
    [
      "update_expiry_config",
      () => adapter.updateExpiryConfigIx(key(4), 100n, U64_MAX),
    ],
    ["upgrade", () => adapter.upgradeIx(key(4), key(6), key(7))],
    [
      "risc_zero_verifier_paused",
      () => adapter.riscZeroVerifierPausedIx(key(3), MOCK_SELECTOR),
    ],
    ["version", () => adapter.versionIx()],
    ["txdata_init", () => adapter.txdataInitIx(key(1), 7n, 900, 500n)],
    [
      "txdata_write",
      () =>
        adapter.txdataWriteIx(
          key(1),
          7n,
          3856,
          Uint8Array.from([1, 2, 3, 4, 5]),
        ),
    ],
    [
      "txdata_write_large",
      () => adapter.txdataWriteIx(key(1), 7n, 0, large(TXDATA_CHUNK_SIZE)),
    ],
    ["txdata_extend", () => adapter.txdataExtendIx(key(1), 7n, 900n)],
    ["txdata_close", () => adapter.txdataCloseIx(key(1), key(2), 7n)],
    [
      "txdata_close_expired",
      () => adapter.txdataCloseExpiredIx(key(3), key(2), key(1), 7n),
    ],
    [
      "settle",
      () =>
        adapter.settleIx(key(1), encode("input"), settlement(key(10), true)),
    ],
    [
      "settle_no_marker",
      () => adapter.settleIx(key(1), encode("input"), settlement(null, false)),
    ],
    [
      "settle_large",
      () => adapter.settleIx(key(1), large(1500), settlement(key(10), true)),
    ],
    [
      "simulate_execute_false",
      () =>
        adapter.simulateExecuteIx(
          key(1),
          encode("input"),
          false,
          settlement(key(10), true),
        ),
    ],
    [
      "simulate_execute_true",
      () =>
        adapter.simulateExecuteIx(
          key(1),
          encode("input"),
          true,
          settlement(null, false),
        ),
    ],
    [
      "simulate_execute_large",
      () =>
        adapter.simulateExecuteIx(
          key(1),
          large(1500),
          true,
          settlement(key(10), true),
        ),
    ],
    [
      "settle_from_txdata",
      () => adapter.settleFromTxdataIx(key(1), 7n, settlement(key(10), true)),
    ],
  ];

  it("builds every instruction the Rust crate does, byte for byte and account for account", async () => {
    expect(
      built.map(([name]) => name),
      "an entry for each of the fixture's",
    ).toEqual(FIXTURE.instructions.map((e) => e.name));
    const covered = new Set<string>();
    for (const [i, [name, build]] of built.entries()) {
      const ix = await build();
      const entry = written(name, ix);
      console.log(entry);
      expect(entry, name).toEqual(FIXTURE.instructions[i]);
      const decoded = coder.instruction.decode(parts(ix).data);
      if (decoded === null)
        throw new Error(`${name}: the coder decodes no instruction`);
      covered.add(decoded.name);
    }
    expect(
      [...covered].sort(),
      "an entry for every instruction the IDL declares",
    ).toEqual(idl.instructions.map((i) => i.name).sort());
  });
});
