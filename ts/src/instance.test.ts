import { sha256 } from "@noble/hashes/sha2";
import {
  AccountRole,
  address,
  createSolanaRpc,
  getAddressDecoder,
  getAddressEncoder,
  getProgramDerivedAddress,
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
import { deriveEventAuthorityPda, deriveVerifierRouterPdas } from "./pda.js";

const DEVNET_GENESIS = "EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG";
const MAINNET_GENESIS = "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d";
const ZERO_ADDRESS = address("11111111111111111111111111111111");

/** The address of 32 bytes `n`, as the Rust crate's tests' `key(n)`. */
const key = (n: number): Address => getAddressDecoder().decode(new Uint8Array(32).fill(n));

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
    expect(e.detail).toEqual({ kind: "NoDeployment", chainId: "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1" });
  });

  it("finds production recording nothing on mainnet-beta", () => {
    const e = thrown(() => recordedDeployment("production", MAINNET_GENESIS));
    console.log(e.message);
    expect(e.detail).toEqual({ kind: "NoDeployment", chainId: "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp" });
  });

  // A cluster no environment records, such as a local validator, is no
  // deployment, under the chain id its genesis hash gives.
  it("finds a genesis hash of no recorded cluster no deployment on its chain", () => {
    for (const environment of ["staging", "production"] as const) {
      const e = thrown(() => recordedDeployment(environment, ZERO_ADDRESS));
      console.log(`${environment}: ${e.message}`);
      expect(e.detail).toEqual({ kind: "NoDeployment", chainId: "solana:11111111111111111111111111111111" });
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
    expect(thrown(() => recordedDeployment("production", DEVNET_GENESIS, record)).detail.kind).toBe("NoDeployment");
  });
});

