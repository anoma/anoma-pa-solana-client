//! On-chain account decoders. Cursor-based parsers that walk the Borsh schema
//! field by field — no hardcoded offsets, so the decoder absorbs PA-side layout
//! changes (new fields, type bumps) at the cost of a re-parse rather than a
//! coordinated cross-repo offset edit.

use crate::constants::{ANCHOR_DISCRIMINATOR_LEN, MAX_TREE_DEPTH};
use crate::cursor::{Cursor, Truncated};

/// The `PAStateAccount` layout number this decoder reads. The PA stores it at
/// byte 8 of the account data, right after the Anchor discriminator, in every
/// layout, and refuses every instruction on an account whose number is not its
/// own; a mismatch seen by a client is a deployment mid-migration.
pub const PA_STATE_SCHEMA_VERSION: u8 = 1;

/// Decoded PA state account. Mirrors the on-chain `PAStateAccount` field by field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PAStateAccount {
    pub schema_version: u8,
    pub bump: u8,
    pub authority: [u8; 32],
    pub verifier_router: [u8; 32],
    pub proof_selector: [u8; 4],
    /// Kind-table commitment every settled aggregation instance must carry.
    pub kind_table_commitment: [u8; 32],
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
    /// The account's layout number is not `PA_STATE_SCHEMA_VERSION`; the
    /// remaining bytes would be misparsed.
    UnsupportedSchemaVersion { found: u8 },
    /// Account data ran out while reading the named field.
    Truncated { field: &'static str },
    /// Encountered an invalid `Option<T>` tag (must be 0 or 1).
    InvalidOptionTag { field: &'static str, tag: u8 },
    /// `current_depth` is outside the valid `[1, MAX_TREE_DEPTH]` range.
    InvalidDepth(u8),
    /// `frontier` declared a length that's smaller than `current_depth`.
    FrontierTooShort { len: usize, depth: usize },
}

impl core::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DecodeError::UnsupportedSchemaVersion { found } => write!(
                f,
                "unsupported PAState schema version {found} (this decoder reads {PA_STATE_SCHEMA_VERSION})"
            ),
            DecodeError::Truncated { field } => {
                write!(f, "PAState truncated while reading {field}")
            }
            DecodeError::InvalidOptionTag { field, tag } => {
                write!(f, "invalid Option tag {tag} for field {field}")
            }
            DecodeError::InvalidDepth(d) => write!(f, "invalid PA tree depth: {d}"),
            DecodeError::FrontierTooShort { len, depth } => {
                write!(f, "PA frontier length {len} is smaller than depth {depth}")
            }
        }
    }
}

/// Decode a raw `PAStateAccount` byte buffer.
///
/// The buffer is the full account-data slice returned by `getAccountInfo`,
/// including the 8-byte Anchor discriminator prefix.
pub fn decode_pa_state(data: &[u8]) -> Result<PAStateAccount, DecodeError> {
    let mut c = Cursor::new(data, ANCHOR_DISCRIMINATOR_LEN);
    let schema_version = c.u8("schema_version")?;
    if schema_version != PA_STATE_SCHEMA_VERSION {
        return Err(DecodeError::UnsupportedSchemaVersion {
            found: schema_version,
        });
    }
    let bump = c.u8("bump")?;
    let authority = c.array_32("authority")?;
    let verifier_router = c.array_32("verifier_router")?;
    let proof_selector: [u8; 4] = c.take(4, "proof_selector")?.try_into().expect("4 bytes");
    let kind_table_commitment = c.array_32("kind_table_commitment")?;

    let pending_authority = match c.u8("pending_authority tag")? {
        0 => None,
        1 => Some(c.array_32("pending_authority")?),
        tag => {
            return Err(DecodeError::InvalidOptionTag {
                field: "pending_authority",
                tag,
            });
        }
    };

    let lifecycle = c.u8("lifecycle")?;
    let root = c.array_32("root")?;
    let next_index = c.u64_le("next_index")?;
    let current_depth = c.u8("current_depth")?;
    let depth = current_depth as usize;
    if depth == 0 || depth > MAX_TREE_DEPTH {
        return Err(DecodeError::InvalidDepth(current_depth));
    }

    let frontier_len = c.u32_le("frontier length")? as usize;
    if frontier_len < depth {
        return Err(DecodeError::FrontierTooShort {
            len: frontier_len,
            depth,
        });
    }
    let frontier = (0..frontier_len)
        .map(|_| c.array_32("frontier entry"))
        .collect::<Result<Vec<_>, _>>()?;

    let min_expiry_slots = c.u64_le("min_expiry_slots")?;
    let max_expiry_slots = c.u64_le("max_expiry_slots")?;

    Ok(PAStateAccount {
        schema_version,
        bump,
        authority,
        verifier_router,
        proof_selector,
        kind_table_commitment,
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

impl From<Truncated> for DecodeError {
    fn from(t: Truncated) -> Self {
        DecodeError::Truncated { field: t.field }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_fixture(pending_some: bool) -> Vec<u8> {
        build_fixture_with_schema(pending_some, PA_STATE_SCHEMA_VERSION)
    }

    /// V2 `PAStateAccount` layout (state.rs): schema_version first, then
    /// bump, authority, verifier_router, proof_selector, kind_table_commitment,
    /// pending_authority, lifecycle, root, next_index, current_depth,
    /// frontier, min/max expiry.
    fn build_fixture_with_schema(pending_some: bool, schema_version: u8) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&[9u8; 8]); // discriminator
        data.push(schema_version); // schema_version
        data.push(255); // bump
        data.extend_from_slice(&[1u8; 32]); // authority
        data.extend_from_slice(&[2u8; 32]); // verifier_router
        data.extend_from_slice(&[0xAB, 0xCD, 0xEF, 0x12]); // proof_selector
        data.extend_from_slice(&[8u8; 32]); // kind_table_commitment
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
        assert_eq!(s.schema_version, PA_STATE_SCHEMA_VERSION);
        assert_eq!(s.bump, 255);
        assert_eq!(s.kind_table_commitment, [8u8; 32]);
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
        // Replace pending_authority tag (byte at offset 8+1+1+32+32+4+32 = 110) with 2.
        data[110] = 2;
        let err = decode_pa_state(&data).expect_err("must reject");
        assert!(matches!(err, DecodeError::InvalidOptionTag { tag: 2, .. }));
    }

    #[test]
    fn rejects_unsupported_schema_version() {
        // The PA refuses every instruction on an account whose layout number
        // is not its own; a client reading another layout would misparse
        // every field after byte 8, so it must refuse too.
        let data = build_fixture_with_schema(false, 2);
        let err = decode_pa_state(&data).expect_err("must reject");
        assert_eq!(err, DecodeError::UnsupportedSchemaVersion { found: 2 });
    }

    #[test]
    fn rejects_truncated_data() {
        let data = build_fixture(false);
        let err = decode_pa_state(&data[..50]).expect_err("must reject");
        assert!(matches!(err, DecodeError::Truncated { .. }));
    }
}
