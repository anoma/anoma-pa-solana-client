import { sha256 } from "@noble/hashes/sha2";
import { Connection, Keypair, PublicKey } from "@solana/web3.js";
import { describe, expect, it } from "vitest";

import { coder } from "./coder.js";
import { MOCK_SELECTOR, PADDING_LEAF } from "./constants.js";
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
import { deriveVerifierRouterPdas } from "./pda.js";

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

describe("ProtocolAdapter", () => {
  it("builds risc_zero_verifier_paused at its address, for the verifier entry of the deployment's selector", () => {
    const pa = Keypair.generate().publicKey;
    const router = Keypair.generate().publicKey;
    const adapter = ProtocolAdapter.at(new Connection("http://127.0.0.1:1"), pa);
    expect(adapter.programId.equals(pa)).toBe(true);
    const ix = adapter.riscZeroVerifierPausedIx(router, MOCK_SELECTOR);
    const [paState] = PublicKey.findProgramAddressSync([new TextEncoder().encode("pa_state")], pa);
    const { entry } = deriveVerifierRouterPdas(router, MOCK_SELECTOR);
    expect(ix.programId.equals(pa)).toBe(true);
    expect(ix.keys).toEqual([
      { pubkey: paState, isSigner: false, isWritable: false },
      { pubkey: entry, isSigner: false, isWritable: false },
    ]);
    const discriminator = idl.instructions.find((i) => i.name === "risc_zero_verifier_paused")!.discriminator;
    expect(Array.from(ix.data)).toEqual(discriminator);
    expect(coder.instruction.decode(ix.data)?.name).toBe("risc_zero_verifier_paused");
  });
});
