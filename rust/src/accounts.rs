//! On-chain account decoders. Cursor-based parsers that walk the Borsh schema
//! field by field — no hardcoded offsets, so the decoder absorbs PA-side layout
//! changes (new fields, type bumps) at the cost of a re-parse rather than a
//! coordinated cross-repo offset edit.

use crate::constants::{ANCHOR_DISCRIMINATOR_LEN, MAX_TREE_DEPTH};
use crate::cursor::{Cursor, Truncated};
use crate::discriminator::anchor_account_disc;
use crate::merkle::CommitmentTreeState;

/// The `PAStateAccount` layout number this decoder reads. The PA stores it at
/// byte 8 of the account data, right after the Anchor discriminator, in every
/// layout, and refuses every instruction on an account whose number is not its
/// own; a mismatch seen by a client is a deployment mid-migration.
pub const PA_STATE_SCHEMA_VERSION: u8 = 4;

/// Decoded PA state account. Mirrors the on-chain `PAStateAccount` field by field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PAStateAccount {
    pub schema_version: u8,
    pub bump: u8,
    /// The adapter's owner, who signs every owner-only instruction and
    /// upgrades the program; all zeros once renounced.
    pub owner: [u8; 32],
    pub verifier_router: [u8; 32],
    pub proof_selector: [u8; 4],
    /// The stored kind-table commitment: a transaction settles when proven
    /// against that table or against the empty one.
    pub kind_table_commitment: [u8; 32],
    /// Whether settlement is paused (the owner's `pause` / `unpause`).
    pub paused: bool,
    pub root: [u8; 32],
    pub next_index: u64,
    pub current_depth: u8,
    pub frontier: Vec<[u8; 32]>,
    pub min_expiry_slots: u64,
    pub max_expiry_slots: u64,
    /// The denylist for consumed resources: no settlement consumes a
    /// resource whose logic ref the owner added to it.
    pub denied_consumed_logic_refs: Vec<[u8; 32]>,
    /// The denylist for created resources: no settlement creates a resource
    /// whose logic ref the owner added to it.
    pub denied_created_logic_refs: Vec<[u8; 32]>,
}

/// The commitment tree the adapter stores, to replay the leaves a settlement
/// appends.
impl From<&PAStateAccount> for CommitmentTreeState {
    fn from(state: &PAStateAccount) -> Self {
        CommitmentTreeState {
            root: state.root,
            next_index: state.next_index,
            current_depth: state.current_depth,
            frontier: state.frontier.clone(),
        }
    }
}

/// Errors produced by the PA state decoder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// The account's layout number is not `PA_STATE_SCHEMA_VERSION`; the
    /// remaining bytes would be misparsed.
    UnsupportedSchemaVersion { found: u8 },
    /// Account data ran out while reading the named field.
    Truncated { field: &'static str },
    /// Encountered an invalid Borsh `bool` byte (must be 0 or 1).
    InvalidBool { field: &'static str, byte: u8 },
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
            DecodeError::InvalidBool { field, byte } => {
                write!(f, "invalid bool byte {byte} for field {field}")
            }
            DecodeError::InvalidDepth(d) => write!(f, "invalid PA tree depth: {d}"),
            DecodeError::FrontierTooShort { len, depth } => {
                write!(f, "PA frontier length {len} is smaller than depth {depth}")
            }
        }
    }
}

impl std::error::Error for DecodeError {}

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
    let owner = c.array_32("owner")?;
    let verifier_router = c.array_32("verifier_router")?;
    let proof_selector: [u8; 4] = c.take(4, "proof_selector")?.try_into().expect("4 bytes");
    let kind_table_commitment = c.array_32("kind_table_commitment")?;
    let paused = bool_field(&mut c, "paused")?;
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
    let denied_consumed_logic_refs = c.vec_array_32("denied_consumed_logic_refs")?;
    let denied_created_logic_refs = c.vec_array_32("denied_created_logic_refs")?;

    Ok(PAStateAccount {
        schema_version,
        bump,
        owner,
        verifier_router,
        proof_selector,
        kind_table_commitment,
        paused,
        root,
        next_index,
        current_depth,
        frontier,
        min_expiry_slots,
        max_expiry_slots,
        denied_consumed_logic_refs,
        denied_created_logic_refs,
    })
}

