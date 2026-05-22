//! Commitment-tree replay primitives. Mirror the PA's on-chain Merkle tree so
//! the integrator can predict the post-settle root and derive the corresponding
//! root-marker PDA without waiting for confirmation.

use sha2::{Digest as _, Sha256};

use crate::constants::MAX_TREE_DEPTH;

/// `sha256("")`, the canonical padding leaf used for empty positions in the
/// commitment tree. Same value as the PA's `EMPTY_HASH_BYTES`.
pub const PADDING_LEAF: [u8; 32] = [
    0xcc, 0x1d, 0x2f, 0x83, 0x84, 0x45, 0xdb, 0x7a, 0xec, 0x43, 0x1d, 0xf9, 0xee, 0x8a, 0x87, 0x1f,
    0x40, 0xe7, 0xaa, 0x5e, 0x06, 0x4f, 0xc0, 0x56, 0x63, 0x3e, 0xf8, 0xc6, 0x0f, 0xab, 0x7b, 0x06,
];

/// SHA-256 hash of `left || right`.
pub fn hash_two(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(left);
    hasher.update(right);
    hasher.finalize().into()
}

/// Precomputed zero hashes at each level of the commitment tree.
///
/// `zero_hashes()[0] = PADDING_LEAF`, then each subsequent level hashes the
/// previous level's pair: `H[i] = hash_two(H[i-1], H[i-1])`.
pub fn zero_hashes() -> [[u8; 32]; MAX_TREE_DEPTH] {
    let mut zeros = [[0u8; 32]; MAX_TREE_DEPTH];
    zeros[0] = PADDING_LEAF;
    for level in 1..MAX_TREE_DEPTH {
        zeros[level] = hash_two(&zeros[level - 1], &zeros[level - 1]);
    }
    zeros
}

/// Replayable commitment-tree state. Mirrors the PA's `frontier` representation
/// (filled-subtree hashes at each level) so the integrator can predict the next
/// root for the next settlement.
#[derive(Clone, Debug)]
pub struct CommitmentTreeState {
    pub root: [u8; 32],
    pub next_index: u64,
    pub current_depth: u8,
    pub frontier: Vec<[u8; 32]>,
}

/// Errors produced by the Merkle replay.
#[derive(Debug)]
pub enum MerkleError {
    NextIndexOverflow,
    CapacityOverflow,
    MaxDepthReached,
}

impl core::fmt::Display for MerkleError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            MerkleError::NextIndexOverflow => write!(f, "tree next_index overflow"),
            MerkleError::CapacityOverflow => write!(f, "tree capacity overflow"),
            MerkleError::MaxDepthReached => write!(f, "commitment tree max depth reached"),
        }
    }
}

impl CommitmentTreeState {
    /// Append a new commitment to the tree, updating `root`, `next_index`,
    /// `frontier`, and (if the tree fills at the current depth) `current_depth`.
    pub fn append(&mut self, leaf: [u8; 32]) -> Result<(), MerkleError> {
        let depth = self.current_depth as usize;
        let zeros = zero_hashes();
        let mut index = self.next_index;
        self.next_index = self
            .next_index
            .checked_add(1)
            .ok_or(MerkleError::NextIndexOverflow)?;

        let mut current = leaf;
        for (level, zero) in zeros.iter().enumerate().take(depth) {
            if index & 1 == 0 {
                self.frontier[level] = current;
                current = hash_two(&current, zero);
            } else {
                current = hash_two(&self.frontier[level], &current);
            }
            index >>= 1;
        }

        let capacity = 1u64
            .checked_shl(self.current_depth as u32)
            .ok_or(MerkleError::CapacityOverflow)?;
        if self.next_index == capacity {
            if depth >= MAX_TREE_DEPTH {
                return Err(MerkleError::MaxDepthReached);
            }
            self.frontier.push(current);
            self.current_depth += 1;
            current = hash_two(&current, &zeros[depth]);
        }

        self.root = current;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn padding_leaf_matches_arm_risc0_empty_hash() {
        // PADDING_LEAF must equal arm_core::constants::EMPTY_HASH_BYTES — the
        // RISC0-flavored "empty input hash" the PA uses for empty tree slots.
        // It is NOT standard SHA-256("") (which is e3b0c4…); arm-risc0 uses a
        // RISC0 SHA-256 variant whose empty-input hash is cc1d2f…
        assert_eq!(
            PADDING_LEAF,
            [
                0xcc, 0x1d, 0x2f, 0x83, 0x84, 0x45, 0xdb, 0x7a, 0xec, 0x43, 0x1d, 0xf9, 0xee, 0x8a,
                0x87, 0x1f, 0x40, 0xe7, 0xaa, 0x5e, 0x06, 0x4f, 0xc0, 0x56, 0x63, 0x3e, 0xf8, 0xc6,
                0x0f, 0xab, 0x7b, 0x06,
            ]
        );
    }

    #[test]
    fn zero_hashes_chain_correctly() {
        let zeros = zero_hashes();
        for level in 1..MAX_TREE_DEPTH {
            assert_eq!(zeros[level], hash_two(&zeros[level - 1], &zeros[level - 1]));
        }
    }

    #[test]
    fn append_single_leaf_advances_index() {
        let mut state = CommitmentTreeState {
            root: PADDING_LEAF,
            next_index: 0,
            current_depth: 1,
            frontier: vec![[0u8; 32]],
        };
        state.append([7u8; 32]).expect("append");
        assert_eq!(state.next_index, 1);
    }
}
