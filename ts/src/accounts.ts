// On-chain account decoders. Cursor-based parsers that walk the Borsh schema
// field by field — no hardcoded offsets, so the decoder absorbs PA-side layout
// changes (new fields, type bumps) at the cost of a re-parse rather than a
// coordinated cross-repo offset edit.

import { ANCHOR_DISCRIMINATOR_LEN, MAX_TREE_DEPTH } from "./constants.js";
import { Cursor, TruncatedError } from "./cursor.js";

/**
 * The `PAStateAccount` layout number this decoder reads. The PA stores it at
 * byte 8 of the account data, right after the Anchor discriminator, in every
 * layout, and refuses every instruction on an account whose number is not its
 * own; a mismatch seen by a client is a deployment mid-migration.
 */
export const PA_STATE_SCHEMA_VERSION = 2;

/** Decoded PA state account. */
export interface PAStateAccount {
  schemaVersion: number;
  bump: number;
  verifierRouter: Uint8Array;
  proofSelector: Uint8Array;
  /** Kind-table commitment every settled aggregation instance must carry. */
  kindTableCommitment: Uint8Array;
  /** Whether settlement is paused (the owner's `pause` / `unpause`). */
  paused: boolean;
  root: Uint8Array;
  nextIndex: bigint;
  currentDepth: number;
  frontier: Uint8Array[];
  minExpirySlots: bigint;
  maxExpirySlots: bigint;
  /** Logic refs the owner denied: no settlement consumes or creates a resource carrying one. */
  deniedLogicRefs: Uint8Array[];
}

export class PAStateDecodeError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "PAStateDecodeError";
  }
}

/**
 * Decode a raw `PAStateAccount` byte buffer.
 *
 * The buffer is the full account-data slice returned by `getAccountInfo`,
 * including the 8-byte Anchor discriminator prefix.
 */
export function decodePaState(data: Uint8Array): PAStateAccount {
  try {
    return decode(new Cursor(data, ANCHOR_DISCRIMINATOR_LEN));
  } catch (e) {
    if (e instanceof TruncatedError) {
      throw new PAStateDecodeError(`PAState ${e.message}`);
    }
    throw e;
  }
}

function decode(c: Cursor): PAStateAccount {
  const schemaVersion = c.u8("schema_version");
  if (schemaVersion !== PA_STATE_SCHEMA_VERSION) {
    throw new PAStateDecodeError(
      `unsupported PAState schema version ${schemaVersion} (this decoder reads ${PA_STATE_SCHEMA_VERSION})`,
    );
  }
  const bump = c.u8("bump");
  const verifierRouter = c.array32("verifier_router");
  const proofSelector = c.take(4, "proof_selector");
  const kindTableCommitment = c.array32("kind_table_commitment");
  const pausedByte = c.u8("paused");
  if (pausedByte !== 0 && pausedByte !== 1) {
    throw new PAStateDecodeError(`invalid bool byte ${pausedByte} for field paused`);
  }
  const root = c.array32("root");
  const nextIndex = c.u64Le("next_index");
  const currentDepth = c.u8("current_depth");
  if (currentDepth === 0 || currentDepth > MAX_TREE_DEPTH) {
    throw new PAStateDecodeError(`invalid PA tree depth: ${currentDepth}`);
  }
  const frontierLen = c.u32Le("frontier length");
  if (frontierLen < currentDepth) {
    throw new PAStateDecodeError(
      `PA frontier length ${frontierLen} is smaller than depth ${currentDepth}`,
    );
  }
  const frontier: Uint8Array[] = [];
  for (let i = 0; i < frontierLen; i++) {
    frontier.push(c.array32("frontier entry"));
  }

  const minExpirySlots = c.u64Le("min_expiry_slots");
  const maxExpirySlots = c.u64Le("max_expiry_slots");
  const deniedLogicRefs = c.vecArray32("denied_logic_refs");

  return {
    schemaVersion,
    bump,
    verifierRouter,
    proofSelector,
    kindTableCommitment,
    paused: pausedByte === 1,
    root,
    nextIndex,
    currentDepth,
    frontier,
    minExpirySlots,
    maxExpirySlots,
    deniedLogicRefs,
  };
}
