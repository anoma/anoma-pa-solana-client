// Solana program identifiers used by AnomaPay.
//
// Devnet defaults are baked in. For mainnet or alternative cluster deployments,
// integrators should override at the call site rather than relying on these.

import { PublicKey } from "@solana/web3.js";

/** Anoma Protocol Adapter program ID (devnet/localnet default). */
export const PA_PROGRAM_ID = new PublicKey(
  "5zeqkB3kc9fd1RvaXB2GeMB53Jgf98QJtaFK38e6tTsc",
);

/**
 * The devnet V2 deployment's settlement lookup table: the accounts every
 * settlement carries that are fixed for the deployment. Settle transactions
 * are v0 messages compiled against it (`TransactionMessage.compileToV0Message`
 * with the fetched `AddressLookupTableAccount`).
 */
export const SETTLE_LOOKUP_TABLE = new PublicKey(
  "4UFsq2ks2DcC29ErmEeHxqXWKLRWo26vs4W65S89bpWn",
);
