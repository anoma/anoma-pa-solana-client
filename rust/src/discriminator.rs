//! Anchor discriminator computation.
//!
//! Every Anchor instruction and event is identified on-chain by an 8-byte prefix
//! computed as `sha256("<namespace>:<name>")[..8]`. Off-chain clients building
//! instructions or decoding events compute the same prefix.

use sha2::{Digest, Sha256};

use crate::constants::ANCHOR_DISCRIMINATOR_LEN;

/// Compute an Anchor instruction discriminator: first 8 bytes of `sha256("global:<name>")`.
pub fn anchor_instruction_disc(name: &str) -> [u8; ANCHOR_DISCRIMINATOR_LEN] {
    anchor_disc("global", name)
}

/// Compute an Anchor event discriminator: first 8 bytes of `sha256("event:<name>")`.
pub fn anchor_event_disc(name: &str) -> [u8; ANCHOR_DISCRIMINATOR_LEN] {
    anchor_disc("event", name)
}

fn anchor_disc(namespace: &str, name: &str) -> [u8; ANCHOR_DISCRIMINATOR_LEN] {
    let mut hasher = Sha256::new();
    hasher.update(namespace.as_bytes());
    hasher.update(b":");
    hasher.update(name.as_bytes());
    let digest = hasher.finalize();
    let mut out = [0u8; ANCHOR_DISCRIMINATOR_LEN];
    out.copy_from_slice(&digest[..ANCHOR_DISCRIMINATOR_LEN]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_call_instruction_disc_matches_pa_constant() {
        // PA's external_calls/cpi.rs uses `0x9faae00afd696cde` as the forward_call discriminator.
        let disc = anchor_instruction_disc("forward_call");
        assert_eq!(disc, [0x9f, 0xaa, 0xe0, 0x0a, 0xfd, 0x69, 0x6c, 0xde]);
    }

    #[test]
    fn discriminator_is_deterministic() {
        assert_eq!(
            anchor_instruction_disc("settle"),
            anchor_instruction_disc("settle")
        );
    }

    #[test]
    fn instruction_and_event_discriminators_differ() {
        assert_ne!(
            anchor_instruction_disc("settle"),
            anchor_event_disc("settle")
        );
    }
}
