// Wire-level constants shared between the PA on-chain program and its off-chain clients.
//
// These values are owned by the PA team. When the PA changes a value (heap budget,
// compute budget, chunk size), bump it here and integrators get the new value via
// their next dependency update.

/** Minimum compute-unit limit the settle transaction must request. */
export const SETTLE_COMPUTE_UNIT_LIMIT = 500_000;

/** Minimum heap-frame bytes the settle transaction must request. */
export const SETTLE_HEAP_FRAME_BYTES = 256 * 1024;

/** Chunk size for `txdata_write` instructions. */
export const TXDATA_CHUNK_SIZE = 900;

/** Default expiry window (in slots) for a `tx_data` upload PDA. */
export const TXDATA_EXPIRY_SLOTS_DEFAULT = 300n;

/** Maximum supported commitment-tree depth. */
export const MAX_TREE_DEPTH = 32;

/**
 * Number of accounts in a wrap forwarder CPI segment.
 *
 * Ordering: `[forwarder_program, config_pda, ix_sysvar, clock_sysvar, user_ata,
 * escrow_ata, escrow_pda, nonce_bitmap_pda, token_program, system_program, payer,
 * token_mint]`.
 */
export const FORWARDER_WRAP_NUM_ACCOUNTS = 12;

/**
 * Number of accounts in an unwrap forwarder CPI segment.
 *
 * Ordering: `[forwarder_program, config_pda, ix_sysvar, clock_sysvar, escrow_ata,
 * recipient_ata, escrow_pda, token_program, token_mint]`.
 */
export const FORWARDER_UNWRAP_NUM_ACCOUNTS = 9;

/** Groth16 proof selector for the verifier-router lookup. */
export const GROTH16_VERIFIER_SELECTOR = new Uint8Array([0x73, 0xc4, 0x57, 0xba]);

/** Anchor account-discriminator prefix length (constant across all Anchor programs). */
export const ANCHOR_DISCRIMINATOR_LEN = 8;

/** 32-byte length used by Pubkey, Digest, commitment hashes, and nullifier bytes. */
export const HASH_LEN = 32;
