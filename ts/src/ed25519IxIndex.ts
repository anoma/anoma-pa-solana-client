// Ed25519 instruction index computation.
//
// The on-chain forwarder reads the verified wrap-authorization signature from
// the transaction's `Instructions` sysvar by absolute index. The frontend must
// carry that index in the witness so the forwarder loads the right ix.
//
// Hardcoding the index is fragile: any preamble reorder (adding a logging ix,
// shifting compute-budget) makes the value wrong. Compute it from the actual
// ix layout instead.

import { ED25519_PROGRAM_ID } from "./programIds.js";

/** Minimal shape of an `Instruction`-like input — anything with a `programId`. */
export interface InstructionLike {
  programId: { equals: (other: { equals: (o: object) => boolean }) => boolean };
}

/**
 * Find the absolute index of the *Nth* `Ed25519Program` instruction in a list.
 *
 * `nth` is zero-based: pass 0 for the first wrap authorization's ed25519 ix,
 * 1 for the second, etc. Throws if no such instruction exists at that position.
 */
export function findEd25519IxIndex(
  instructions: readonly InstructionLike[],
  nth = 0,
): number {
  let seen = 0;
  for (let i = 0; i < instructions.length; i++) {
    if (instructions[i]!.programId.equals(ED25519_PROGRAM_ID as never)) {
      if (seen === nth) {
        return i;
      }
      seen++;
    }
  }
  throw new Error(
    `transaction does not contain an Ed25519 verify instruction at occurrence ${nth}`,
  );
}

/**
 * Default `ed25519_ix_index` for the canonical settle-tx layout:
 *
 *   [0] SetComputeUnitLimit
 *   [1] RequestHeapFrame
 *   [2] Ed25519 verify
 *   [3] settle_from_txdata
 *
 * Exposed as a constant for sites that genuinely build that exact layout (test
 * scripts, fixtures). Production code should prefer `findEd25519IxIndex` against
 * the actual instruction list to avoid silent drift if the preamble changes.
 */
export const DEFAULT_ED25519_IX_INDEX = 2;
