// Helpers for computing the AnomaPay-specific `label_ref` and `value_ref`
// digests that the WASM ARM library expects when constructing resources.

import { sha256 } from "@noble/hashes/sha2";
import bs58 from "bs58";

const concat = (...parts: Uint8Array[]): Uint8Array => {
  const total = parts.reduce((acc, p) => acc + p.length, 0);
  const out = new Uint8Array(total);
  let off = 0;
  for (const p of parts) {
    out.set(p, off);
    off += p.length;
  }
  return out;
};

const hash = (bytes: Uint8Array): Uint8Array => {
  const digest = sha256(bytes);
  return new Uint8Array(digest);
};

/**
 * Resource `label_ref`: `sha256(forwarderProgramId(32) || tokenMint(32))`.
 *
 * Both inputs may be base58 strings (wallet/program addresses) or raw 32-byte
 * buffers. Output is 32 bytes.
 */
export function calculateLabelRef(
  forwarderProgram: string | Uint8Array,
  tokenMint: string | Uint8Array,
): Uint8Array {
  const fw = typeof forwarderProgram === "string" ? bs58.decode(forwarderProgram) : forwarderProgram;
  const mint = typeof tokenMint === "string" ? bs58.decode(tokenMint) : tokenMint;
  return hash(concat(fw, mint));
}

/**
 * Resource `value_ref` for a shielded resource owned by an authority key + an
 * encryption key: `sha256(authVK(32) || encPub(33|65))`.
 */
export function calculateValueRefFromAuth(
  authVerifyingKey: Uint8Array,
  encryptionPubkey: Uint8Array,
): Uint8Array {
  return hash(concat(authVerifyingKey, encryptionPubkey));
}

/**
 * Resource `value_ref` for an ephemeral resource at a user's wallet address:
 * the 32-byte ed25519 pubkey, base58-decoded. No hashing.
 */
export function calculateValueRefFromUserAddress(
  userWalletAddress: string,
): Uint8Array {
  const bytes = bs58.decode(userWalletAddress);
  if (bytes.length !== 32) {
    throw new Error(`user wallet address must decode to 32 bytes; got ${bytes.length}`);
  }
  return bytes;
}
