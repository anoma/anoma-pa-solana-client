// Solana program identifiers used by AnomaPay.
//
// Devnet defaults are baked in. For mainnet or alternative cluster deployments,
// integrators should override at the call site rather than relying on these.

import { PublicKey } from "@solana/web3.js";

/** Anoma Protocol Adapter program ID (devnet/localnet default). */
export const PA_PROGRAM_ID = new PublicKey(
  "5zeqkB3kc9fd1RvaXB2GeMB53Jgf98QJtaFK38e6tTsc",
);
