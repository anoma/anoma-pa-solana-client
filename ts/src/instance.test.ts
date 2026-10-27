import { sha256 } from "@noble/hashes/sha2";
import BN from "bn.js";
import { Connection, Keypair, PublicKey, type AccountMeta, type TransactionInstruction } from "@solana/web3.js";
import { describe, expect, it } from "vitest";

import { coder } from "./coder.js";
import { PADDING_LEAF } from "./constants.js";
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
} from "./instance.js";
import { deriveEventAuthorityPda, deriveVerifierRouterPdas } from "./pda.js";

const DEVNET_GENESIS = "EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG";
const MAINNET_GENESIS = "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d";

/** What `recordedDeployment` throws, or a failure when it returns. */
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
    const unknown = PublicKey.default.toBase58();
    for (const environment of ["staging", "production"] as const) {
      const e = thrown(() => recordedDeployment(environment, unknown));
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
      programId: new PublicKey(idl.address),
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
    const program = Keypair.generate().publicKey;
    const data = (bytes: number[], from = program) => ({
      programId: from.toBase58(),
      data: [Buffer.from(bytes).toString("base64"), "base64"] as [string, "base64"],
    });
    expect(returnedBool(program, data([1]))).toBe(true);
    expect(returnedBool(program, data([0]))).toBe(false);
    for (const [what, returned] of [
      ["nothing", null],
      ["another program's", data([1], Keypair.generate().publicKey)],
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

/** `value`, decoded by the coder, with keys as base58, integers as decimal strings and bytes as number arrays. */
function plain(value: unknown): unknown {
  if (value instanceof PublicKey) return value.toBase58();
  if (BN.isBN(value)) return value.toString();
  if (value instanceof Uint8Array) return Array.from(value);
  if (Array.isArray(value)) return value.map(plain);
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, plain(v)]));
  }
  return value;
}

