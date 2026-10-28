//! Wire-level constants shared between the PA on-chain program and its off-chain clients.
//!
//! These values are owned by the PA team. When the PA changes a value (heap budget,
//! compute budget, chunk size), bump it here and integrators get the new value via
//! their next dependency update.

/// Minimum compute-unit limit the settle transaction must request. Groth16 verification
/// plus Merkle updates plus bincode deserialization overrun Solana's default 200K.
pub const SETTLE_COMPUTE_UNIT_LIMIT: u32 = 500_000;

/// Minimum heap-frame bytes the settle transaction must request. Bincode-deserializing
/// a typical ARM transaction overruns Solana's default 32 KB heap.
pub const SETTLE_HEAP_FRAME_BYTES: u32 = 256 * 1024;

/// The compute-unit limit of an upload's init, write and close transactions, each
/// one adapter instruction: the runtime's default budget for one instruction
/// (`DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT`), which a v1 transaction states
/// rather than defaults to.
pub const UPLOAD_COMPUTE_UNIT_LIMIT: u32 = 200_000;

/// The account data a transaction may load: the runtime's maximum
/// (`MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES`, 64 MiB), which legacy and v0
/// transactions get by default and a v1 transaction must state (unset, it is 0).
pub const MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES: u32 = 64 * 1024 * 1024;

/// Chunk size for `txdata_write` instructions: the largest chunk whose write
/// transaction fits a v1 transaction (`v1::MAX_TRANSACTION_SIZE`, 4,096 bytes).
pub const TXDATA_CHUNK_SIZE: usize = 3856;

/// Default expiry window (in slots) for a `tx_data` upload PDA. Roughly 2 minutes
/// at current Solana slot times.
pub const TXDATA_EXPIRY_SLOTS_DEFAULT: u64 = 300;

/// The commitment tree holds at most 2^`MAX_TREE_DEPTH` leaves, as the
/// adapter's; the leaf that fills it grows the tree to depth
/// `MAX_TREE_DEPTH + 1`.
pub const MAX_TREE_DEPTH: usize = 32;

/// The selector a mock seal carries, risc0's convention for a receipt that
/// holds a claim digest instead of a proof. The local validator registers the
/// mock verifier under it.
pub const MOCK_SELECTOR: [u8; 4] = [0xff; 4];
