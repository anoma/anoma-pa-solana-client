//! Reading the adapter's state account, and the verifier router's entry
//! account, which the adapter's IDL does not declare.

use anchor::prelude::Pubkey;
use anchor::{AccountDeserialize, AnchorDeserialize, Discriminator};

use crate::generated::protocol_adapter::accounts::PAStateAccount;
use crate::generated::protocol_adapter::constants::SCHEMA_VERSION;
use crate::merkle::CommitmentTreeState;

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

/// Why account data is not the adapter's state.
#[derive(Debug)]
pub enum StateDecodeError {
    /// The state's layout number, at byte 8 in every layout, is not
    /// `SCHEMA_VERSION`, the layout the IDL describes. The adapter refuses
    /// every instruction on such an account: the deployment is mid-migration.
    UnsupportedSchemaVersion { found: u8 },
    /// The data is not a `PAStateAccount` as the IDL describes it.
    Account(anchor::error::Error),
}

impl core::fmt::Display for StateDecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            StateDecodeError::UnsupportedSchemaVersion { found } => write!(
                f,
                "the adapter state's schema version is {found}; this client reads {SCHEMA_VERSION}"
            ),
            StateDecodeError::Account(e) => write!(f, "the adapter state does not decode: {e}"),
        }
    }
}

impl std::error::Error for StateDecodeError {}

/// The adapter's state from its account's data, as `getAccountInfo` returns
/// it (the account discriminator first). The account may be larger than the
/// encoding; the bytes after it are ignored, as the adapter ignores them.
pub fn decode_pa_state(data: &[u8]) -> Result<PAStateAccount, StateDecodeError> {
    if let Some(&found) = data
        .strip_prefix(PAStateAccount::DISCRIMINATOR)
        .and_then(<[u8]>::first)
    {
        if found != SCHEMA_VERSION {
            return Err(StateDecodeError::UnsupportedSchemaVersion { found });
        }
    }
    PAStateAccount::try_deserialize(&mut &*data).map_err(StateDecodeError::Account)
}

/// A verifier router's `VerifierEntry`: the verifier program registered under
/// a selector, and whether the router has paused it, as the router's IDL lays
/// it out after the account discriminator.
#[derive(Clone, Debug, PartialEq, Eq, AnchorDeserialize)]
pub struct VerifierEntryAccount {
    pub selector: [u8; 4],
    pub verifier: Pubkey,
    pub paused: bool,
}

/// Decode a router `VerifierEntry` account's data, its 8-byte discriminator
/// first.
pub fn decode_verifier_entry(data: &[u8]) -> Result<VerifierEntryAccount, std::io::Error> {
    let mut body = data.get(8..).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "a verifier entry ends inside its discriminator",
        )
    })?;
    VerifierEntryAccount::deserialize(&mut body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anchor::AccountSerialize;

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
        println!("{entry:?}");
        assert_eq!(entry.selector, crate::MOCK_SELECTOR);
        assert_eq!(
            entry.verifier.to_string(),
            "H3ZFoDHFvthGZu3kxpif3oSWm8MQn8uKvgDhrvVVHvHf"
        );
        assert!(!entry.paused);
        let mut paused = data.clone();
        paused[44] = 1;
        assert!(decode_verifier_entry(&paused).unwrap().paused);
        paused[44] = 2;
        let invalid = decode_verifier_entry(&paused);
        println!("paused byte 2: {invalid:?}");
        assert!(invalid.is_err(), "a bool byte of 2 decodes: {invalid:?}");
        let short = decode_verifier_entry(&data[..20]);
        println!("20 bytes: {short:?}");
        assert!(short.is_err(), "a 20-byte entry decodes: {short:?}");
        assert!(decode_verifier_entry(&data[..4]).is_err());
    }

    /// `encode_pa_state` of the hand-written client before the generated
    /// bindings replaced it, for `state()`: the account discriminator, then
    /// the schema-4 fields in the IDL's order.
    const HAND_WRITTEN_STATE: &str = "c905c1bc09bb833504ff01010101010101010101010101010101010101010101010101010101010101010202020202020202020202020202020202020202020202020202020202020202abcdef120808080808080808080808080808080808080808080808080808080808080808010404040404040404040404040404040404040404040404040404040404040404640000000000000003030000000505050505050505050505050505050505050505050505050505050505050505060606060606060606060606060606060606060606060606060606060606060607070707070707070707070707070707070707070707070707070707070707076400000000000000c04b03000000000001000000dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd02000000eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeefefefefefefefefefefefefefefefefefefefefefefefefefefefefefefefef";

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    fn state() -> PAStateAccount {
        PAStateAccount {
            schema_version: SCHEMA_VERSION,
            bump: 255,
            owner: Pubkey::new_from_array([1; 32]),
            verifier_router: Pubkey::new_from_array([2; 32]),
            proof_selector: [0xAB, 0xCD, 0xEF, 0x12],
            kind_tree_root: [8; 32],
            paused: true,
            root: [4; 32],
            next_index: 100,
            current_depth: 3,
            frontier: vec![[5; 32], [6; 32], [7; 32]],
            min_expiry_slots: 100,
            max_expiry_slots: 216_000,
            denied_consumed_logic_refs: vec![[0xDD; 32]],
            denied_created_logic_refs: vec![[0xEE; 32], [0xEF; 32]],
        }
    }

    /// The generated `PAStateAccount` is the layout the hand-written client
    /// read and wrote: the same bytes both ways.
    #[test]
    fn the_state_fields_are_the_idls() {
        let mut encoded = Vec::new();
        state().try_serialize(&mut encoded).unwrap();
        assert_eq!(encoded, hex(HAND_WRITTEN_STATE));
        let decoded = decode_pa_state(&hex(HAND_WRITTEN_STATE)).unwrap();
        println!("{decoded:#?}");
        assert_eq!(format!("{decoded:?}"), format!("{:?}", state()));
        let tree = CommitmentTreeState::from(&decoded);
        assert_eq!(tree.root, [4; 32]);
        assert_eq!(tree.next_index, 100);
        assert_eq!(tree.current_depth, 3);
        assert_eq!(tree.frontier, vec![[5; 32], [6; 32], [7; 32]]);
    }

    #[test]
    fn bytes_past_the_encoding_are_ignored() {
        let mut data = hex(HAND_WRITTEN_STATE);
        data.extend([0; 64]);
        let decoded = decode_pa_state(&data).unwrap();
        assert_eq!(format!("{decoded:?}"), format!("{:?}", state()));
    }

    #[test]
    fn another_schema_version_is_refused() {
        // The adapter refuses every instruction on an account whose layout
        // number is not its own; a client reading another layout would
        // misparse every field after byte 8, so it refuses too.
        let mut data = hex(HAND_WRITTEN_STATE);
        data[8] = 3;
        let result = decode_pa_state(&data);
        assert!(
            matches!(
                result,
                Err(StateDecodeError::UnsupportedSchemaVersion { found: 3 })
            ),
            "{result:?}"
        );
    }

    #[test]
    fn data_that_is_not_the_state_is_refused() {
        let data = hex(HAND_WRITTEN_STATE);
        let mut other = data.clone();
        other[0] ^= 1;
        for (what, data) in [
            ("another account's discriminator", &other[..]),
            ("a truncated state", &data[..data.len() - 1]),
            ("a discriminator alone", &data[..8]),
        ] {
            let result = decode_pa_state(data);
            println!("{what}: {result:?}");
            assert!(
                matches!(result, Err(StateDecodeError::Account(_))),
                "{what}: {result:?}"
            );
        }
    }
}
