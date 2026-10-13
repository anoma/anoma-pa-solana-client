//! The transactions of one settlement: one v1 transaction carrying the
//! settlement input when it fits, otherwise the input uploaded in chunks and
//! settled from the upload.

use std::collections::BTreeSet;

use solana_instruction::{AccountMeta, Instruction};
use solana_message::v1::{self, TransactionConfig};
use solana_message::{CompileError, Hash};
use solana_pubkey::Pubkey;

use crate::accounts::PAStateAccount;
use crate::constants::{
    MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES, SETTLE_COMPUTE_UNIT_LIMIT, SETTLE_HEAP_FRAME_BYTES,
    TXDATA_CHUNK_SIZE, UPLOAD_COMPUTE_UNIT_LIMIT,
};
use crate::instructions::{
    settle_from_txdata_ix, settle_ix, txdata_close_ix, txdata_init_ix, txdata_write_ix,
};
use crate::merkle::{CommitmentTreeState, MerkleError, PADDING_LEAF};
use crate::pda::{
    derive_nullifier_pda, derive_pa_state_pda, derive_root_marker_pda, derive_tx_data_pda,
    derive_verifier_router_pdas,
};

/// The v1 transaction settings of a settlement transaction: its compute and
/// heap budgets, and the runtime's maximum of loaded account data, which a v1
/// transaction states rather than defaults to.
pub const SETTLE_TRANSACTION_CONFIG: TransactionConfig = TransactionConfig {
    priority_fee: None,
    compute_unit_limit: Some(SETTLE_COMPUTE_UNIT_LIMIT),
    loaded_accounts_data_size_limit: Some(MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES),
    heap_size: Some(SETTLE_HEAP_FRAME_BYTES),
};

/// The v1 transaction settings of an upload's init, write and close
/// transactions, each one adapter instruction.
pub const UPLOAD_TRANSACTION_CONFIG: TransactionConfig = TransactionConfig {
    priority_fee: None,
    compute_unit_limit: Some(UPLOAD_COMPUTE_UNIT_LIMIT),
    loaded_accounts_data_size_limit: Some(MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES),
    heap_size: None,
};

/// The v1 message of `instructions`, paid for and signed by `payer`, with
/// the settings `config` (`SETTLE_TRANSACTION_CONFIG` or
/// `UPLOAD_TRANSACTION_CONFIG`).
pub fn transaction_message(
    payer: &Pubkey,
    instructions: &[Instruction],
    blockhash: Hash,
    config: TransactionConfig,
) -> Result<v1::Message, CompileError> {
    v1::Message::try_compile_with_config(payer, instructions, blockhash, config)
}

/// The bytes the v1 transaction of `message` takes on the wire: the message,
/// then one 64-byte signature per signer. A transaction fits when this is at
/// most `v1::MAX_TRANSACTION_SIZE`.
pub fn transaction_size(message: &v1::Message) -> usize {
    message.serialize().len() + 64 * usize::from(message.header.num_required_signatures)
}

/// What one settlement needs.
pub struct SettlementRequest<'a> {
    pub pa_program: Pubkey,
    /// Pays for the settlement (and owns the upload, when there is one), and
    /// signs every transaction.
    pub payer: Pubkey,
    /// Tells the payer's concurrent uploads apart, when the input is uploaded.
    pub upload_id: u64,
    /// The slot after which an upload may be reclaimed.
    pub expires_slot: u64,
    /// The settlement input (`settlement_input`, or a fixture's `tx_b64`).
    pub input: &'a [u8],
    /// The adapter's state before the settlement.
    pub state: &'a PAStateAccount,
    /// The verifier program the router's entry for the adapter's selector names.
    pub verifier_program: Pubkey,
    /// The consumed resources' nullifiers, in the aggregation instance's order.
    pub nullifiers: &'a [[u8; 32]],
    /// The commitment-tree roots the consumed resources' proofs were made
    /// against, in any order.
    pub consumed_roots: &'a [[u8; 32]],
    /// The commitments the settlement creates, in order.
    pub created: &'a [[u8; 32]],
    /// Each external call's account segment, in the order the calls run.
    pub call_segments: Vec<Vec<AccountMeta>>,
    /// The instructions the settlement transaction carries before the
    /// adapter's, such as a forwarder's signature checks and the accounts its
    /// calls need created, in the order their calls name them.
    pub preceding: Vec<Instruction>,
}

