//! On-chain account decoders. Cursor-based parsers that walk the Borsh schema
//! field by field — no hardcoded offsets, so the decoder absorbs PA-side layout
//! changes (new fields, type bumps) at the cost of a re-parse rather than a
//! coordinated cross-repo offset edit.

use crate::constants::{ANCHOR_DISCRIMINATOR_LEN, HASH_LEN, MAX_TREE_DEPTH};

/// Decoded PA state account. Mirrors the on-chain `PAStateAccount` field by field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PAStateAccount {
    pub bump: u8,
    pub authority: [u8; 32],
    pub verifier_router: [u8; 32],
    pub proof_selector: [u8; 4],
    pub pending_authority: Option<[u8; 32]>,
    pub lifecycle: u8,
    pub root: [u8; 32],
    pub next_index: u64,
    pub current_depth: u8,
    pub frontier: Vec<[u8; 32]>,
    pub min_expiry_slots: u64,
    pub max_expiry_slots: u64,
}

/// Errors produced by the PA state decoder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// Account data ran out while reading the named field.
    Truncated { field: &'static str },
    /// Encountered an invalid `Option<T>` tag (must be 0 or 1).
    InvalidOptionTag { field: &'static str, tag: u8 },
    /// `current_depth` is outside the valid `[1, MAX_TREE_DEPTH]` range.
    InvalidDepth(u8),
    /// `frontier` declared a length that's smaller than `current_depth`.
    FrontierTooShort { len: usize, depth: usize },
    /// An array slice didn't have the expected fixed length (should not happen
    /// in practice, but exposed as an error rather than a panic).
    InvalidLength { field: &'static str },
}

impl core::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DecodeError::Truncated { field } => {
                write!(f, "PAState truncated while reading {field}")
            }
            DecodeError::InvalidOptionTag { field, tag } => {
                write!(f, "invalid Option tag {tag} for field {field}")
            }
            DecodeError::InvalidDepth(d) => write!(f, "invalid PA tree depth: {d}"),
            DecodeError::FrontierTooShort { len, depth } => write!(
                f,
                "PA frontier length {len} is smaller than depth {depth}"
            ),
            DecodeError::InvalidLength { field } => write!(f, "invalid {field} length"),
        }
    }
}

/// Decode a raw `PAStateAccount` byte buffer.
///
/// The buffer is the full account-data slice returned by `getAccountInfo`,
/// including the 8-byte Anchor discriminator prefix.
pub fn decode_pa_state(data: &[u8]) -> Result<PAStateAccount, DecodeError> {
    let mut cursor = ANCHOR_DISCRIMINATOR_LEN;
    let bump = read_u8(data, &mut cursor, "bump")?;
    let authority = read_array_32(data, &mut cursor, "authority")?;
    let verifier_router = read_array_32(data, &mut cursor, "verifier_router")?;
    let proof_selector_slice = take(data, &mut cursor, 4, "proof_selector")?;
    let mut proof_selector = [0u8; 4];
    proof_selector.copy_from_slice(proof_selector_slice);

    let pending_tag = read_u8(data, &mut cursor, "pending_authority tag")?;
    let pending_authority = match pending_tag {
        0 => None,
        1 => Some(read_array_32(data, &mut cursor, "pending_authority")?),
        tag => {
            return Err(DecodeError::InvalidOptionTag {
                field: "pending_authority",
                tag,
            });
        }
    };

    let lifecycle = read_u8(data, &mut cursor, "lifecycle")?;
    let root = read_array_32(data, &mut cursor, "root")?;
    let next_index = read_u64_le(data, &mut cursor, "next_index")?;
    let current_depth = read_u8(data, &mut cursor, "current_depth")?;
    let depth = current_depth as usize;
    if depth == 0 || depth > MAX_TREE_DEPTH {
        return Err(DecodeError::InvalidDepth(current_depth));
    }

    let frontier_len = read_u32_le(data, &mut cursor, "frontier length")? as usize;
    if frontier_len < depth {
        return Err(DecodeError::FrontierTooShort {
            len: frontier_len,
            depth,
        });
    }
    let mut frontier = Vec::with_capacity(frontier_len);
    for _ in 0..frontier_len {
        frontier.push(read_array_32(data, &mut cursor, "frontier entry")?);
    }

    let min_expiry_slots = read_u64_le(data, &mut cursor, "min_expiry_slots")?;
    let max_expiry_slots = read_u64_le(data, &mut cursor, "max_expiry_slots")?;

    Ok(PAStateAccount {
        bump,
        authority,
        verifier_router,
        proof_selector,
        pending_authority,
        lifecycle,
        root,
        next_index,
        current_depth,
        frontier,
        min_expiry_slots,
        max_expiry_slots,
    })
}

