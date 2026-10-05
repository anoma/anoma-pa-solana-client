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

/// Chunk size for `txdata_write` instructions. Chosen to keep each write ix under
/// Solana's 1232-byte transaction limit after accounts, signatures, and discriminator.
pub const TXDATA_CHUNK_SIZE: usize = 900;

/// Default expiry window (in slots) for a `tx_data` upload PDA. Roughly 2 minutes
/// at current Solana slot times.
pub const TXDATA_EXPIRY_SLOTS_DEFAULT: u64 = 300;

/// Maximum supported commitment-tree depth. Mirrors the PA's tree cap.
pub const MAX_TREE_DEPTH: usize = 32;

/// The selector a mock seal carries, risc0's convention for a receipt that
/// holds a claim digest instead of a proof. The local validator registers the
/// mock verifier under it.
pub const MOCK_SELECTOR: [u8; 4] = [0xff; 4];

/// Anchor account-discriminator prefix length (constant across all Anchor programs).
pub const ANCHOR_DISCRIMINATOR_LEN: usize = 8;

/// 32-byte length used by `Pubkey`, `Digest`, commitment hashes, and nullifier bytes.
pub const HASH_LEN: usize = 32;