describe("ProtocolAdapter's builders", () => {
  const key = () => Keypair.generate().publicKey;
  const pa = key();
  const adapter = ProtocolAdapter.at(new Connection("http://127.0.0.1:1"), pa);
  const [payer, owner, router, authority, refund, buffer, spill] = [key(), key(), key(), key(), key(), key(), key()];
  const encode = (s: string) => new TextEncoder().encode(s);
  const u64le = (n: bigint) => {
    const b = Buffer.alloc(8);
    b.writeBigUInt64LE(n);
    return b;
  };
  const pda = (...seeds: Uint8Array[]) => PublicKey.findProgramAddressSync(seeds, pa)[0];
  const paState = pda(encode("pa_state"));
  const txData = pda(encode("tx_data"), authority.toBuffer(), u64le(7n));
  const events = { event_authority: deriveEventAuthorityPda(pa)[0], program: pa };
  const programData = PublicKey.findProgramAddressSync(
    [pa.toBuffer()],
    new PublicKey("BPFLoaderUpgradeab1e11111111111111111111111"),
  )[0];
  const upgradeAuthority = pda(encode("upgrade_authority"));
  const remaining = [{ pubkey: key(), isSigner: false, isWritable: true }];
  const settlement = {
    paState,
    newRootMarker: null,
    verifierRouterProgram: key(),
    router: key(),
    verifierEntry: key(),
    verifierProgram: key(),
    remainingAccounts: remaining,
  };
  const settlementNamed = {
    pa_state: paState,
    verifier_router_program: settlement.verifierRouterProgram,
    router: settlement.router,
    verifier_entry: settlement.verifierEntry,
    verifier_program: settlement.verifierProgram,
  };
  const marker = key();
  const selector = Uint8Array.from([1, 2, 3, 4]);
  const root = new Uint8Array(32).fill(7);

  // Each row: the instruction, the arguments it must decode to, the accounts
  // the caller or the instance's address determine (`null`: an absent
  // optional account), and the remaining accounts. Every other account is the
  // IDL's fixed address or the PDA of its constant seeds.
  const rows: [string, TransactionInstruction, object, Record<string, PublicKey | null>, AccountMeta[]][] = [
    [
      "initialize",
      adapter.initializeIx(payer, owner, router, selector),
      { initial_owner: owner.toBase58(), verifier_router: router.toBase58(), proof_selector: [1, 2, 3, 4] },
      {
        payer,
        program_data: programData,
        verifier_entry: deriveVerifierRouterPdas(router, selector).entry,
        ...events,
      },
      [],
    ],
    ["pause", adapter.pauseIx(authority), {}, { authority, ...events }, []],
    ["unpause", adapter.unpauseIx(authority), {}, { authority, ...events }, []],
    [
      "transfer_ownership",
      adapter.transferOwnershipIx(authority, owner),
      { new_owner: owner.toBase58() },
      { authority, ...events },
      [],
    ],
    ["renounce_ownership", adapter.renounceOwnershipIx(authority), {}, { authority, ...events }, []],
    [
      "set_kind_tree_root",
      adapter.setKindTreeRootIx(authority, root),
      { new_kind_tree_root: Array.from(root) },
      { authority, ...events },
      [],
    ],
    [
      "update_expiry_config",
      adapter.updateExpiryConfigIx(authority, 100n, 2n ** 64n - 1n),
      { new_min_expiry_slots: "100", new_max_expiry_slots: (2n ** 64n - 1n).toString() },
      { authority },
      [],
    ],
    [
      "upgrade",
      adapter.upgradeIx(authority, buffer, spill),
      {},
      { authority, program_data: programData, upgraded_program: pa, buffer, spill, ...events },
      [],
    ],
    ["version", adapter.versionIx(), {}, {}, []],
    [
      "risc_zero_verifier_paused",
      adapter.riscZeroVerifierPausedIx(router, selector),
      {},
      { verifier_entry: deriveVerifierRouterPdas(router, selector).entry },
      [],
    ],
    [
      "txdata_init",
      adapter.txdataInitIx(authority, 7n, 900, 500n),
      { upload_id: "7", capacity: 900, expires_slot: "500" },
      { tx_data: txData, authority },
      [],
    ],
    [
      "txdata_extend",
      adapter.txdataExtendIx(authority, 7n, 900n),
      { upload_id: "7", new_expires_slot: "900" },
      { tx_data: txData, authority },
      [],
    ],
    [
      "txdata_close",
      adapter.txdataCloseIx(authority, refund, 7n),
      { upload_id: "7" },
      { tx_data: txData, authority, refund },
      [],
    ],
    [
      "txdata_close_expired",
      adapter.txdataCloseExpiredIx(payer, refund, authority, 7n),
      { upload_id: "7", authority: authority.toBase58() },
      { tx_data: txData, payer, refund },
      [],
    ],
    [
      "settle_from_txdata",
      adapter.settleFromTxdataIx(authority, 7n, settlement),
      { upload_id: "7" },
      { ...settlementNamed, tx_data: txData, authority, new_root_marker: null, ...events },
      remaining,
    ],
    [
      "settle_from_txdata",
      adapter.settleFromTxdataIx(authority, 7n, { ...settlement, newRootMarker: marker }),
      { upload_id: "7" },
      { ...settlementNamed, tx_data: txData, authority, new_root_marker: marker, ...events },
      remaining,
    ],
  ];

  it("builds each instruction at the instance's address, as the IDL declares it", () => {
    for (const [name, ix, args, named, rest] of rows) {
      const idlIx = idl.instructions.find((i) => i.name === name);
      if (idlIx === undefined) throw new Error(`the IDL declares no instruction ${name}`);
      console.log(`${name}: ${ix.keys.length} accounts, ${ix.data.length} data bytes`);
      expect(ix.programId.equals(pa), name).toBe(true);
      expect(Array.from(ix.data.subarray(0, idlIx.discriminator.length)), name).toEqual(idlIx.discriminator);
      const decoded = coder.instruction.decode(ix.data);
      expect(decoded?.name, name).toBe(name);
      expect(plain(decoded?.data), name).toEqual(args);

      expect(ix.keys.length, name).toBe(idlIx.accounts.length + rest.length);
      for (const [i, a] of idlIx.accounts.entries()) {
        if ("accounts" in a) throw new Error(`${name}: account group ${a.name}`);
        const built = ix.keys[i]!;
        const given = named[a.name];
        if (given === null) {
          expect(a.optional, `${name}.${a.name} is optional`).toBe(true);
          expect(built, `${name}.${a.name} absent`).toEqual({ pubkey: pa, isSigner: false, isWritable: false });
          continue;
        }
        let expected = given;
        if (expected === undefined && a.address !== undefined) expected = new PublicKey(a.address);
        if (expected === undefined && a.pda?.seeds.every((seed) => seed.kind === "const")) {
          expected = pda(...a.pda.seeds.map((seed) => Uint8Array.from(seed.kind === "const" ? seed.value : [])));
        }
        if (expected === undefined) throw new Error(`${name}.${a.name}: no expected address`);
        expect(built.pubkey.toBase58(), `${name}.${a.name}`).toBe(expected.toBase58());
        expect([built.isSigner, built.isWritable], `${name}.${a.name} flags`).toEqual([
          a.signer ?? false,
          a.writable ?? false,
        ]);
      }
      expect(ix.keys.slice(idlIx.accounts.length), `${name} remaining accounts`).toEqual(rest);
    }
  });
});
