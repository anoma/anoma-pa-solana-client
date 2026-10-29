// The program-derived addresses the IDL does not resolve: the markers a
// settlement takes as remaining accounts, the event authority, and the
// verifier router's accounts. Pure functions: same inputs always produce the
// same address + bump.

import {
  address,
  getAddressEncoder,
  getProgramDerivedAddress,
  type Address,
  type ProgramDerivedAddress,
} from "@solana/kit";

import { idlSeed } from "./idl.js";

const addressBytes = (a: Address) => getAddressEncoder().encode(a);

// ---- PA program PDAs --------------------------------------------------------

/** Derive a nullifier marker PDA, from the IDL's `NULLIFIER_SEED`. */
export function deriveNullifierPda(
  paProgram: Address,
  paState: Address,
  nullifier: Uint8Array,
): Promise<ProgramDerivedAddress> {
  return getProgramDerivedAddress({
    programAddress: paProgram,
    seeds: [idlSeed("NULLIFIER_SEED"), addressBytes(paState), nullifier],
  });
}

/** Derive a root marker PDA, from the IDL's `ROOT_SEED`. */
export function deriveRootMarkerPda(
  paProgram: Address,
  paState: Address,
  root: Uint8Array,
): Promise<ProgramDerivedAddress> {
  return getProgramDerivedAddress({
    programAddress: paProgram,
    seeds: [idlSeed("ROOT_SEED"), addressBytes(paState), root],
  });
}

/**
 * Derive a program's event authority PDA. Seed: `["__event_authority"]`.
 *
 * Anchor's `#[event_cpi]` signs each event self-invocation with this PDA and
 * requires it, followed by the program's own address, after an instruction's
 * other named accounts: for the PA, every instruction that emits an event.
 * Any program that emits events this way derives its event authority with the
 * same seed.
 */
export function deriveEventAuthorityPda(program: Address): Promise<ProgramDerivedAddress> {
  return getProgramDerivedAddress({ programAddress: program, seeds: ["__event_authority"] });
}

// ---- Verifier router PDAs --------------------------------------------------

/**
 * Derive the verifier-router state PDA and the router's verifier-entry PDA for
 * `selector`, the proof selector the adapter was initialized with.
 */
export async function deriveVerifierRouterPdas(
  verifierRouterProgram: Address,
  selector: Uint8Array,
): Promise<{ router: Address; entry: Address }> {
  const [[router], [entry]] = await Promise.all([
    getProgramDerivedAddress({ programAddress: verifierRouterProgram, seeds: ["router"] }),
    getProgramDerivedAddress({ programAddress: verifierRouterProgram, seeds: ["verifier", selector] }),
  ]);
  return { router, entry };
}

/** The upgradeable BPF loader, which owns every upgradeable program's ProgramData account. */
const BPF_LOADER_UPGRADEABLE = address("BPFLoaderUpgradeab1e11111111111111111111111");

/** Derive the upgradeable loader's ProgramData account of `program`, which `initialize` and `upgrade` take. */
export async function deriveProgramDataAddress(program: Address): Promise<Address> {
  return (await getProgramDerivedAddress({ programAddress: BPF_LOADER_UPGRADEABLE, seeds: [addressBytes(program)] }))[0];
}
