// Solana program identifiers used by AnomaPay.
//
// Devnet defaults are baked in. For mainnet or alternative cluster deployments,
// integrators should override at the call site rather than relying on these.

import { PublicKey } from "@solana/web3.js";

/** Anoma Protocol Adapter program ID (devnet/localnet default). */
export const PA_PROGRAM_ID = new PublicKey(
  "28Hvr1YFv2ouGN2fS99aF3ZzYXzkncJVVaHcZNhquLFT",
);

/**
 * SPL Token Forwarder program ID: the V2 forwarder's declared id, under which
 * the adapter repository builds and deploys it.
 */
export const FORWARDER_PROGRAM_ID = new PublicKey(
  "5CrHbBeHjg53UyL3Htn9dCYYTy68fMcrbDoeAdo4yQrx",
);

/**
 * The devnet V2 deployment's settlement lookup table: the accounts every
 * settlement carries that are fixed for the deployment. Settle transactions
 * are v0 messages compiled against it (`TransactionMessage.compileToV0Message`
 * with the fetched `AddressLookupTableAccount`).
 */
export const SETTLE_LOOKUP_TABLE = new PublicKey(
  "CKAMrsJSf1SDgsmaM7hsKEwmi2efQsoCAuMNf4msGRSW",
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
