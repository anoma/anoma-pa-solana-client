// Wire-level constants shared between the PA on-chain program and its off-chain clients.
//
// These values are owned by the PA team. When the PA changes a value (heap budget,
// compute budget, chunk size), bump it here and integrators get the new value via
// their next dependency update.

/** Minimum compute-unit limit the settle transaction must request. */
export const SETTLE_COMPUTE_UNIT_LIMIT = 500_000;

/** Minimum heap-frame bytes the settle transaction must request. */
export const SETTLE_HEAP_FRAME_BYTES = 256 * 1024;

/**
 * The compute-unit limit of an upload's init, write and close transactions, each
 * one adapter instruction: the runtime's default budget for one instruction,
 * which a v1 transaction states rather than defaults to.
 */
export const UPLOAD_COMPUTE_UNIT_LIMIT = 200_000;

/**
 * The account data a transaction may load: the runtime's maximum (64 MiB), which
 * a v1 transaction must state (unset, it is 0).
 */
export const MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES = 64 * 1024 * 1024;

/** Chunk size for `txdata_write` instructions: the largest whose write transaction fits a v1 transaction. */
export const TXDATA_CHUNK_SIZE = 3856;

/** Default expiry window (in slots) for a `tx_data` upload PDA. */
export const TXDATA_EXPIRY_SLOTS_DEFAULT = 300n;

/** Maximum supported commitment-tree depth. */
export const MAX_TREE_DEPTH = 32;

/**
 * The selector a mock seal carries, risc0's convention for a receipt that
 * holds a claim digest instead of a proof. The local validator registers the
 * mock verifier under it.
 */
export const MOCK_SELECTOR = new Uint8Array([0xff, 0xff, 0xff, 0xff]);

/** Anchor account-discriminator prefix length (constant across all Anchor programs). */
export const ANCHOR_DISCRIMINATOR_LEN = 8;

/** 32-byte length used by Pubkey, Digest, commitment hashes, and nullifier bytes. */
export const HASH_LEN = 32;
