// Anchor discriminator computation.
//
// Every Anchor instruction and event is identified on-chain by an 8-byte prefix
// computed as `sha256("<namespace>:<name>")[..8]`. Off-chain clients building
// instructions or decoding events compute the same prefix.

import { sha256 } from "@noble/hashes/sha2";

import { ANCHOR_DISCRIMINATOR_LEN } from "./constants.js";

/** Compute an Anchor instruction discriminator: first 8 bytes of sha256("global:<name>"). */
export function anchorInstructionDisc(name: string): Uint8Array {
  return anchorDisc("global", name);
}

/** Compute an Anchor event discriminator: first 8 bytes of sha256("event:<name>"). */
export function anchorEventDisc(name: string): Uint8Array {
  return anchorDisc("event", name);
}

function anchorDisc(namespace: string, name: string): Uint8Array {
  const preimage = new TextEncoder().encode(`${namespace}:${name}`);
  return sha256(preimage).slice(0, ANCHOR_DISCRIMINATOR_LEN);
}
