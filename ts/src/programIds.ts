// Solana program identifiers used by AnomaPay.
//
// Devnet defaults are baked in. For mainnet or alternative cluster deployments,
// integrators should override at the call site rather than relying on these.

import { PublicKey } from "@solana/web3.js";

/** Anoma Protocol Adapter program ID (devnet/localnet default). */
export const PA_PROGRAM_ID = new PublicKey(
  "28Hvr1YFv2ouGN2fS99aF3ZzYXzkncJVVaHcZNhquLFT",
);

/** SPL Token Forwarder program ID (devnet/localnet default). */
export const FORWARDER_PROGRAM_ID = new PublicKey(
  "3cLKSYBijunpCc2F2gzizUkhYtyFrLr4RVdNiaK79b48",
);

/**
 * Solana's native ed25519 signature-verification program. Used to carry verified
 * wrap-authorization signatures into the settle transaction.
 */
export const ED25519_PROGRAM_ID = new PublicKey(
  "Ed25519SigVerify111111111111111111111111111",
);

/**
 * Solana's `Instructions` sysvar — used by the forwarder to introspect the
 * ed25519-verify instruction at `ed25519_ix_index`.
 */
export const INSTRUCTIONS_SYSVAR_ID = new PublicKey(
  "Sysvar1nstructions1111111111111111111111111",
);
