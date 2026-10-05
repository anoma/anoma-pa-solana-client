// Program-derived-address helpers for the PA.
//
// These mirror the seed schemas baked into the on-chain programs. They are pure
// functions: same inputs always produce the same PublicKey + bump.

import { PublicKey } from "@solana/web3.js";

const u64Le = (value: bigint): Uint8Array => {
  const buf = new Uint8Array(8);
  new DataView(buf.buffer).setBigUint64(0, value, true);
  return buf;
};

// ---- PA program PDAs --------------------------------------------------------

/** Derive the global PA state PDA. Seed: `["pa_state"]`. */
export function derivePaStatePda(paProgram: PublicKey): [PublicKey, number] {
  return PublicKey.findProgramAddressSync(
    [new TextEncoder().encode("pa_state")],
    paProgram,
  );
}

/** Derive a per-authority+upload `tx_data` PDA. */
export function deriveTxDataPda(
  paProgram: PublicKey,
  authority: PublicKey,
  uploadId: bigint,
): [PublicKey, number] {
  return PublicKey.findProgramAddressSync(
    [new TextEncoder().encode("tx_data"), authority.toBuffer(), u64Le(uploadId)],
    paProgram,
  );
}

/** Derive a nullifier marker PDA. */
export function deriveNullifierPda(
  paProgram: PublicKey,
  paState: PublicKey,
  nullifier: Uint8Array,
): [PublicKey, number] {
  return PublicKey.findProgramAddressSync(
    [new TextEncoder().encode("nullifier"), paState.toBuffer(), nullifier],
    paProgram,
  );
}

/** Derive a root marker PDA. */
export function deriveRootMarkerPda(
  paProgram: PublicKey,
  paState: PublicKey,
  root: Uint8Array,
): [PublicKey, number] {
  return PublicKey.findProgramAddressSync(
    [new TextEncoder().encode("root"), paState.toBuffer(), root],
    paProgram,
  );
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
