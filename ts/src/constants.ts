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

/**
 * The depth of the deepest commitment tree, as the adapter's: the one holding
 * 2^32 leaves, since the leaf that fills the depth-32 tree grows it.
 */
export const MAX_TREE_DEPTH = 33;

/**
 * The selector a mock seal carries, risc0's convention for a receipt that
 * holds a claim digest instead of a proof. The local validator registers the
 * mock verifier under it.
 */
export const MOCK_SELECTOR = new Uint8Array([0xff, 0xff, 0xff, 0xff]);

/**
 * The commitment tree's padding leaf, the leaf of every empty position: the
 * ARM empty-hash constant (the PA's `EMPTY_HASH_WORDS`), as the Rust crate's
 * `PADDING_LEAF`. Not `sha256("")` (`e3b0c442…`).
 */
export const PADDING_LEAF = new Uint8Array([
  0xcc, 0x1d, 0x2f, 0x83, 0x84, 0x45, 0xdb, 0x7a, 0xec, 0x43, 0x1d, 0xf9, 0xee, 0x8a, 0x87, 0x1f,
  0x40, 0xe7, 0xaa, 0x5e, 0x06, 0x4f, 0xc0, 0x56, 0x63, 0x3e, 0xf8, 0xc6, 0x0f, 0xab, 0x7b, 0x06,
]);
