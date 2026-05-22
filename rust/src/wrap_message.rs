//! Wrap authorization message: the 120-byte struct the user signs (via ed25519)
//! to authorize a wrap. Replaces Permit2 / EIP-712 from the EVM stack.
//!
//! Layout (little-endian where multi-byte):
//!
//! ```text
//! forwarder_id     : [u8; 32]
//! token_mint       : [u8; 32]
//! amount           : u64
//! nonce            : u64
//! deadline         : i64           // signed
//! action_tree_root : [u8; 32]
//! ```
//!
//! Total: 120 bytes. The user signs `base64(sha256(layout))` as UTF-8 text — Phantom
//! refuses raw binary input to `signMessage` because it could be a serialized tx.

use base64::Engine as _;
use sha2::{Digest, Sha256};

/// 120-byte length of the serialized wrap message.
pub const WRAP_MESSAGE_LEN: usize = 120;

/// In-memory representation of a wrap authorization message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WrapMessage {
    pub forwarder_id: [u8; 32],
    pub token_mint: [u8; 32],
    pub amount: u64,
    pub nonce: u64,
    /// Unix timestamp in seconds; signed so the forwarder's `deadline: i64` matches.
    pub deadline: i64,
    pub action_tree_root: [u8; 32],
}

impl WrapMessage {
    /// Serialize to the canonical 120-byte little-endian layout.
    pub fn serialize(&self) -> [u8; WRAP_MESSAGE_LEN] {
        let mut out = [0u8; WRAP_MESSAGE_LEN];
        out[0..32].copy_from_slice(&self.forwarder_id);
        out[32..64].copy_from_slice(&self.token_mint);
        out[64..72].copy_from_slice(&self.amount.to_le_bytes());
        out[72..80].copy_from_slice(&self.nonce.to_le_bytes());
        out[80..88].copy_from_slice(&self.deadline.to_le_bytes());
        out[88..120].copy_from_slice(&self.action_tree_root);
        out
    }

    /// SHA-256 of the serialized 120-byte layout.
    pub fn sha256_digest(&self) -> [u8; 32] {
        let serialized = self.serialize();
        sha256(&serialized)
    }

    /// Base64-encode the SHA-256 digest as UTF-8 text. This is the exact byte
    /// string the wallet's `signMessage` must sign and the on-chain
    /// `Ed25519Program` instruction must carry as its message bytes.
    pub fn base64_digest(&self) -> String {
        base64::engine::general_purpose::STANDARD.encode(self.sha256_digest())
    }
}

/// Convenience: SHA-256 over arbitrary bytes.
pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_message() -> WrapMessage {
        WrapMessage {
            forwarder_id: [1u8; 32],
            token_mint: [2u8; 32],
            amount: 1_000_000,
            nonce: 7,
            deadline: 1_700_000_000,
            action_tree_root: [3u8; 32],
        }
    }

    #[test]
    fn serialize_is_120_bytes() {
        assert_eq!(fixture_message().serialize().len(), 120);
    }

    #[test]
    fn field_offsets_are_stable() {
        let bytes = fixture_message().serialize();
        assert_eq!(&bytes[0..32], &[1u8; 32]); // forwarder_id
        assert_eq!(&bytes[32..64], &[2u8; 32]); // token_mint
        assert_eq!(&bytes[64..72], &1_000_000u64.to_le_bytes()); // amount
        assert_eq!(&bytes[72..80], &7u64.to_le_bytes()); // nonce
        assert_eq!(&bytes[80..88], &1_700_000_000i64.to_le_bytes()); // deadline (signed!)
        assert_eq!(&bytes[88..120], &[3u8; 32]); // action_tree_root
    }

    #[test]
    fn deadline_is_signed_negative_serializes_correctly() {
        let mut msg = fixture_message();
        msg.deadline = -1;
        let bytes = msg.serialize();
        // -1 as i64 LE = 0xFF * 8
        assert_eq!(&bytes[80..88], &[0xFF; 8]);
    }

    #[test]
    fn base64_digest_is_44_chars() {
        // base64 of 32 bytes is always 44 chars (= 4 * ceil(32/3) with one padding char).
        assert_eq!(fixture_message().base64_digest().len(), 44);
    }
}
