// On-chain account decoders. Cursor-based parsers that walk the Borsh schema
// field by field — no hardcoded offsets, so the decoder absorbs PA-side layout
// changes (new fields, type bumps) at the cost of a re-parse rather than a
// coordinated cross-repo offset edit.

import { ANCHOR_DISCRIMINATOR_LEN, HASH_LEN, MAX_TREE_DEPTH } from "./constants.js";

/** Decoded PA state account. */
export interface PAStateAccount {
  bump: number;
  authority: Uint8Array;
  verifierRouter: Uint8Array;
  proofSelector: Uint8Array;
  pendingAuthority: Uint8Array | null;
  lifecycle: number;
  root: Uint8Array;
  nextIndex: bigint;
  currentDepth: number;
  frontier: Uint8Array[];
  minExpirySlots: bigint;
  maxExpirySlots: bigint;
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
  let cursor = ANCHOR_DISCRIMINATOR_LEN;

  const take = (len: number, field: string): Uint8Array => {
    if (cursor + len > data.length) {
      throw new PAStateDecodeError(`PAState truncated while reading ${field}`);
    }
    const slice = data.slice(cursor, cursor + len);
    cursor += len;
    return slice;
  };

  const readU8 = (field: string): number => take(1, field)[0]!;
  const readU32Le = (field: string): number =>
    new DataView(take(4, field).buffer).getUint32(0, true);
  const readU64Le = (field: string): bigint =>
    new DataView(take(8, field).buffer).getBigUint64(0, true);
  const readHash = (field: string): Uint8Array => take(HASH_LEN, field);

  const bump = readU8("bump");
  const authority = readHash("authority");
  const verifierRouter = readHash("verifier_router");
  const proofSelector = take(4, "proof_selector");

  const pendingTag = readU8("pending_authority tag");
  let pendingAuthority: Uint8Array | null;
  switch (pendingTag) {
    case 0:
      pendingAuthority = null;
      break;
    case 1:
      pendingAuthority = readHash("pending_authority");
      break;
    default:
      throw new PAStateDecodeError(
        `invalid Option tag ${pendingTag} for field pending_authority`,
      );
  }

  const lifecycle = readU8("lifecycle");
  const root = readHash("root");
  const nextIndex = readU64Le("next_index");
  const currentDepth = readU8("current_depth");
  if (currentDepth === 0 || currentDepth > MAX_TREE_DEPTH) {
    throw new PAStateDecodeError(`invalid PA tree depth: ${currentDepth}`);
  }
  const frontierLen = readU32Le("frontier length");
  if (frontierLen < currentDepth) {
    throw new PAStateDecodeError(
      `PA frontier length ${frontierLen} is smaller than depth ${currentDepth}`,
    );
  }
  const frontier: Uint8Array[] = [];
  for (let i = 0; i < frontierLen; i++) {
    frontier.push(readHash("frontier entry"));
  }

  const minExpirySlots = readU64Le("min_expiry_slots");
  const maxExpirySlots = readU64Le("max_expiry_slots");

  return {
    bump,
    authority,
    verifierRouter,
    proofSelector,
    pendingAuthority,
    lifecycle,
    root,
    nextIndex,
    currentDepth,
    frontier,
    minExpirySlots,
    maxExpirySlots,
  };
}
