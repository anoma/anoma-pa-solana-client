// Blockhash-expiry-aware transaction confirmation.
//
// `connection.confirmTransaction(sig, commitment)` is deprecated and does not
// surface blockhash expiry. This helper polls `getSignatureStatuses` and aborts
// when `getBlockHeight() > lastValidBlockHeight`.

import type { BlockhashWithExpiryBlockHeight, Commitment, Connection } from "@solana/web3.js";

/**
 * Wait for a transaction signature to reach `commitment`, or fail if the
 * underlying blockhash expires first.
 */
export async function confirmTransactionWithBlockhash(
  connection: Connection,
  blockhash: BlockhashWithExpiryBlockHeight,
  signature: string,
  commitment: Commitment = "confirmed",
  pollIntervalMs = 500,
): Promise<void> {
  while (true) {
    const [{ value: statuses }, height] = await Promise.all([
      connection.getSignatureStatuses([signature]),
      connection.getBlockHeight(),
    ]);
    const status = statuses[0];
    if (status?.err) {
      throw new Error(
        `Transaction ${signature} failed: ${JSON.stringify(status.err)}`,
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
