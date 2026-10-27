// The program-derived addresses the IDL does not resolve: the markers a
// settlement takes as remaining accounts, the event authority, and the
// verifier router's accounts. Pure functions: same inputs always produce the
// same PublicKey + bump.

import { PublicKey } from "@solana/web3.js";

import { idlSeed } from "./idl.js";

// ---- PA program PDAs --------------------------------------------------------

/** Derive a nullifier marker PDA, from the IDL's `NULLIFIER_SEED`. */
export function deriveNullifierPda(
  paProgram: PublicKey,
  paState: PublicKey,
  nullifier: Uint8Array,
): [PublicKey, number] {
  return PublicKey.findProgramAddressSync([idlSeed("NULLIFIER_SEED"), paState.toBuffer(), nullifier], paProgram);
}

/** Derive a root marker PDA, from the IDL's `ROOT_SEED`. */
export function deriveRootMarkerPda(
  paProgram: PublicKey,
  paState: PublicKey,
  root: Uint8Array,
): [PublicKey, number] {
  return PublicKey.findProgramAddressSync([idlSeed("ROOT_SEED"), paState.toBuffer(), root], paProgram);
}

/**
 * Derive a program's event authority PDA. Seed: `["__event_authority"]`.
 *
 * Anchor's `#[event_cpi]` signs each event self-invocation with this PDA and
 * requires it, followed by the program's own address, after an instruction's
 * other named accounts: for the PA, `settle` and `settle_from_txdata`. Any
 * program that emits events this way derives its event authority with the
 * same seed.
 */
export function deriveEventAuthorityPda(program: PublicKey): [PublicKey, number] {
  return PublicKey.findProgramAddressSync(
    [new TextEncoder().encode("__event_authority")],
    program,
  );
}

// ---- Verifier router PDAs --------------------------------------------------

/**
 * Derive the verifier-router state PDA and the router's verifier-entry PDA for
 * `selector`, the proof selector the adapter was initialized with.
 */
export function deriveVerifierRouterPdas(
  verifierRouterProgram: PublicKey,
  selector: Uint8Array,
): { router: PublicKey; entry: PublicKey } {
  const [router] = PublicKey.findProgramAddressSync(
    [new TextEncoder().encode("router")],
    verifierRouterProgram,
  );
  const [entry] = PublicKey.findProgramAddressSync(
    [new TextEncoder().encode("verifier"), selector],
    verifierRouterProgram,
  );
  return { router, entry };
}