/// How one settlement's input reaches the adapter.
#[derive(Debug, PartialEq)]
pub enum Settlement {
    /// One transaction: the preceding instructions, then `settle` carrying
    /// the input.
    Inline { transaction: Vec<Instruction> },
    /// The input uploaded in chunks, one transaction each, then settled from
    /// the upload.
    Upload {
        /// Creates the upload.
        init: Instruction,
        /// Fills the upload, one chunk per transaction.
        writes: Vec<Instruction>,
        /// The settlement transaction: the preceding instructions, then
        /// `settle_from_txdata`.
        settle: Vec<Instruction>,
        /// Closes the upload, whatever the settlement's outcome.
        close: Instruction,
    },
}

/// One settlement's transactions.
#[derive(Debug)]
pub struct SettlementPlan {
    pub settlement: Settlement,
    /// The root the adapter holds after the settlement; `None` when it creates
    /// nothing and so produces no root.
    pub new_root: Option<[u8; 32]>,
}

/// Why a settlement cannot be planned.
#[derive(Debug)]
pub enum PlanError {
    /// The commitments it creates overflow the commitment tree.
    Merkle(MerkleError),
    /// Its transaction cannot be compiled.
    Compile(CompileError),
    /// Its transaction names more accounts than a transaction can lock.
    TooManyAccounts { accounts: usize },
}

impl core::fmt::Display for PlanError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            PlanError::Merkle(e) => write!(f, "the settlement's commitments: {e}"),
            PlanError::Compile(e) => write!(f, "the settlement transaction: {e}"),
            PlanError::TooManyAccounts { accounts } => write!(
                f,
                "the settlement transaction names {accounts} accounts; a transaction locks at most {}",
                v1::MAX_ADDRESSES
            ),
        }
    }
}

impl core::error::Error for PlanError {}

impl From<MerkleError> for PlanError {
    fn from(e: MerkleError) -> Self {
        PlanError::Merkle(e)
    }
}

impl From<CompileError> for PlanError {
    fn from(e: CompileError) -> Self {
        PlanError::Compile(e)
    }
}