/// Encode `state` as the PA stores it: the `PAStateAccount` discriminator,
/// then the fields `decode_pa_state` reads, in its order. A test runtime
/// writes this over an adapter's state account (an account's allocation can
/// exceed the encoding; the PA ignores the bytes after it).
pub fn encode_pa_state(state: &PAStateAccount) -> Vec<u8> {
    let mut data = anchor_account_disc("PAStateAccount").to_vec();
    data.push(state.schema_version);
    data.push(state.bump);
    data.extend_from_slice(&state.owner);
    data.extend_from_slice(&state.verifier_router);
    data.extend_from_slice(&state.proof_selector);
    data.extend_from_slice(&state.kind_table_commitment);
    data.push(u8::from(state.paused));
    data.extend_from_slice(&state.root);
    data.extend_from_slice(&state.next_index.to_le_bytes());
    data.push(state.current_depth);
    push_vec_array_32(&mut data, &state.frontier);
    data.extend_from_slice(&state.min_expiry_slots.to_le_bytes());
    data.extend_from_slice(&state.max_expiry_slots.to_le_bytes());
    push_vec_array_32(&mut data, &state.denied_consumed_logic_refs);
    push_vec_array_32(&mut data, &state.denied_created_logic_refs);
    data
}

/// Appends `list` as Borsh writes a `Vec<[u8; 32]>`: its length, then its
/// entries.
fn push_vec_array_32(data: &mut Vec<u8>, list: &[[u8; 32]]) {
    let len = u32::try_from(list.len()).expect("a PA state list fits a Borsh length");
    data.extend_from_slice(&len.to_le_bytes());
    for entry in list {
        data.extend_from_slice(entry);
    }
}

impl From<Truncated> for DecodeError {
    fn from(t: Truncated) -> Self {
        DecodeError::Truncated { field: t.field }
    }
}

/// A verifier router's `VerifierEntry`: the verifier program registered under
/// a selector, and whether the router has paused it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifierEntryAccount {
    pub selector: [u8; 4],
    pub verifier: [u8; 32],
    pub paused: bool,
}

/// Decode a router `VerifierEntry` account (Anchor discriminator, then
/// selector, verifier, paused), as the router's IDL lays it out.
pub fn decode_verifier_entry(data: &[u8]) -> Result<VerifierEntryAccount, DecodeError> {
    let mut c = Cursor::new(data, ANCHOR_DISCRIMINATOR_LEN);
    let selector = c.take(4, "selector")?.try_into().expect("4 bytes");
    let verifier = c.array_32("verifier")?;
    let paused = bool_field(&mut c, "paused")?;
    Ok(VerifierEntryAccount {
        selector,
        verifier,
        paused,
    })
}

