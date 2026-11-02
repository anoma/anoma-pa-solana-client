// The staging adapter on devnet, read over a Helius endpoint: run with
// `npm run test:deployed` at each deployment and upgrade. Each read is checked
// against the account it reads, fetched and decoded here, and the marker reads
// against a nullifier and a root the adapter's own history settled.

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { BorshCoder, type Idl } from "@anchor-lang/core";
import {
  createSolanaRpc,
  fetchEncodedAccount,
  getBase58Encoder,
  getProgramDerivedAddress,
  type Address,
  type Rpc,
  type Signature,
  type SolanaRpcApiMainnet,
} from "@solana/kit";
import { describe, expect, it } from "vitest";

import { coder } from "./coder.js";
import { PADDING_LEAF } from "./constants.js";
import { protocolAdapter } from "./contract.js";
import { decodeCpiEvent, EVENT_IX_TAG } from "./events.js";
import { idl, idlConstant, idlSeed } from "./idl.js";
import { deriveVerifierRouterPdas } from "./pda.js";

function devnetUrl(): string {
  const url = process.env.HELIUS_DEVNET_URL;
  if (url === undefined || url === "") {
    throw new Error(
      "HELIUS_DEVNET_URL is unset: set HELIUS_DEVNET_URL to the Helius devnet endpoint " +
        "(scripts/helius.env in the umbrella repository) and run npm run test:deployed",
    );
  }
  return url;
}

/** The verifier router's accounts, decoded with the IDL the Rust crate generates its bindings from. */
const verifierRouterCoder = new BorshCoder(
  JSON.parse(
    readFileSync(fileURLToPath(new URL("../../rust/idls/verifier_router.json", import.meta.url)), "utf8"),
  ) as Idl,
);

/** The data of the account at `address`, which must exist. */
async function accountData(rpc: Rpc<SolanaRpcApiMainnet>, address: Address): Promise<Uint8Array> {
  const account = await fetchEncodedAccount(rpc, address);
  if (!account.exists) throw new Error(`the account ${address} does not exist`);
  return account.data;
}

const bytes = (value: number[]): Uint8Array => Uint8Array.from(value);

/**
 * The nullifiers and roots of the latest settlement in `program`'s history,
 * from its `ActionExecuted` and `CommitmentTreeRootAdded` events, or `null`
 * when the history holds no settlement.
 */
async function latestSettlement(
  rpc: Rpc<SolanaRpcApiMainnet>,
  program: Address,
): Promise<{ signature: Signature; nullifiers: number[][]; roots: number[][] } | null> {
  let before: Signature | undefined;
  for (;;) {
    const page = await rpc.getSignaturesForAddress(program, { limit: 1000, ...(before && { before }) }).send();
    if (page.length === 0) return null;
    for (const { signature, err } of page) {
      if (err !== null) continue;
      const tx = await rpc
        .getTransaction(signature, { encoding: "json", maxSupportedTransactionVersion: 1 })
        .send();
      if (tx === null) throw new Error(`the transaction ${signature} is not found`);
      const keys = [
        ...tx.transaction.message.accountKeys,
        ...(tx.meta?.loadedAddresses?.writable ?? []),
        ...(tx.meta?.loadedAddresses?.readonly ?? []),
      ];
      const nullifiers: number[][] = [];
      const roots: number[][] = [];
      for (const group of tx.meta?.innerInstructions ?? []) {
        for (const ix of group.instructions) {
          if (keys[ix.programIdIndex] !== program) continue;
          const data = Uint8Array.from(getBase58Encoder().encode(ix.data));
          if (!EVENT_IX_TAG.every((b, i) => data[i] === b)) continue;
          const event = decodeCpiEvent(data);
          if (event.name === "ActionExecuted") nullifiers.push(...(event.data as { nullifiers: number[][] }).nullifiers);
          if (event.name === "CommitmentTreeRootAdded") roots.push((event.data as { root: number[] }).root);
        }
      }
      if (nullifiers.length > 0) return { signature, nullifiers, roots };
    }
    before = page[page.length - 1]!.signature;
  }
}

describe("the staging adapter on devnet", () => {
  it("answers every read with what its accounts hold", async () => {
    const rpc = createSolanaRpc(devnetUrl());
    const adapter = await protocolAdapter(rpc, "staging");
    expect(adapter.address).toBe(idl.address);

    const [paState] = await getProgramDerivedAddress({
      programAddress: adapter.address,
      seeds: [idlSeed("PA_STATE_SEED")],
    });
    const state = coder.accounts.decode("PAStateAccount", await accountData(rpc, paState));
    console.log({ address: adapter.address, state });

    expect(await adapter.paused()).toBe(state.paused);
    expect(await adapter.riscZeroVerifierRouter()).toBe(state.verifier_router);
    expect(await adapter.riscZeroVerifierSelector()).toEqual(state.proof_selector);
    expect(await adapter.commitmentCount()).toBe(state.next_index);
    expect(await adapter.commitmentTreeDepth()).toBe(state.current_depth);
    expect(await adapter.commitmentTreeCapacity()).toBe(2n ** BigInt(state.current_depth));
    expect(await adapter.commitmentTreeLeftSiblings()).toEqual(state.frontier);
    const empty = await adapter.commitmentTreeEmptySubtreeRoots();
    expect(empty.length).toBe(state.current_depth + 1);
    expect(empty[0]).toEqual(Array.from(PADDING_LEAF));
    expect(await adapter.latestCommitmentTreeRoot()).toEqual(state.root);
    expect(await adapter.getKindTreeRoot()).toEqual(state.kind_tree_root);
    expect(adapter.emptyKindTreeRoot()).toEqual(idlConstant("EMPTY_KIND_TREE_ROOT"));
    expect(await adapter.owner()).toBe(state.owner);

    // The owner, an operator wallet, pays the simulation's fee.
    const { entry } = await deriveVerifierRouterPdas(state.verifier_router, bytes(state.proof_selector));
    const verifierEntry = verifierRouterCoder.accounts.decode("VerifierEntry", await accountData(rpc, entry));
    console.log({ verifierEntry });
    expect(await adapter.riscZeroVerifierPaused(state.owner)).toBe(verifierEntry.estopped);

    const zero = new Uint8Array(32);
    for (const consumed of [true, false]) {
      const denied: number[][] = consumed ? state.denied_consumed_logic_refs : state.denied_created_logic_refs;
      for (const logicRef of denied) {
        expect(await adapter.isLogicRefDenied(bytes(logicRef), consumed), `${logicRef} consumed ${consumed}`).toBe(true);
      }
      const zeroDenied = denied.some((r) => r.every((b) => b === 0));
      expect(await adapter.isLogicRefDenied(zero, consumed)).toBe(zeroDenied);
    }

    expect(await adapter.isCommitmentTreeRootHistorical(bytes(state.root))).toBe(true);
    expect(await adapter.isCommitmentTreeRootHistorical(zero)).toBe(false);
    expect(await adapter.isNullifierContained(zero)).toBe(false);

    // Every commitment is created by a settlement, which consumes a resource
    // and adds a root: an adapter holding commitments has settled one.
    const settled = await latestSettlement(rpc, adapter.address);
    console.log({ settled });
    expect(settled === null).toBe(state.next_index === 0n);
    for (const nullifier of settled?.nullifiers ?? []) {
      expect(await adapter.isNullifierContained(bytes(nullifier)), `nullifier ${nullifier}`).toBe(true);
    }
    for (const root of settled?.roots ?? []) {
      expect(await adapter.isCommitmentTreeRootHistorical(bytes(root)), `root ${root}`).toBe(true);
    }
  });
});
