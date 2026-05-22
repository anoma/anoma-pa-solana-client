// Program-derived-address helpers for the PA and the SPL Token Forwarder.
//
// These mirror the seed schemas baked into the on-chain programs. They are pure
// functions: same inputs always produce the same PublicKey + bump.

import { PublicKey } from "@solana/web3.js";

import { GROTH16_VERIFIER_SELECTOR } from "./constants.js";
import {
  ASSOCIATED_TOKEN_PROGRAM_ID,
  SPL_TOKEN_PROGRAM_ID,
} from "./programIds.js";

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

// ---- Forwarder PDAs --------------------------------------------------------

/** Derive the forwarder's global config PDA. */
export function deriveForwarderConfigPda(
  forwarderProgram: PublicKey,
): [PublicKey, number] {
  return PublicKey.findProgramAddressSync(
    [new TextEncoder().encode("config")],
    forwarderProgram,
  );
}

/**
 * Derive the forwarder's escrow PDA for a given mint. Seed: `["escrow", mint]`.
 *
 * This PDA is both the authority on the escrow's Associated Token Account and the
 * delegate users must name in their SPL `Approve` instruction before a wrap.
 */
export function deriveForwarderEscrowPda(
  forwarderProgram: PublicKey,
  tokenMint: PublicKey,
): [PublicKey, number] {
  return PublicKey.findProgramAddressSync(
    [new TextEncoder().encode("escrow"), tokenMint.toBuffer()],
    forwarderProgram,
  );
}

/** Derive the forwarder's nonce bitmap PDA. `word_index = nonce / 256`. */
export function deriveNonceBitmapPda(
  forwarderProgram: PublicKey,
  user: PublicKey,
  wordIndex: bigint,
): [PublicKey, number] {
  return PublicKey.findProgramAddressSync(
    [
      new TextEncoder().encode("nonce_bitmap"),
      user.toBuffer(),
      u64Le(wordIndex),
    ],
    forwarderProgram,
  );
}

// ---- SPL Associated Token Account ------------------------------------------

/**
 * Derive the SPL Associated Token Account address for a wallet and mint.
 *
 * Returns only the address (bump is unused by the ATA program during creation).
 */
export function deriveAssociatedTokenAddress(
  wallet: PublicKey,
  tokenMint: PublicKey,
): PublicKey {
  const [ata] = PublicKey.findProgramAddressSync(
    [wallet.toBuffer(), SPL_TOKEN_PROGRAM_ID.toBuffer(), tokenMint.toBuffer()],
    ASSOCIATED_TOKEN_PROGRAM_ID,
  );
  return ata;
}

// ---- Verifier router PDAs --------------------------------------------------

/** Derive the verifier-router state PDA and the Groth16 verifier-entry PDA. */
export function deriveVerifierRouterPdas(
  verifierRouterProgram: PublicKey,
): { router: PublicKey; entry: PublicKey } {
  const [router] = PublicKey.findProgramAddressSync(
    [new TextEncoder().encode("router")],
    verifierRouterProgram,
  );
  const [entry] = PublicKey.findProgramAddressSync(
    [new TextEncoder().encode("verifier"), GROTH16_VERIFIER_SELECTOR],
    verifierRouterProgram,
  );
  return { router, entry };
}
