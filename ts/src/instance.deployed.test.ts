// The staging adapter on devnet, read over a Helius endpoint: run with
// `npm run test:deployed` at each deployment and upgrade.

import { createSolanaRpc } from "@solana/kit";
import { describe, expect, it } from "vitest";

import { PADDING_LEAF } from "./constants.js";
import { idlConstant } from "./idl.js";
import { protocolAdapter } from "./instance.js";

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

describe("the staging adapter on devnet", () => {
  it("answers every read", async () => {
    const adapter = await protocolAdapter(createSolanaRpc(devnetUrl()), "staging");
    const owner = await adapter.owner();
    const depth = await adapter.commitmentTreeDepth();
    const count = await adapter.commitmentCount();
    const capacity = await adapter.commitmentTreeCapacity();
    const root = await adapter.latestCommitmentTreeRoot();
    const kindTreeRoot = await adapter.getKindTreeRoot();
    console.log({
      programId: adapter.programId,
      owner,
      paused: await adapter.paused(),
      verifierRouter: await adapter.riscZeroVerifierRouter(),
      selector: await adapter.riscZeroVerifierSelector(),
      depth,
      count,
      capacity,
      kindTreeRoot,
    });

    // The owner, an operator wallet, pays the simulation's fee.
    expect(typeof (await adapter.riscZeroVerifierPaused(owner))).toBe("boolean");
    expect((await adapter.riscZeroVerifierSelector()).length).toBe(4);
    expect(capacity).toBe(2n ** BigInt(depth));
    expect(capacity > count).toBe(true);
    expect((await adapter.commitmentTreeLeftSiblings()).length).toBeGreaterThanOrEqual(depth);
    const empty = await adapter.commitmentTreeEmptySubtreeRoots();
    expect(empty.length).toBe(depth + 1);
    expect(empty[0]).toEqual(Array.from(PADDING_LEAF));
    expect(kindTreeRoot.length).toBe(32);
    expect(adapter.emptyKindTreeRoot()).toEqual(idlConstant("EMPTY_KIND_TREE_ROOT"));

    const zero = new Uint8Array(32);
    expect(await adapter.isCommitmentTreeRootHistorical(Uint8Array.from(root))).toBe(true);
    expect(await adapter.isCommitmentTreeRootHistorical(zero)).toBe(false);
    expect(await adapter.isNullifierContained(zero)).toBe(false);
    expect(await adapter.isLogicRefDenied(zero, true)).toBe(false);
    expect(await adapter.isLogicRefDenied(zero, false)).toBe(false);
    expect(owner).not.toBe("11111111111111111111111111111111");
  });
});
