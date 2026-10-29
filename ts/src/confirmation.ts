// Blockhash-expiry-aware transaction confirmation.
//
// Polls `getSignatureStatuses` and aborts when `getBlockHeight() >
// lastValidBlockHeight`, so a transaction whose blockhash expired fails
// instead of being waited on forever.

import type { BlockhashLifetimeConstraint, Commitment, Rpc, Signature, SolanaRpcApiMainnet } from "@solana/kit";

import { toJson } from "./json.js";

/**
 * Wait for a transaction signature to reach `commitment`, or fail if the
 * underlying blockhash expires first.
 */
export async function confirmTransactionWithBlockhash(
  rpc: Rpc<SolanaRpcApiMainnet>,
  blockhash: BlockhashLifetimeConstraint,
  signature: Signature,
  commitment: Commitment = "confirmed",
  pollIntervalMs = 500,
): Promise<void> {
  while (true) {
    const [{ value: statuses }, height] = await Promise.all([
      rpc.getSignatureStatuses([signature]).send(),
      rpc.getBlockHeight().send(),
    ]);
    const status = statuses[0];
    if (status?.err) {
      throw new Error(
        `Transaction ${signature} failed: ${toJson(status.err)}`,
      );
    }
    if (status?.confirmationStatus === commitment || status?.confirmationStatus === "finalized") {
      return;
    }
    if (height > blockhash.lastValidBlockHeight) {
      throw new Error(
        `Transaction ${signature} expired before reaching ${commitment}`,
      );
    }
    await new Promise(r => setTimeout(r, pollIntervalMs));
  }
}