/// A Borsh `bool`: one byte, 0 or 1.
fn bool_field(c: &mut Cursor<'_>, field: &'static str) -> Result<bool, DecodeError> {
    match c.u8(field)? {
        0 => Ok(false),
        1 => Ok(true),
        byte => Err(DecodeError::InvalidBool { field, byte }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_the_committed_mock_verifier_entry() {
        // The adapter repo's tests/fixtures/verifier-entries entry that
        // registers the localnet mock verifier
        // (H3ZFoDHFvthGZu3kxpif3oSWm8MQn8uKvgDhrvVVHvHf) under 0xffffffff.
        use base64::Engine;
        let data = base64::engine::general_purpose::STANDARD
            .decode("ZveUniGZZF3/////7mKfmDSxMGCnyy/TbpmX7hTkzxHBC9Wi03rWqde0u7oA")
            .unwrap();
        let entry = decode_verifier_entry(&data).unwrap();
        assert_eq!(entry.selector, crate::MOCK_SELECTOR);
        assert!(!entry.paused);
        let mut paused = data.clone();
        paused[44] = 1;
        assert!(decode_verifier_entry(&paused).unwrap().paused);
        paused[44] = 2;
        assert_eq!(
            decode_verifier_entry(&paused),
            Err(DecodeError::InvalidBool {
                field: "paused",
                byte: 2
            })
        );
        assert_eq!(
            decode_verifier_entry(&data[..20]),
            Err(DecodeError::Truncated { field: "verifier" })
        );
        #[cfg(feature = "solana")]
        assert_eq!(
            solana_pubkey::Pubkey::new_from_array(entry.verifier).to_string(),
            "H3ZFoDHFvthGZu3kxpif3oSWm8MQn8uKvgDhrvVVHvHf"
        );
    }

    /// The schema-4 `PAStateAccount` layout (state.rs): schema_version, bump,
    /// owner, verifier_router, proof_selector, kind_table_commitment, paused, root,
    /// next_index, current_depth, frontier, min/max expiry, then the denylists
    /// for consumed and for created resources.
    fn build_fixture(
        schema_version: u8,
        paused: u8,
        denied_consumed: &[[u8; 32]],
        denied_created: &[[u8; 32]],
    ) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&anchor_account_disc("PAStateAccount"));
        data.push(schema_version);
        data.push(255); // bump
        data.extend_from_slice(&[1u8; 32]); // owner
        data.extend_from_slice(&[2u8; 32]); // verifier_router
        data.extend_from_slice(&[0xAB, 0xCD, 0xEF, 0x12]); // proof_selector
        data.extend_from_slice(&[8u8; 32]); // kind_table_commitment
        data.push(paused);
        data.extend_from_slice(&[4u8; 32]); // root
        data.extend_from_slice(&100u64.to_le_bytes()); // next_index
        data.push(3); // current_depth
        data.extend_from_slice(&3u32.to_le_bytes()); // frontier len
        data.extend_from_slice(&[5u8; 32]);
        data.extend_from_slice(&[6u8; 32]);
        data.extend_from_slice(&[7u8; 32]);
        data.extend_from_slice(&100u64.to_le_bytes()); // min_expiry_slots
        data.extend_from_slice(&216_000u64.to_le_bytes()); // max_expiry_slots
        for denylist in [denied_consumed, denied_created] {
            data.extend_from_slice(&(denylist.len() as u32).to_le_bytes());
            for r in denylist {
                data.extend_from_slice(r);
            }
        }
        data
    }

    #[test]
    fn decodes_every_field() {
        let data = build_fixture(PA_STATE_SCHEMA_VERSION, 1, &[[0xDD; 32]], &[[0xEE; 32]]);
        let s = decode_pa_state(&data).expect("decode");
        assert_eq!(s.schema_version, PA_STATE_SCHEMA_VERSION);
        assert_eq!(s.bump, 255);
        assert_eq!(s.owner, [1u8; 32]);
        assert_eq!(s.verifier_router, [2u8; 32]);
        assert_eq!(s.proof_selector, [0xAB, 0xCD, 0xEF, 0x12]);
        assert_eq!(s.kind_table_commitment, [8u8; 32]);
        assert!(s.paused);
        assert_eq!(s.root, [4u8; 32]);
        assert_eq!(s.next_index, 100);
        assert_eq!(s.current_depth, 3);
        assert_eq!(s.frontier, vec![[5u8; 32], [6u8; 32], [7u8; 32]]);
        assert_eq!(s.min_expiry_slots, 100);
        assert_eq!(s.max_expiry_slots, 216_000);
        assert_eq!(s.denied_consumed_logic_refs, vec![[0xDD; 32]]);
        assert_eq!(s.denied_created_logic_refs, vec![[0xEE; 32]]);
    }

    #[test]
    fn encodes_what_it_decodes() {
        let data = build_fixture(
            PA_STATE_SCHEMA_VERSION,
            1,
            &[[0xDD; 32], [0xEE; 32]],
            &[[0xEE; 32]],
        );
        assert_eq!(
            encode_pa_state(&decode_pa_state(&data).expect("decode")),
            data
        );
    }

    #[test]
    fn decodes_an_unpaused_state_with_no_denied_refs() {
        let s =
            decode_pa_state(&build_fixture(PA_STATE_SCHEMA_VERSION, 0, &[], &[])).expect("decode");
        assert!(!s.paused);
        assert!(s.denied_consumed_logic_refs.is_empty());
        assert!(s.denied_created_logic_refs.is_empty());
    }

    #[test]
    fn rejects_an_invalid_paused_byte() {
        let err = decode_pa_state(&build_fixture(PA_STATE_SCHEMA_VERSION, 2, &[], &[]))
            .expect_err("must reject");
        assert_eq!(
            err,
            DecodeError::InvalidBool {
                field: "paused",
                byte: 2
            }
        );
    }

    #[test]
    fn rejects_unsupported_schema_version() {
        // The PA refuses every instruction on an account whose layout number
        // is not its own; a client reading another layout would misparse
        // every field after byte 8, so it must refuse too.
        let data = build_fixture(3, 0, &[], &[]);
        let err = decode_pa_state(&data).expect_err("must reject");
        assert_eq!(err, DecodeError::UnsupportedSchemaVersion { found: 3 });
    }

    #[test]
    fn rejects_truncated_data() {
        let data = build_fixture(PA_STATE_SCHEMA_VERSION, 0, &[], &[[0xDD; 32]]);
        let err = decode_pa_state(&data[..data.len() - 1]).expect_err("must reject");
        assert!(matches!(
            err,
            DecodeError::Truncated {
                field: "denied_created_logic_refs"
            }
        ));
    }
}