fn take<'a>(
    data: &'a [u8],
    cursor: &mut usize,
    len: usize,
    field: &'static str,
) -> Result<&'a [u8], DecodeError> {
    let end = cursor
        .checked_add(len)
        .ok_or(DecodeError::Truncated { field })?;
    let slice = data.get(*cursor..end).ok_or(DecodeError::Truncated { field })?;
    *cursor = end;
    Ok(slice)
}

fn read_u8(data: &[u8], cursor: &mut usize, field: &'static str) -> Result<u8, DecodeError> {
    Ok(take(data, cursor, 1, field)?[0])
}

fn read_u32_le(data: &[u8], cursor: &mut usize, field: &'static str) -> Result<u32, DecodeError> {
    let bytes: [u8; 4] = take(data, cursor, 4, field)?
        .try_into()
        .map_err(|_| DecodeError::InvalidLength { field })?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_u64_le(data: &[u8], cursor: &mut usize, field: &'static str) -> Result<u64, DecodeError> {
    let bytes: [u8; 8] = take(data, cursor, 8, field)?
        .try_into()
        .map_err(|_| DecodeError::InvalidLength { field })?;
    Ok(u64::from_le_bytes(bytes))
}

fn read_array_32(
    data: &[u8],
    cursor: &mut usize,
    field: &'static str,
) -> Result<[u8; 32], DecodeError> {
    take(data, cursor, HASH_LEN, field)?
        .try_into()
        .map_err(|_| DecodeError::InvalidLength { field })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_fixture(pending_some: bool) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&[9u8; 8]); // discriminator
        data.push(255); // bump
        data.extend_from_slice(&[1u8; 32]); // authority
        data.extend_from_slice(&[2u8; 32]); // verifier_router
        data.extend_from_slice(&[0xAB, 0xCD, 0xEF, 0x12]); // proof_selector
        if pending_some {
            data.push(1);
            data.extend_from_slice(&[3u8; 32]);
        } else {
            data.push(0);
        }
        data.push(0); // lifecycle = Running
        data.extend_from_slice(&[4u8; 32]); // root
        data.extend_from_slice(&100u64.to_le_bytes()); // next_index
        data.push(3); // current_depth
        data.extend_from_slice(&3u32.to_le_bytes()); // frontier len
        data.extend_from_slice(&[5u8; 32]);
        data.extend_from_slice(&[6u8; 32]);
        data.extend_from_slice(&[7u8; 32]);
        data.extend_from_slice(&100u64.to_le_bytes()); // min_expiry_slots
        data.extend_from_slice(&216_000u64.to_le_bytes()); // max_expiry_slots
        data
    }

    #[test]
    fn decodes_state_with_no_pending_authority() {
        let data = build_fixture(false);
        let s = decode_pa_state(&data).expect("decode");
        assert_eq!(s.bump, 255);
        assert_eq!(s.authority, [1u8; 32]);
        assert_eq!(s.proof_selector, [0xAB, 0xCD, 0xEF, 0x12]);
        assert!(s.pending_authority.is_none());
        assert_eq!(s.root, [4u8; 32]);
        assert_eq!(s.next_index, 100);
        assert_eq!(s.current_depth, 3);
        assert_eq!(s.frontier.len(), 3);
        assert_eq!(s.max_expiry_slots, 216_000);
    }

    #[test]
    fn decodes_state_with_pending_authority() {
        let data = build_fixture(true);
        let s = decode_pa_state(&data).expect("decode");
        assert_eq!(s.pending_authority, Some([3u8; 32]));
    }

    #[test]
    fn rejects_invalid_option_tag() {
        let mut data = build_fixture(false);
        // Replace pending_authority tag (byte at offset 8+1+32+32+4 = 77) with 2.
        data[77] = 2;
        let err = decode_pa_state(&data).expect_err("must reject");
        assert!(matches!(err, DecodeError::InvalidOptionTag { tag: 2, .. }));
    }

    #[test]
    fn rejects_truncated_data() {
        let data = build_fixture(false);
        let err = decode_pa_state(&data[..50]).expect_err("must reject");
        assert!(matches!(err, DecodeError::Truncated { .. }));
    }
}