describe("the reads computed from the state", () => {
  it("gives a tree the capacity two to its depth", () => {
    for (const depth of [1, 2, 3, 31, 32]) {
      console.log(`depth ${depth}: capacity ${capacity(depth)}`);
      expect(capacity(depth)).toBe(2n ** BigInt(depth));
    }
  });

  // Each height's empty subtree root, up to the depth: the padding leaf, then
  // each the hash of the one below with itself.
  it("gives the empty subtree roots of every height up to the depth", () => {
    for (const depth of [1, 2, 5, 32]) {
      const roots = emptySubtreeRoots(depth);
      console.log(`depth ${depth}: ${roots.length} roots`);
      expect(roots.length).toBe(depth + 1);
      expect(roots[0]).toEqual(Array.from(PADDING_LEAF));
      for (let h = 1; h <= depth; h++) {
        expect(roots[h], `height ${h}`).toEqual(Array.from(sha256(Uint8Array.from([...roots[h - 1]!, ...roots[h - 1]!]))));
      }
    }
  });

  it("denies a logic ref on the side whose list holds it", () => {
    const state = {
      denied_consumed_logic_refs: [new Array(32).fill(0xdd)],
      denied_created_logic_refs: [new Array(32).fill(0xee), new Array(32).fill(0xdd)],
    };
    for (const [byte, consumed, denied] of [
      [0xdd, true, true],
      [0xdd, false, true],
      [0xee, true, false],
      [0xee, false, true],
      [0x01, true, false],
      [0x01, false, false],
    ] as const) {
      expect(isLogicRefDenied(state, new Uint8Array(32).fill(byte), consumed), `${byte} consumed ${consumed}`).toBe(
        denied,
      );
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

/** `value`, decoded by the coder, with integers as decimal strings and bytes as number arrays (addresses are strings already). */
function plain(value: unknown): unknown {
  if (typeof value === "bigint") return value.toString();
  if (value instanceof Uint8Array) return Array.from(value);
  if (Array.isArray(value)) return value.map(plain);
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, plain(v)]));
  }
  return value;
}

/** The accounts and data of `ix`, which every builder sets. */
function parts(ix: Instruction): { accounts: readonly AccountMeta[]; data: Uint8Array } {
  if (ix.accounts === undefined || ix.data === undefined) throw new Error("the instruction has no accounts or data");
  return { accounts: ix.accounts as readonly AccountMeta[], data: Uint8Array.from(ix.data) };
}

// The inputs of the Rust crate's `instructions` tests: the adapter `key(9)`,
// every other account `key(n)`; and inputs larger than 1000 bytes (byte i of
// a large input is i mod 256).
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
const settlement = (newRootMarker: Address | null, remaining: boolean): SettlementAccounts => ({
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

describe("ProtocolAdapter's builders", () => {
  const u64le = (n: bigint) => {
    const b = Buffer.alloc(8);
    b.writeBigUInt64LE(n);
    return b;
  };
  const pda = async (...seeds: Uint8Array[]) => (await getProgramDerivedAddress({ programAddress: pa, seeds }))[0];
  const bytesOf = (a: Address) => Uint8Array.from(getAddressEncoder().encode(a));

  it("builds each instruction at the instance's address, as the IDL declares it", async () => {
    const [payer, owner, router, authority, refund, buffer, spill] = [1, 2, 3, 4, 5, 6, 7].map(key);
    const paState = await pda(encode("pa_state"));
    const txData = await pda(encode("tx_data"), bytesOf(authority), u64le(7n));
    const events = { event_authority: (await deriveEventAuthorityPda(pa))[0], program: pa };
    const programData = (
      await getProgramDerivedAddress({
        programAddress: address("BPFLoaderUpgradeab1e11111111111111111111111"),
        seeds: [bytesOf(pa)],
      })
    )[0];
    const verifierEntry = (await deriveVerifierRouterPdas(router, MOCK_SELECTOR)).entry;
    const accounts = settlement(null, true);
    const settlementNamed = {
      pa_state: paState,
      verifier_router_program: accounts.verifierRouterProgram,
      router: accounts.router,
      verifier_entry: accounts.verifierEntry,
      verifier_program: accounts.verifierProgram,
    };
    const marker = key(10);
    const root = new Uint8Array(32).fill(7);
    const input = large(1500);
    const chunk = large(TXDATA_CHUNK_SIZE);

    // Each row: the instruction, the arguments it must decode to, the accounts
    // the caller or the instance's address determine (`null`: an absent
    // optional account), and the remaining accounts. Every other account is the
    // IDL's fixed address or the PDA of its constant seeds.
    const rows: [string, Instruction, object, Record<string, Address | null>, AccountMeta[]][] = [
      [
        "initialize",
        await adapter.initializeIx(payer, owner, router, MOCK_SELECTOR),
        { initial_owner: owner, verifier_router: router, proof_selector: Array.from(MOCK_SELECTOR) },
        { payer, program_data: programData, verifier_entry: verifierEntry, ...events },
        [],
      ],
      ["pause", await adapter.pauseIx(authority), {}, { authority, ...events }, []],
      ["unpause", await adapter.unpauseIx(authority), {}, { authority, ...events }, []],
      [
        "deny_logic_refs",
        await adapter.denyLogicRefsIx(authority, manyEntries),
        { logic_refs: manyEntries.map((r) => ({ logic_ref: Array.from(r.logicRef), consumed: r.consumed })) },
        { authority, ...events },
        [],
      ],
      [
        "transfer_ownership",
        await adapter.transferOwnershipIx(authority, owner),
        { new_owner: owner },
        { authority, ...events },
        [],
      ],
      ["renounce_ownership", await adapter.renounceOwnershipIx(authority), {}, { authority, ...events }, []],
      [
        "set_kind_tree_root",
        await adapter.setKindTreeRootIx(authority, root),
        { new_kind_tree_root: Array.from(root) },
        { authority, ...events },
        [],
      ],
      [
        "update_expiry_config",
        await adapter.updateExpiryConfigIx(authority, 100n, U64_MAX),
        { new_min_expiry_slots: "100", new_max_expiry_slots: U64_MAX.toString() },
        { authority },
        [],
      ],
      [
        "upgrade",
        await adapter.upgradeIx(authority, buffer, spill),
        {},
        { authority, program_data: programData, upgraded_program: pa, buffer, spill, ...events },
        [],
      ],
      ["version", adapter.versionIx(), {}, {}, []],
      [
        "risc_zero_verifier_paused",
        await adapter.riscZeroVerifierPausedIx(router, MOCK_SELECTOR),
        {},
        { verifier_entry: verifierEntry },
        [],
      ],
      [
        "txdata_init",
        await adapter.txdataInitIx(authority, 7n, 900, 500n),
        { upload_id: "7", capacity: 900, expires_slot: "500" },
        { tx_data: txData, authority },
        [],
      ],
      [
        "txdata_write",
        await adapter.txdataWriteIx(authority, 7n, 0, chunk),
        { upload_id: "7", offset: 0, data: Array.from(chunk) },
        { tx_data: txData, authority },
        [],
      ],
      [
        "txdata_extend",
        await adapter.txdataExtendIx(authority, 7n, 900n),
        { upload_id: "7", new_expires_slot: "900" },
        { tx_data: txData, authority },
        [],
      ],
      [
        "txdata_close",
        await adapter.txdataCloseIx(authority, refund, 7n),
        { upload_id: "7" },
        { tx_data: txData, authority, refund },
        [],
      ],
      [
        "txdata_close_expired",
        await adapter.txdataCloseExpiredIx(payer, refund, authority, 7n),
        { upload_id: "7", authority },
        { tx_data: txData, payer, refund },
        [],
      ],
      [
        "settle",
        await adapter.settleIx(payer, input, accounts),
        { transaction_data: Array.from(input) },
        { ...settlementNamed, payer, new_root_marker: null, ...events },
        accounts.remainingAccounts,
      ],
      [
        "simulate_execute",
        await adapter.simulateExecuteIx(payer, input, true, { ...accounts, newRootMarker: marker }),
        { transaction_data: Array.from(input), skip_risc_zero_proof_verification: true },
        { ...settlementNamed, payer, new_root_marker: marker, ...events },
        accounts.remainingAccounts,
      ],
      [
        "settle_from_txdata",
        await adapter.settleFromTxdataIx(authority, 7n, accounts),
        { upload_id: "7" },
        { ...settlementNamed, tx_data: txData, authority, new_root_marker: null, ...events },
        accounts.remainingAccounts,
      ],
      [
        "settle_from_txdata",
        await adapter.settleFromTxdataIx(authority, 7n, { ...accounts, newRootMarker: marker }),
        { upload_id: "7" },
        { ...settlementNamed, tx_data: txData, authority, new_root_marker: marker, ...events },
        accounts.remainingAccounts,
      ],
    ];
    expect(
      [...new Set(rows.map(([name]) => name))].sort(),
      "a row for every instruction the IDL declares",
    ).toEqual(idl.instructions.map((i) => i.name).sort());

    for (const [name, ix, args, named, rest] of rows) {
      const idlIx = idl.instructions.find((i) => i.name === name);
      if (idlIx === undefined) throw new Error(`the IDL declares no instruction ${name}`);
      const { accounts: metas, data } = parts(ix);
      console.log(`${name}: ${metas.length} accounts, ${data.length} data bytes`);
      expect(ix.programAddress, name).toBe(pa);
      expect(Array.from(data.subarray(0, idlIx.discriminator.length)), name).toEqual(idlIx.discriminator);
      const decoded = coder.instruction.decode(data);
      expect(decoded?.name, name).toBe(name);
      expect(plain(decoded?.data), name).toEqual(args);

      expect(metas.length, name).toBe(idlIx.accounts.length + rest.length);
      for (const [i, a] of idlIx.accounts.entries()) {
        if ("accounts" in a) throw new Error(`${name}: account group ${a.name}`);
        const built = metas[i]!;
        const given = named[a.name];
        if (given === null) {
          expect(a.optional, `${name}.${a.name} is optional`).toBe(true);
          expect(built, `${name}.${a.name} absent`).toEqual({ address: pa, role: AccountRole.READONLY });
          continue;
        }
        let expected = given;
        if (expected === undefined && a.address !== undefined) expected = address(a.address);
        if (expected === undefined && a.pda?.seeds.every((seed) => seed.kind === "const")) {
          expected = await pda(...a.pda.seeds.map((seed) => Uint8Array.from(seed.kind === "const" ? seed.value : [])));
        }
        if (expected === undefined) throw new Error(`${name}.${a.name}: no expected address`);
        expect(built.address, `${name}.${a.name}`).toBe(expected);
        expect([isSignerRole(built.role), isWritableRole(built.role)], `${name}.${a.name} flags`).toEqual([
          a.signer ?? false,
          a.writable ?? false,
        ]);
      }
      expect(metas.slice(idlIx.accounts.length), `${name} remaining accounts`).toEqual(rest);
    }
  });
});

/**
 * Each instruction the Rust crate's builders build for the inputs above, as
 * the Rust crate printed it: its program, its data's length and sha256, and
 * its accounts (address, signer, writable) in order. Captured from
 * `anoma-pa-solana-client`'s `instructions::*_ix` at this commit's IDL.
 */
const EXPECTED = `\
== initialize
program cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN
length 76
sha256 fddd37d671b11fa53a83c924ac410f02c9e04172fb17e42d056bd1cb91623bfe
acct Fb88t17apPNxTw4BCCC85yvcybdCvaY41qGJYpWXyrsR false true
acct 4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi true true
acct 11111111111111111111111111111111 false false
acct Hibe1esmNpASGJBynhevaMybmY2gcGcPDrfz8zMZyw1J false true
acct CFLMyAiswuYuTmzEmBT9b9C1T1V6xqvWEfy7DT1T8MEP false false
acct BPFLoaderUpgradeab1e11111111111111111111111 false false
acct AdJWkJknpP3onZQWvkzhfUfz6LxxwbDF1ohYmAV7NtK1 false false
acct BaNhKDjFJTKQhS1To1XK8eApMSYBoMUNky6wKrf4RX5N false false
acct cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN false false
== pause
program cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN
length 8
sha256 f18f7b90a2b7d04148a25e2a878db5c4df9a0a7f81226de6b1945d00f9ed89b7
acct Fb88t17apPNxTw4BCCC85yvcybdCvaY41qGJYpWXyrsR false true
acct GgBaCs3NCBuZN12kCJgAW63ydqohFkHEdfdEXBPzLHq true false
acct BaNhKDjFJTKQhS1To1XK8eApMSYBoMUNky6wKrf4RX5N false false
acct cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN false false
== unpause
program cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN
length 8
sha256 b56b20cd6ad1fdc7fc2b8c1caf08fcaad3aba3069f7dc94daa08c523275f50ca
acct Fb88t17apPNxTw4BCCC85yvcybdCvaY41qGJYpWXyrsR false true
acct GgBaCs3NCBuZN12kCJgAW63ydqohFkHEdfdEXBPzLHq true false
acct BaNhKDjFJTKQhS1To1XK8eApMSYBoMUNky6wKrf4RX5N false false
acct cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN false false
== deny_logic_refs
program cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN
length 78
sha256 633f9c638a475a1165430b1e0a3faf2f6a95c5398f642883fd04094aae8f62e1
acct Fb88t17apPNxTw4BCCC85yvcybdCvaY41qGJYpWXyrsR false true
acct GgBaCs3NCBuZN12kCJgAW63ydqohFkHEdfdEXBPzLHq true true
acct 11111111111111111111111111111111 false false
acct BaNhKDjFJTKQhS1To1XK8eApMSYBoMUNky6wKrf4RX5N false false
acct cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN false false
== deny_logic_refs_large
program cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN
length 1332
sha256 7275c806b58437ebee604f014bd562893f42317de611a3bfa8c7d93e088eee87
acct Fb88t17apPNxTw4BCCC85yvcybdCvaY41qGJYpWXyrsR false true
acct GgBaCs3NCBuZN12kCJgAW63ydqohFkHEdfdEXBPzLHq true true
acct 11111111111111111111111111111111 false false
acct BaNhKDjFJTKQhS1To1XK8eApMSYBoMUNky6wKrf4RX5N false false
acct cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN false false
== set_kind_tree_root
program cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN
length 40
sha256 af9fa72a8c3949b981f118a576598a89ae88be20cb1b02a43835972c825193c5
acct Fb88t17apPNxTw4BCCC85yvcybdCvaY41qGJYpWXyrsR false true
acct GgBaCs3NCBuZN12kCJgAW63ydqohFkHEdfdEXBPzLHq true false
acct BaNhKDjFJTKQhS1To1XK8eApMSYBoMUNky6wKrf4RX5N false false
acct cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN false false
== transfer_ownership
program cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN
length 40
sha256 24580020f7cb8bfa853f9c39dbaa4b9d0ad205ac678e1e2808bffe7f0e93528a
acct Fb88t17apPNxTw4BCCC85yvcybdCvaY41qGJYpWXyrsR false true
acct GgBaCs3NCBuZN12kCJgAW63ydqohFkHEdfdEXBPzLHq true false
acct BaNhKDjFJTKQhS1To1XK8eApMSYBoMUNky6wKrf4RX5N false false
acct cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN false false
== renounce_ownership
program cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN
length 8
sha256 576b3542618e9e95fddb3e160d224aad4fa3760a39fc206fc9a4e2b42122612f
acct Fb88t17apPNxTw4BCCC85yvcybdCvaY41qGJYpWXyrsR false true
acct GgBaCs3NCBuZN12kCJgAW63ydqohFkHEdfdEXBPzLHq true false
acct BaNhKDjFJTKQhS1To1XK8eApMSYBoMUNky6wKrf4RX5N false false
acct cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN false false
== update_expiry_config
program cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN
length 24
sha256 a860aea2272759e32c06dc8cb0563c9c7bc78d6e3822c593d13fa6187b23b32d
acct Fb88t17apPNxTw4BCCC85yvcybdCvaY41qGJYpWXyrsR false true
acct GgBaCs3NCBuZN12kCJgAW63ydqohFkHEdfdEXBPzLHq true false
== upgrade
program cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN
length 8
sha256 a2c26be6f40769ab5368ba4fbb248e82c749e4ab4ed56c2730be9b774cd78b8f
acct Fb88t17apPNxTw4BCCC85yvcybdCvaY41qGJYpWXyrsR false false
acct GgBaCs3NCBuZN12kCJgAW63ydqohFkHEdfdEXBPzLHq true false
acct Hibe1esmNpASGJBynhevaMybmY2gcGcPDrfz8zMZyw1J false true
acct cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN false true
acct QWmroo4YnnMqYW3cnxWkFdaTxGD3P7vMSzwMHGbUzwF false true
acct US517G5965aydkZ46HS38QLi7UQiSojurfbQfKCELFx false true
acct CFLMyAiswuYuTmzEmBT9b9C1T1V6xqvWEfy7DT1T8MEP false false
acct SysvarRent111111111111111111111111111111111 false false
acct SysvarC1ock11111111111111111111111111111111 false false
acct BPFLoaderUpgradeab1e11111111111111111111111 false false
acct BaNhKDjFJTKQhS1To1XK8eApMSYBoMUNky6wKrf4RX5N false false
acct cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN false false
== risc_zero_verifier_paused
program cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN
length 8
sha256 5eb79ab7c555a933ca70d18e8faa34e6974980efcc0ef40bd14a77aa57bf3e92
acct Fb88t17apPNxTw4BCCC85yvcybdCvaY41qGJYpWXyrsR false false
acct AdJWkJknpP3onZQWvkzhfUfz6LxxwbDF1ohYmAV7NtK1 false false
== version
program cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN
length 8
sha256 4fe6a6d48fa7e8f893a2322a3ce1efe87f62277d0cd3a2ed7b32bd68ff2681bd
== txdata_init
program cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN
length 28
sha256 3d744650a047db77a20f11109d1f00fa052d727610a838bb45b778ad479471c5
acct Fb88t17apPNxTw4BCCC85yvcybdCvaY41qGJYpWXyrsR false false
acct DxJbRZapfMrinq8ofVVdfJZ3nmSG277N6wVwnbHdCzMK false true
acct 4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi true true
acct 11111111111111111111111111111111 false false
== txdata_write
program cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN
length 29
sha256 7f727b523380fce9315e4a4f744259b6f135dba46f6f08c84a0953d343c04dd1
acct DxJbRZapfMrinq8ofVVdfJZ3nmSG277N6wVwnbHdCzMK false true
acct 4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi true false
== txdata_write_large
program cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN
length 3880
sha256 2710e3d55d48963d266841d5e32facbc2b33e2b93e2b183d150bad3d1cf3bb09
acct DxJbRZapfMrinq8ofVVdfJZ3nmSG277N6wVwnbHdCzMK false true
acct 4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi true false
== txdata_extend
program cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN
length 24
sha256 4b69f709820c56e26a88144ef6733e04c8c3d6ae5a43783b64c25d486ce7ed8d
acct Fb88t17apPNxTw4BCCC85yvcybdCvaY41qGJYpWXyrsR false false
acct DxJbRZapfMrinq8ofVVdfJZ3nmSG277N6wVwnbHdCzMK false true
acct 4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi true false
== txdata_close
program cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN
length 16
sha256 ccf638f497b81165d07b8c21a028676900368acdfe51243e60116294628ac24a
acct DxJbRZapfMrinq8ofVVdfJZ3nmSG277N6wVwnbHdCzMK false true
acct 4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi true false
acct 8qbHbw2BbbTHBW1sbeqakYXVKRQM8Ne7pLK7m6CVfeR false true
== txdata_close_expired
program cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN
length 48
sha256 c6fe8f1356808dc1ed224771a509959c72078a1fa4c6f464da28c72df953605d
acct DxJbRZapfMrinq8ofVVdfJZ3nmSG277N6wVwnbHdCzMK false true
acct CktRuQ2mttgRGkXJtyksdKHjUdc2C4TgDzyB98oEzy8 true false
acct 8qbHbw2BbbTHBW1sbeqakYXVKRQM8Ne7pLK7m6CVfeR false true
== settle
program cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN
length 17
sha256 73c89628930b0e6950816c282d898ff96e680bd9dd56938cb61d57c1cf66c3dd
acct Fb88t17apPNxTw4BCCC85yvcybdCvaY41qGJYpWXyrsR false true
acct 4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi true true
acct 11111111111111111111111111111111 false false
acct gBxS1f6uyyGPuW5MzGBukidSb71jdsCb5fZaoSzULE5 false true
acct k7FaK87WHGVXzkaoHb7CdVPgkKDQhZ29VLDeBVbDfYn false false
acct p2Yicb86aZig616Eav2VWG9vuXR5mEqhtzshZYBxzsV false false
acct swqrv48gsrwpBFbftEwnP2vB4jckpvfGJfXkwaniLCC false false
acct ws91DX9HBAAxGW77BZs5FogRDwpRtcUpiLBpKdPTfWu false false
acct BaNhKDjFJTKQhS1To1XK8eApMSYBoMUNky6wKrf4RX5N false false
acct cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN false false
acct 2MNus2KCpxwXnp19iyXNpWSFtBD2UGjQBAL8AbtywfT9 false true
acct 2RJD1KnDRGEkvuFfAGrJ7PD28LRE9LRDjZznDywagzmr false false
== settle_no_marker
program cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN
length 17
sha256 73c89628930b0e6950816c282d898ff96e680bd9dd56938cb61d57c1cf66c3dd
acct Fb88t17apPNxTw4BCCC85yvcybdCvaY41qGJYpWXyrsR false true
acct 4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi true true
acct 11111111111111111111111111111111 false false
acct cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN false false
acct k7FaK87WHGVXzkaoHb7CdVPgkKDQhZ29VLDeBVbDfYn false false
acct p2Yicb86aZig616Eav2VWG9vuXR5mEqhtzshZYBxzsV false false
acct swqrv48gsrwpBFbftEwnP2vB4jckpvfGJfXkwaniLCC false false
acct ws91DX9HBAAxGW77BZs5FogRDwpRtcUpiLBpKdPTfWu false false
acct BaNhKDjFJTKQhS1To1XK8eApMSYBoMUNky6wKrf4RX5N false false
acct cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN false false
== settle_large
program cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN
length 1512
sha256 86b1cc03836b4f06b13533ad99aac4de0cd93a5f1bbf523b547e97c2664757c9
acct Fb88t17apPNxTw4BCCC85yvcybdCvaY41qGJYpWXyrsR false true
acct 4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi true true
acct 11111111111111111111111111111111 false false
acct gBxS1f6uyyGPuW5MzGBukidSb71jdsCb5fZaoSzULE5 false true
acct k7FaK87WHGVXzkaoHb7CdVPgkKDQhZ29VLDeBVbDfYn false false
acct p2Yicb86aZig616Eav2VWG9vuXR5mEqhtzshZYBxzsV false false
acct swqrv48gsrwpBFbftEwnP2vB4jckpvfGJfXkwaniLCC false false
acct ws91DX9HBAAxGW77BZs5FogRDwpRtcUpiLBpKdPTfWu false false
acct BaNhKDjFJTKQhS1To1XK8eApMSYBoMUNky6wKrf4RX5N false false
acct cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN false false
acct 2MNus2KCpxwXnp19iyXNpWSFtBD2UGjQBAL8AbtywfT9 false true
acct 2RJD1KnDRGEkvuFfAGrJ7PD28LRE9LRDjZznDywagzmr false false
== simulate_execute_false
program cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN
length 18
sha256 280be21d19598dde60e33389544cc5a8c462cae5228d4e7942a2f721ff603aa7
acct Fb88t17apPNxTw4BCCC85yvcybdCvaY41qGJYpWXyrsR false true
acct 4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi true true
acct 11111111111111111111111111111111 false false
acct gBxS1f6uyyGPuW5MzGBukidSb71jdsCb5fZaoSzULE5 false true
acct k7FaK87WHGVXzkaoHb7CdVPgkKDQhZ29VLDeBVbDfYn false false
acct p2Yicb86aZig616Eav2VWG9vuXR5mEqhtzshZYBxzsV false false
acct swqrv48gsrwpBFbftEwnP2vB4jckpvfGJfXkwaniLCC false false
acct ws91DX9HBAAxGW77BZs5FogRDwpRtcUpiLBpKdPTfWu false false
acct BaNhKDjFJTKQhS1To1XK8eApMSYBoMUNky6wKrf4RX5N false false
acct cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN false false
acct 2MNus2KCpxwXnp19iyXNpWSFtBD2UGjQBAL8AbtywfT9 false true
acct 2RJD1KnDRGEkvuFfAGrJ7PD28LRE9LRDjZznDywagzmr false false
== simulate_execute_true
program cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN
length 18
sha256 e5a37ed26ed87254a4b001ec8701644ff4d4a9add7ec060f91be863af40dfeea
acct Fb88t17apPNxTw4BCCC85yvcybdCvaY41qGJYpWXyrsR false true
acct 4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi true true
acct 11111111111111111111111111111111 false false
acct cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN false false
acct k7FaK87WHGVXzkaoHb7CdVPgkKDQhZ29VLDeBVbDfYn false false
acct p2Yicb86aZig616Eav2VWG9vuXR5mEqhtzshZYBxzsV false false
acct swqrv48gsrwpBFbftEwnP2vB4jckpvfGJfXkwaniLCC false false
acct ws91DX9HBAAxGW77BZs5FogRDwpRtcUpiLBpKdPTfWu false false
acct BaNhKDjFJTKQhS1To1XK8eApMSYBoMUNky6wKrf4RX5N false false
acct cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN false false
== simulate_execute_large
program cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN
length 1513
sha256 37bc1448dc3dc133e32cc10c20a0382efeb72b3098e49b92d43ccb59ce3f806a
acct Fb88t17apPNxTw4BCCC85yvcybdCvaY41qGJYpWXyrsR false true
acct 4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi true true
acct 11111111111111111111111111111111 false false
acct gBxS1f6uyyGPuW5MzGBukidSb71jdsCb5fZaoSzULE5 false true
acct k7FaK87WHGVXzkaoHb7CdVPgkKDQhZ29VLDeBVbDfYn false false
acct p2Yicb86aZig616Eav2VWG9vuXR5mEqhtzshZYBxzsV false false
acct swqrv48gsrwpBFbftEwnP2vB4jckpvfGJfXkwaniLCC false false
acct ws91DX9HBAAxGW77BZs5FogRDwpRtcUpiLBpKdPTfWu false false
acct BaNhKDjFJTKQhS1To1XK8eApMSYBoMUNky6wKrf4RX5N false false
acct cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN false false
acct 2MNus2KCpxwXnp19iyXNpWSFtBD2UGjQBAL8AbtywfT9 false true
acct 2RJD1KnDRGEkvuFfAGrJ7PD28LRE9LRDjZznDywagzmr false false
== settle_from_txdata
program cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN
length 16
sha256 6325719fea6023c28458c935a6d1e36489095460cba6f4cfdc78e9b430933946
acct Fb88t17apPNxTw4BCCC85yvcybdCvaY41qGJYpWXyrsR false true
acct DxJbRZapfMrinq8ofVVdfJZ3nmSG277N6wVwnbHdCzMK false false
acct 4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi true true
acct 11111111111111111111111111111111 false false
acct gBxS1f6uyyGPuW5MzGBukidSb71jdsCb5fZaoSzULE5 false true
acct k7FaK87WHGVXzkaoHb7CdVPgkKDQhZ29VLDeBVbDfYn false false
acct p2Yicb86aZig616Eav2VWG9vuXR5mEqhtzshZYBxzsV false false
acct swqrv48gsrwpBFbftEwnP2vB4jckpvfGJfXkwaniLCC false false
acct ws91DX9HBAAxGW77BZs5FogRDwpRtcUpiLBpKdPTfWu false false
acct BaNhKDjFJTKQhS1To1XK8eApMSYBoMUNky6wKrf4RX5N false false
acct cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN false false
acct 2MNus2KCpxwXnp19iyXNpWSFtBD2UGjQBAL8AbtywfT9 false true
acct 2RJD1KnDRGEkvuFfAGrJ7PD28LRE9LRDjZznDywagzmr false false
`;

/** `ix` in `EXPECTED`'s form. */
function written(name: string, ix: Instruction): string {
  const { accounts, data } = parts(ix);
  let out = `== ${name}\nprogram ${ix.programAddress}\nlength ${data.length}\n`;
  out += `sha256 ${Buffer.from(sha256(data)).toString("hex")}\n`;
  for (const meta of accounts) out += `acct ${meta.address} ${isSignerRole(meta.role)} ${isWritableRole(meta.role)}\n`;
  return out;
}

/** `EXPECTED`'s entry `name`. */
function expected(name: string): string {
  const start = EXPECTED.indexOf(`== ${name}\n`);
  if (start < 0) throw new Error(`EXPECTED has no ${name}`);
  const end = EXPECTED.indexOf("== ", start + 3);
  return EXPECTED.slice(start, end < 0 ? undefined : end);
}

describe("ProtocolAdapter's builders, against the Rust crate's", () => {
  const built: [string, () => Instruction | Promise<Instruction>][] = [
    ["initialize", () => adapter.initializeIx(key(1), key(2), key(3), MOCK_SELECTOR)],
    ["pause", () => adapter.pauseIx(key(4))],
    ["unpause", () => adapter.unpauseIx(key(4))],
    ["deny_logic_refs", () => adapter.denyLogicRefsIx(key(4), entries)],
    ["deny_logic_refs_large", () => adapter.denyLogicRefsIx(key(4), manyEntries)],
    ["set_kind_tree_root", () => adapter.setKindTreeRootIx(key(4), new Uint8Array(32).fill(7))],
    ["transfer_ownership", () => adapter.transferOwnershipIx(key(4), key(5))],
    ["renounce_ownership", () => adapter.renounceOwnershipIx(key(4))],
    ["update_expiry_config", () => adapter.updateExpiryConfigIx(key(4), 100n, U64_MAX)],
    ["upgrade", () => adapter.upgradeIx(key(4), key(6), key(7))],
    ["risc_zero_verifier_paused", () => adapter.riscZeroVerifierPausedIx(key(3), MOCK_SELECTOR)],
    ["version", () => adapter.versionIx()],
    ["txdata_init", () => adapter.txdataInitIx(key(1), 7n, 900, 500n)],
    ["txdata_write", () => adapter.txdataWriteIx(key(1), 7n, 3856, Uint8Array.from([1, 2, 3, 4, 5]))],
    ["txdata_write_large", () => adapter.txdataWriteIx(key(1), 7n, 0, large(TXDATA_CHUNK_SIZE))],
    ["txdata_extend", () => adapter.txdataExtendIx(key(1), 7n, 900n)],
    ["txdata_close", () => adapter.txdataCloseIx(key(1), key(2), 7n)],
    ["txdata_close_expired", () => adapter.txdataCloseExpiredIx(key(3), key(2), key(1), 7n)],
    ["settle", () => adapter.settleIx(key(1), encode("input"), settlement(key(10), true))],
    ["settle_no_marker", () => adapter.settleIx(key(1), encode("input"), settlement(null, false))],
    ["settle_large", () => adapter.settleIx(key(1), large(1500), settlement(key(10), true))],
    [
      "simulate_execute_false",
      () => adapter.simulateExecuteIx(key(1), encode("input"), false, settlement(key(10), true)),
    ],
    ["simulate_execute_true", () => adapter.simulateExecuteIx(key(1), encode("input"), true, settlement(null, false))],
    ["simulate_execute_large", () => adapter.simulateExecuteIx(key(1), large(1500), true, settlement(key(10), true))],
    ["settle_from_txdata", () => adapter.settleFromTxdataIx(key(1), 7n, settlement(key(10), true))],
  ];

  it("builds every instruction the Rust crate does, byte for byte and account for account", async () => {
    const names = built.map(([name]) => name);
    expect(names, "an entry for each of EXPECTED's").toEqual([...EXPECTED.matchAll(/^== (\S+)$/gm)].map((m) => m[1]));
    for (const [name, build] of built) {
      const ix = written(name, await build());
      console.log(ix);
      expect(ix, name).toBe(expected(name));
    }
  });

  it.each(["settle_large", "simulate_execute_large", "txdata_write_large", "deny_logic_refs_large"])(
    "%s encodes an instruction larger than 1000 bytes",
    async (name) => {
      const ix = await built.find(([n]) => n === name)![1]();
      const { data } = parts(ix);
      console.log(`${name}: ${data.length} data bytes`);
      expect(data.length, name).toBeGreaterThan(1000);
      expect(written(name, ix), name).toBe(expected(name));
    },
  );
});
