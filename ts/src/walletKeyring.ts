/**
 * Wallet-derived keyring derivation.
 *
 * Mirrors `pay-interface-app/src/domain/keys/services.ts::createUserKeyringFromIkm`
 * and `pay-interface-app/src/domain/keys/models.ts::KeyPair.generatePrivateKey`:
 *
 *   1. Caller signs `signMessage(address)` with the wallet (ed25519 for Solana).
 *   2. The 64-byte signature is the IKM passed to {@link deriveKeyringSecrets}.
 *   3. HKDF-SHA256(IKM, KEYRING_SALT, "", 32) → seed.
 *   4. For each domain (authority / nullifier / encryption / discovery):
 *      HMAC-SHA256(key=seed, message=domain) → 32-byte secret.
 *
 * Pubkey derivation is left to callers so this module stays free of a
 * secp256k1 dependency. Three of the four secrets (authority, encryption,
 * discovery) are secp256k1 secret keys; the fourth (nullifier) is fed into
 * SHA-256 to produce the nullifier-key commitment (see
 * {@link nullifierCommitment}).
 *
 * The Rust mirror lives at `rust/src/wallet_keyring.rs` and is parity-tested
 * against the fixtures used here.
 */

import { hkdf } from "@noble/hashes/hkdf";
import { hmac } from "@noble/hashes/hmac";
import { sha256 } from "@noble/hashes/sha2";

/** HKDF salt — must match the Rust mirror's `KEYRING_SALT`. */
export const KEYRING_SALT = "anoma-pa:keyring-seed";

/** PRF domain separator for the authority signing key (secp256k1 secret). */
export const AUTHORITY_DOMAIN = "ANOMA_AUTHORITY_KEY";

/** PRF domain separator for the nullifier key. */
export const NULLIFIER_DOMAIN = "ANOMA_NULLIFIER_KEY";

/** PRF domain separator for the resource-encryption key (secp256k1 secret). */
export const ENCRYPTION_DOMAIN = "ANOMA_STATIC_ENCRYPTION_KEY";

/** PRF domain separator for the discovery key (secp256k1 secret). */
export const DISCOVERY_DOMAIN = "ANOMA_STATIC_DISCOVERY_KEY";

/**
 * Canonical wallet-sign message — must match
 * `pay-interface-app/src/signing-message.ts::getSignMessage`.
 *
 * The returned string is what the wallet signs to produce the IKM passed to
 * {@link deriveKeyringSecrets}. Any drift from the frontend's exact bytes
 * here will produce a different keyring.
 */
export function signMessage(address: string): string {
  return (
    `I authorize AnomaPay to derive my account from address ${address}.\n` +
    `Do NOT sign this message if the request url is not https://beta.anomapay.app/`
  );
}

/**
 * The four 32-byte secrets that make up a user keyring.
 *
 * `authority`, `encryption`, and `discovery` are secp256k1 secret keys;
 * callers compute their compressed pubkeys with whichever secp256k1 library
 * they prefer. `nullifier` is the raw nk byte string; its commitment is
 * `SHA-256(nullifier)` (see {@link nullifierCommitment}).
 */
export interface KeyringSecrets {
  authority: Uint8Array;
  nullifier: Uint8Array;
  encryption: Uint8Array;
  discovery: Uint8Array;
}

/**
 * Derive the four keyring secrets from a wallet signature over
 * `signMessage(walletAddress)`.
 *
 * `signatureIkm` is treated as opaque key material — for a Solana wallet,
 * this is the 64-byte ed25519 signature bytes.
 */
export function deriveKeyringSecrets(
  signatureIkm: Uint8Array
): KeyringSecrets {
  const salt = new TextEncoder().encode(KEYRING_SALT);
  const seed = hkdf(sha256, signatureIkm, salt, "", 32);
  return {
    authority: prf(seed, AUTHORITY_DOMAIN),
    nullifier: prf(seed, NULLIFIER_DOMAIN),
    encryption: prf(seed, ENCRYPTION_DOMAIN),
    discovery: prf(seed, DISCOVERY_DOMAIN),
  };
}

/** Nullifier-key commitment used in resources' `nk_commitment` field. */
export function nullifierCommitment(nk: Uint8Array): Uint8Array {
  return sha256(nk);
}

function prf(seed: Uint8Array, domain: string): Uint8Array {
  return hmac(sha256, seed, new TextEncoder().encode(domain));
}