/// The transactions of one settlement: one v1 transaction with the input
/// inline when that transaction fits `v1::MAX_TRANSACTION_SIZE`, otherwise
/// the upload's transactions. Either way the settlement transaction takes
/// `SETTLE_TRANSACTION_CONFIG`, and an upload's others
/// `UPLOAD_TRANSACTION_CONFIG`.
///
/// The settlement's remaining accounts are the nullifier PDAs, then the call
/// segments, then the root marker of every consumed root other than the
/// padding leaf. The adapter accepts its current root without a marker; the
/// marker is passed anyway, so the settlement still lands when another one
/// moves the root first.
pub fn plan_settlement(request: SettlementRequest<'_>) -> Result<SettlementPlan, PlanError> {
    let SettlementRequest {
        pa_program,
        payer,
        upload_id,
        expires_slot,
        input,
        state,
        verifier_program,
        nullifiers,
        consumed_roots,
        created,
        call_segments,
        preceding,
    } = request;
    let (pa_state, _) = derive_pa_state_pda(&pa_program);

    let new_root = if created.is_empty() {
        None
    } else {
        let mut tree = CommitmentTreeState::from(state);
        for leaf in created {
            tree.append(*leaf)?;
        }
        Some(tree.root)
    };
    let new_root_marker =
        new_root.map(|root| derive_root_marker_pda(&pa_program, &pa_state, &root).0);

    let mut remaining: Vec<AccountMeta> = nullifiers
        .iter()
        .map(|nf| AccountMeta::new(derive_nullifier_pda(&pa_program, &pa_state, nf).0, false))
        .collect();
    remaining.extend(call_segments.into_iter().flatten());
    let roots: BTreeSet<[u8; 32]> = consumed_roots
        .iter()
        .copied()
        .filter(|root| *root != PADDING_LEAF)
        .collect();
    remaining.extend(roots.iter().map(|root| {
        AccountMeta::new_readonly(
            derive_root_marker_pda(&pa_program, &pa_state, root).0,
            false,
        )
    }));

    let verifier_router = Pubkey::new_from_array(state.verifier_router);
    let (router, verifier_entry) =
        derive_verifier_router_pdas(&verifier_router, state.proof_selector);

    let mut transaction = preceding.clone();
    transaction.push(settle_ix(
        &pa_program,
        &pa_state,
        &payer,
        input,
        new_root_marker.as_ref(),
        &verifier_router,
        &router,
        &verifier_entry,
        &verifier_program,
        remaining.clone(),
    ));
    // The blockhash is a fixed 32 bytes, so any one gives the size.
    let message = transaction_message(
        &payer,
        &transaction,
        Hash::default(),
        SETTLE_TRANSACTION_CONFIG,
    )?;
    // The upload's settlement transaction names the same accounts and the
    // upload, so it cannot lock them either.
    if message.account_keys.len() > usize::from(v1::MAX_ADDRESSES) {
        return Err(PlanError::TooManyAccounts {
            accounts: message.account_keys.len(),
        });
    }
    if transaction_size(&message) <= v1::MAX_TRANSACTION_SIZE {
        return Ok(SettlementPlan {
            settlement: Settlement::Inline { transaction },
            new_root,
        });
    }

    let (tx_data, _) = derive_tx_data_pda(&pa_program, &payer, upload_id);
    let mut settle = preceding;
    settle.push(settle_from_txdata_ix(
        &pa_program,
        &pa_state,
        &tx_data,
        &payer,
        upload_id,
        new_root_marker.as_ref(),
        &verifier_router,
        &router,
        &verifier_entry,
        &verifier_program,
        remaining,
    ));
    Ok(SettlementPlan {
        settlement: Settlement::Upload {
            init: txdata_init_ix(
                &pa_program,
                &pa_state,
                &tx_data,
                &payer,
                upload_id,
                input.len() as u32,
                expires_slot,
            ),
            writes: input
                .chunks(TXDATA_CHUNK_SIZE)
                .enumerate()
                .map(|(i, chunk)| {
                    txdata_write_ix(
                        &pa_program,
                        &tx_data,
                        &payer,
                        upload_id,
                        (i * TXDATA_CHUNK_SIZE) as u32,
                        chunk,
                    )
                })
                .collect(),
            settle,
            close: txdata_close_ix(&pa_program, &tx_data, &payer, &payer, upload_id),
        },
        new_root,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::MOCK_SELECTOR;

    fn key(n: u8) -> Pubkey {
        Pubkey::new_from_array([n; 32])
    }

    fn state_with(leaves: &[[u8; 32]]) -> PAStateAccount {
        let tree = CommitmentTreeState::over(leaves).unwrap();
        PAStateAccount {
            schema_version: 3,
            bump: 255,
            owner: [1; 32],
            verifier_router: [2; 32],
            proof_selector: MOCK_SELECTOR,
            kind_table_commitment: [0; 32],
            paused: false,
            root: tree.root,
            next_index: tree.next_index,
            current_depth: tree.current_depth,
            frontier: tree.frontier,
            min_expiry_slots: 10,
            max_expiry_slots: 1000,
            denied_consumed_logic_refs: vec![],
            denied_created_logic_refs: vec![],
        }
    }

    fn request<'a>(
        state: &'a PAStateAccount,
        input: &'a [u8],
        nullifiers: &'a [[u8; 32]],
        consumed_roots: &'a [[u8; 32]],
        created: &'a [[u8; 32]],
        call_segments: Vec<Vec<AccountMeta>>,
    ) -> SettlementRequest<'a> {
        SettlementRequest {
            pa_program: key(9),
            payer: key(8),
            upload_id: 7,
            expires_slot: 500,
            input,
            state,
            verifier_program: key(6),
            nullifiers,
            consumed_roots,
            created,
            call_segments,
            preceding: vec![],
        }
    }

    /// The settlement instruction of `plan`, inline or from the upload, and
    /// how many named accounts precede its remaining ones.
    fn settle_instruction(plan: &SettlementPlan) -> (&Instruction, usize) {
        match &plan.settlement {
            // `settle`: no upload account.
            Settlement::Inline { transaction } => (transaction.last().unwrap(), 10),
            Settlement::Upload { settle, .. } => (settle.last().unwrap(), 11),
        }
    }

    /// The size of the inline transaction carrying an input of `len` bytes,
    /// with nothing else in it.
    fn inline_size(state: &PAStateAccount, len: usize) -> usize {
        let input = vec![0u8; len];
        let plan = plan_settlement(request(state, &input, &[], &[], &[], vec![])).unwrap();
        let Settlement::Inline { transaction } = plan.settlement else {
            panic!("an input of {len} bytes uploads");
        };
        transaction_size(
            &transaction_message(
                &key(8),
                &transaction,
                Hash::default(),
                SETTLE_TRANSACTION_CONFIG,
            )
            .unwrap(),
        )
    }

    #[test]
    fn the_remaining_accounts_are_nullifiers_then_call_segments_then_root_markers() {
        let leaves = [[3; 32], [4; 32]];
        let earlier_root = CommitmentTreeState::over(&leaves[..1]).unwrap().root;
        let state = state_with(&leaves);
        let nullifiers = [[10; 32], [11; 32]];
        // The current root, a historical one twice, and the padding leaf.
        let roots = [state.root, earlier_root, PADDING_LEAF, earlier_root];
        let segment = vec![
            AccountMeta::new_readonly(key(20), false),
            AccountMeta::new(key(21), false),
        ];
        let (pa_state, _) = derive_pa_state_pda(&key(9));
        let mut expected: Vec<AccountMeta> = nullifiers
            .iter()
            .map(|nf| AccountMeta::new(derive_nullifier_pda(&key(9), &pa_state, nf).0, false))
            .collect();
        expected.extend(segment.clone());
        // One marker per distinct root other than the padding leaf, the current root included.
        let mut markers: Vec<[u8; 32]> = vec![state.root, earlier_root];
        markers.sort();
        expected.extend(markers.iter().map(|root| {
            AccountMeta::new_readonly(derive_root_marker_pda(&key(9), &pa_state, root).0, false)
        }));

        // The same remaining accounts whether the input is inline or uploaded.
        for input in [vec![1u8; 5], vec![1u8; v1::MAX_TRANSACTION_SIZE]] {
            let plan = plan_settlement(request(
                &state,
                &input,
                &nullifiers,
                &roots,
                &[],
                vec![segment.clone()],
            ))
            .unwrap();
            let (settle, named) = settle_instruction(&plan);
            assert_eq!(
                settle.accounts[named..],
                expected[..],
                "input of {} bytes",
                input.len()
            );
        }
    }

    #[test]
    fn a_settlement_that_creates_commitments_names_the_marker_of_the_root_it_produces() {
        let state = state_with(&[[3; 32]]);
        let created = [[5; 32], [6; 32]];
        let plan = plan_settlement(request(&state, b"input", &[], &[], &created, vec![])).unwrap();
        let expected_root = CommitmentTreeState::over(&[[3; 32], [5; 32], [6; 32]])
            .unwrap()
            .root;
        assert_eq!(plan.new_root, Some(expected_root));
        let (pa_state, _) = derive_pa_state_pda(&key(9));
        let marker = derive_root_marker_pda(&key(9), &pa_state, &expected_root).0;
        // new_root_marker follows pa_state, the payer and the system program.
        let (settle, _) = settle_instruction(&plan);
        assert_eq!(settle.accounts[3], AccountMeta::new(marker, false));

        let none = plan_settlement(request(&state, b"input", &[], &[], &[], vec![])).unwrap();
        assert_eq!(none.new_root, None);
        // Anchor marks an absent optional account by the program's own address.
        let (settle, _) = settle_instruction(&none);
        assert_eq!(settle.accounts[3], AccountMeta::new_readonly(key(9), false));
    }

    #[test]
    fn a_settlement_that_fits_one_transaction_carries_its_input_inline() {
        let state = state_with(&[]);
        let preceding = Instruction::new_with_bytes(key(30), b"check", vec![]);
        let mut req = request(&state, b"input", &[], &[], &[], vec![]);
        req.preceding = vec![preceding.clone()];
        let plan = plan_settlement(req).unwrap();
        let Settlement::Inline { transaction } = plan.settlement else {
            panic!("a 5-byte input uploads");
        };
        // The preceding instructions first, then `settle` with the input.
        assert_eq!(transaction.len(), 2);
        assert_eq!(transaction[0], preceding);
        let settle = &transaction[1];
        assert_eq!(
            settle.data[..8],
            crate::discriminator::anchor_instruction_disc("settle")
        );
        assert_eq!(settle.data[8..12], 5u32.to_le_bytes());
        assert_eq!(&settle.data[12..], b"input");
        let router_program = Pubkey::new_from_array(state.verifier_router);
        let (router, entry) = derive_verifier_router_pdas(&router_program, MOCK_SELECTOR);
        for k in [router_program, router, entry, key(6)] {
            assert!(
                settle.accounts.iter().any(|a| a.pubkey == k),
                "settle names {k}"
            );
        }
    }

    #[test]
    fn the_largest_inline_transaction_is_the_v1_limit_and_one_byte_more_uploads() {
        let state = state_with(&[]);
        // Each input byte adds one byte to the transaction.
        let largest = v1::MAX_TRANSACTION_SIZE - inline_size(&state, 0);
        assert_eq!(inline_size(&state, largest), v1::MAX_TRANSACTION_SIZE);
        let input = vec![0u8; largest + 1];
        let plan = plan_settlement(request(&state, &input, &[], &[], &[], vec![])).unwrap();
        assert!(
            matches!(plan.settlement, Settlement::Upload { .. }),
            "an input one byte over the inline limit is uploaded"
        );
    }

    #[test]
    fn a_settlement_too_large_for_one_transaction_is_uploaded_in_chunks_at_their_offsets() {
        let state = state_with(&[]);
        let input: Vec<u8> = (0..(2 * TXDATA_CHUNK_SIZE + 5)).map(|i| i as u8).collect();
        let preceding = Instruction::new_with_bytes(key(30), b"check", vec![]);
        let mut req = request(&state, &input, &[], &[], &[], vec![]);
        req.preceding = vec![preceding.clone()];
        let plan = plan_settlement(req).unwrap();
        let Settlement::Upload {
            init,
            writes,
            settle,
            close,
        } = plan.settlement
        else {
            panic!("an input of two chunks settles inline");
        };
        let (tx_data, _) = derive_tx_data_pda(&key(9), &key(8), 7);
        let (pa_state, _) = derive_pa_state_pda(&key(9));
        assert_eq!(
            init,
            txdata_init_ix(
                &key(9),
                &pa_state,
                &tx_data,
                &key(8),
                7,
                input.len() as u32,
                500
            )
        );
        // Two full chunks, then the 5-byte rest.
        let c = TXDATA_CHUNK_SIZE;
        let expected: Vec<Instruction> = [(0, c), (c, 2 * c), (2 * c, 2 * c + 5)]
            .iter()
            .map(|&(start, end)| {
                txdata_write_ix(
                    &key(9),
                    &tx_data,
                    &key(8),
                    7,
                    start as u32,
                    &input[start..end],
                )
            })
            .collect();
        assert_eq!(writes, expected);
        // The preceding instructions first, then `settle_from_txdata` of the upload.
        assert_eq!(settle.len(), 2);
        assert_eq!(settle[0], preceding);
        assert_eq!(
            settle[1].data[..8],
            crate::discriminator::anchor_instruction_disc("settle_from_txdata")
        );
        assert_eq!(
            settle[1].accounts[1],
            AccountMeta::new_readonly(tx_data, false)
        );
        assert_eq!(
            close,
            txdata_close_ix(&key(9), &tx_data, &key(8), &key(8), 7)
        );
    }

    #[test]
    fn a_settlement_naming_more_accounts_than_a_transaction_locks_is_refused() {
        let state = state_with(&[]);
        let nullifiers: Vec<[u8; 32]> = (0..usize::from(v1::MAX_ADDRESSES))
            .map(|i| {
                let mut nf = [0u8; 32];
                nf[..8].copy_from_slice(&(i as u64).to_le_bytes());
                nf
            })
            .collect();
        let result = plan_settlement(request(&state, b"input", &nullifiers, &[], &[], vec![]));
        assert!(
            matches!(result, Err(PlanError::TooManyAccounts { accounts }) if accounts > usize::from(v1::MAX_ADDRESSES)),
            "{result:?}"
        );
    }

    #[test]
    fn a_full_chunk_is_the_largest_whose_write_transaction_fits() {
        let size = |len: usize| {
            let write = txdata_write_ix(&key(9), &key(7), &key(8), 7, 0, &vec![0u8; len]);
            transaction_size(
                &transaction_message(
                    &key(8),
                    &[write],
                    Hash::default(),
                    UPLOAD_TRANSACTION_CONFIG,
                )
                .unwrap(),
            )
        };
        assert!(size(TXDATA_CHUNK_SIZE) <= v1::MAX_TRANSACTION_SIZE);
        assert!(size(TXDATA_CHUNK_SIZE + 1) > v1::MAX_TRANSACTION_SIZE);
    }
}
