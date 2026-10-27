//! The transactions of one settlement: one v1 transaction carrying the
//! settlement input when it fits, otherwise the input uploaded in chunks and
//! settled from the upload.

use std::collections::BTreeSet;

use solana_instruction::{AccountMeta, Instruction};
use solana_message::v1::{self, TransactionConfig};
use solana_message::{CompileError, Hash};
use solana_pubkey::Pubkey;

use crate::constants::{
    MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES, SETTLE_COMPUTE_UNIT_LIMIT, SETTLE_HEAP_FRAME_BYTES,
    TXDATA_CHUNK_SIZE, UPLOAD_COMPUTE_UNIT_LIMIT,
};
use crate::generated::protocol_adapter::accounts::PAStateAccount;
use crate::instructions::{
    settle_from_txdata_ix, settle_ix, simulate_execute_ix, txdata_close_ix, txdata_init_ix,
    txdata_write_ix, SettlementAccounts,
};
use crate::merkle::{CommitmentTreeState, MerkleError, PADDING_LEAF};
use crate::pda::{
    derive_nullifier_pda, derive_pa_state_pda, derive_root_marker_pda, derive_tx_data_pda,
    derive_verifier_router_pdas,
};

/// The v1 transaction settings of a settlement transaction: its compute and
/// heap budgets, and the runtime's maximum of loaded account data, which a v1
/// transaction states rather than defaults to.
const SETTLE_TRANSACTION_CONFIG: TransactionConfig = TransactionConfig {
    priority_fee: None,
    compute_unit_limit: Some(SETTLE_COMPUTE_UNIT_LIMIT),
    loaded_accounts_data_size_limit: Some(MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES),
    heap_size: Some(SETTLE_HEAP_FRAME_BYTES),
};

/// The v1 transaction settings of an upload's init, write and close
/// transactions, each one adapter instruction.
const UPLOAD_TRANSACTION_CONFIG: TransactionConfig = TransactionConfig {
    priority_fee: None,
    compute_unit_limit: Some(UPLOAD_COMPUTE_UNIT_LIMIT),
    loaded_accounts_data_size_limit: Some(MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES),
    heap_size: None,
};

/// One transaction of a settlement: its instructions and the v1 settings it
/// is sent with.
#[derive(Debug, PartialEq)]
pub struct PlannedTransaction {
    pub instructions: Vec<Instruction>,
    pub config: TransactionConfig,
}

impl PlannedTransaction {
    /// Its v1 message, paid for and signed by `payer`.
    pub fn message(&self, payer: &Pubkey, blockhash: Hash) -> Result<v1::Message, CompileError> {
        v1::Message::try_compile_with_config(payer, &self.instructions, blockhash, self.config)
    }
}

/// The bytes the v1 transaction of `message` takes on the wire: the version
/// byte, the message, then one 64-byte signature per signer. A transaction
/// fits when this is at most `v1::MAX_TRANSACTION_SIZE`.
pub fn transaction_size(message: &v1::Message) -> usize {
    1 + message.size() + 64 * usize::from(message.header.num_required_signatures)
}

/// The index of the first preceding instruction (`CallAccounts::preceding`)
/// in a settlement transaction: the planners place them first.
pub const PRECEDING_START: usize = 0;

/// What a settlement passes the forwarders for their calls.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallAccounts {
    /// Each call's CPI segment, in call order: the forwarder program, then
    /// the accounts its instruction takes.
    pub segments: Vec<Vec<AccountMeta>>,
    /// Instructions the settlement transaction carries first, before the
    /// adapter's (an ed25519 signature check, an account a call needs), in the
    /// order the calls' instruction data names them.
    pub preceding: Vec<Instruction>,
}

/// What one settlement needs.
pub struct SettlementRequest<'a> {
    pub pa_program: Pubkey,
    /// Pays for the settlement (and owns the upload, when there is one), and
    /// signs every transaction.
    pub payer: Pubkey,
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
    /// The external calls' account segments and the instructions that
    /// precede the adapter's.
    pub accounts: CallAccounts,
}

/// The upload a settlement too large for one transaction reads its input
/// from.
#[derive(Debug, PartialEq)]
pub struct Upload {
    /// Creates the upload.
    pub init: PlannedTransaction,
    /// Fill the upload, one chunk each. Each write names its offset, so they
    /// land in any order.
    pub writes: Vec<PlannedTransaction>,
    /// Closes the upload, whatever the settlement's outcome.
    pub close: PlannedTransaction,
}

/// One settlement's transactions.
#[derive(Debug)]
pub struct SettlementPlan {
    /// The upload to fill before `settle` and close after it; `None` when
    /// `settle` carries the input itself.
    pub upload: Option<Upload>,
    /// The settlement transaction: the preceding instructions, then the
    /// adapter's `settle` with the input, or `settle_from_txdata` of the
    /// upload.
    pub settle: PlannedTransaction,
    /// The root the adapter holds after the settlement; `None` when it creates
    /// nothing and so produces no root.
    pub new_root: Option<[u8; 32]>,
}

impl SettlementPlan {
    /// How many transactions it sends, each signed by the payer alone: the
    /// settlement, and the upload's init, writes and close when it has one.
    pub fn transaction_count(&self) -> usize {
        1 + self
            .upload
            .as_ref()
            .map_or(0, |upload| 2 + upload.writes.len())
    }
}

/// Why a settlement cannot be planned.
#[derive(Debug)]
pub enum PlanError {
    /// The commitments it creates overflow the commitment tree.
    Merkle(MerkleError),
    /// Its transaction cannot be compiled.
    Compile(CompileError),
    /// Its settlement transaction names more accounts than a transaction can
    /// lock.
    TooManyAccounts { accounts: usize },
    /// Its settlement transaction is larger than a transaction, even with the
    /// input uploaded.
    TooLarge { bytes: usize },
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
            PlanError::TooLarge { bytes } => write!(
                f,
                "the settlement transaction takes {bytes} bytes with its input uploaded; a transaction takes at most {}",
                v1::MAX_TRANSACTION_SIZE
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

/// What every transaction settling one request carries: the adapter's
/// accounts and remaining accounts, the input, and the instructions before
/// the adapter's.
struct Settling<'a> {
    pa_program: Pubkey,
    payer: Pubkey,
    accounts: SettlementAccounts,
    /// The root the settlement produces; `None` when it creates nothing.
    new_root: Option<[u8; 32]>,
    input: &'a [u8],
    preceding: Vec<Instruction>,
}

/// What settling `request` carries.
///
/// The settlement's remaining accounts are the nullifier PDAs, then the call
/// segments, then the root marker of every consumed root other than the
/// padding leaf. The adapter accepts its current root without a marker; the
/// marker is passed anyway, so the settlement still lands when another one
/// moves the root first.
fn settling(request: SettlementRequest<'_>) -> Result<Settling<'_>, PlanError> {
    let SettlementRequest {
        pa_program,
        payer,
        input,
        state,
        verifier_program,
        nullifiers,
        consumed_roots,
        created,
        accounts:
            CallAccounts {
                segments: call_segments,
                preceding,
            },
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

    let (router, verifier_entry) =
        derive_verifier_router_pdas(&state.verifier_router, state.proof_selector);
    Ok(Settling {
        pa_program,
        payer,
        accounts: SettlementAccounts {
            pa_state,
            new_root_marker,
            verifier_router_program: state.verifier_router,
            router,
            verifier_entry,
            verifier_program,
            remaining_accounts: remaining,
        },
        new_root,
        input,
        preceding,
    })
}

/// Refuses a settlement transaction, `message`, that names more accounts
/// than a transaction locks or is larger than a transaction.
fn within_limits(message: &v1::Message) -> Result<(), PlanError> {
    let accounts = message.account_keys.len();
    if accounts > usize::from(v1::MAX_ADDRESSES) {
        return Err(PlanError::TooManyAccounts { accounts });
    }
    let bytes = transaction_size(message);
    if bytes > v1::MAX_TRANSACTION_SIZE {
        return Err(PlanError::TooLarge { bytes });
    }
    Ok(())
}

/// The transactions of one settlement: one v1 transaction with the input
/// inline when that transaction fits `v1::MAX_TRANSACTION_SIZE`, otherwise
/// the upload's transactions and the settlement from the upload. A settlement
/// whose transaction names more accounts than a transaction locks, or is too
/// large even with its input uploaded, is refused.
///
/// `upload_id` tells the payer's concurrent uploads apart, and
/// `expires_slot` is the slot after which the upload may be reclaimed; both
/// are used only when the input is uploaded.
pub fn plan_settlement(
    request: SettlementRequest<'_>,
    upload_id: u64,
    expires_slot: u64,
) -> Result<SettlementPlan, PlanError> {
    let s = settling(request)?;
    let mut instructions = s.preceding;
    instructions.push(settle_ix(
        &s.pa_program,
        &s.payer,
        s.input,
        s.accounts.clone(),
    ));
    let mut settle = PlannedTransaction {
        instructions,
        config: SETTLE_TRANSACTION_CONFIG,
    };
    // The blockhash is a fixed 32 bytes, so any one gives the size.
    let mut message = settle.message(&s.payer, Hash::default())?;
    let upload = if transaction_size(&message) <= v1::MAX_TRANSACTION_SIZE {
        None
    } else {
        let (tx_data, _) = derive_tx_data_pda(&s.pa_program, &s.payer, upload_id);
        let pa_state = s.accounts.pa_state;
        settle.instructions.pop();
        settle.instructions.push(settle_from_txdata_ix(
            &s.pa_program,
            &tx_data,
            &s.payer,
            upload_id,
            s.accounts,
        ));
        message = settle.message(&s.payer, Hash::default())?;
        let upload_transaction = |ix| PlannedTransaction {
            instructions: vec![ix],
            config: UPLOAD_TRANSACTION_CONFIG,
        };
        Some(Upload {
            init: upload_transaction(txdata_init_ix(
                &s.pa_program,
                &pa_state,
                &tx_data,
                &s.payer,
                upload_id,
                s.input.len() as u32,
                expires_slot,
            )),
            writes: s
                .input
                .chunks(TXDATA_CHUNK_SIZE)
                .enumerate()
                .map(|(i, chunk)| {
                    upload_transaction(txdata_write_ix(
                        &s.pa_program,
                        &tx_data,
                        &s.payer,
                        upload_id,
                        (i * TXDATA_CHUNK_SIZE) as u32,
                        chunk,
                    ))
                })
                .collect(),
            close: upload_transaction(txdata_close_ix(
                &s.pa_program,
                &tx_data,
                &s.payer,
                &s.payer,
                upload_id,
            )),
        })
    };
    within_limits(&message)?;
    Ok(SettlementPlan {
        upload,
        settle,
        new_root: s.new_root,
    })
}

/// The transaction simulating one settlement (pa-evm's `simulateExecute`):
/// the settlement's one transaction with `simulate_execute` in place of
/// `settle`, verifying the RISC Zero proof unless
/// `skip_risc_zero_proof_verification`. Sent to the RPC's simulation, it
/// changes nothing and needs no signature. A simulation too large for one
/// transaction is refused (`TooLarge`): unlike a settlement, it is never
/// uploaded.
pub fn plan_simulation(
    request: SettlementRequest<'_>,
    skip_risc_zero_proof_verification: bool,
) -> Result<PlannedTransaction, PlanError> {
    let s = settling(request)?;
    let mut instructions = s.preceding;
    instructions.push(simulate_execute_ix(
        &s.pa_program,
        &s.payer,
        s.input,
        skip_risc_zero_proof_verification,
        s.accounts,
    ));
    let simulation = PlannedTransaction {
        instructions,
        config: SETTLE_TRANSACTION_CONFIG,
    };
    within_limits(&simulation.message(&s.payer, Hash::default())?)?;
    Ok(simulation)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::constants::MOCK_SELECTOR;
    use crate::generated::protocol_adapter::client::args;
    use crate::generated::protocol_adapter::constants::SCHEMA_VERSION;
    use anchor::Discriminator;

    fn key(n: u8) -> Pubkey {
        Pubkey::new_from_array([n; 32])
    }

    pub(crate) fn state_with(leaves: &[[u8; 32]]) -> PAStateAccount {
        let tree = CommitmentTreeState::over(leaves).unwrap();
        PAStateAccount {
            schema_version: SCHEMA_VERSION,
            bump: 255,
            owner: key(1),
            verifier_router: key(2),
            proof_selector: MOCK_SELECTOR,
            kind_tree_root: [0; 32],
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
            input,
            state,
            verifier_program: key(6),
            nullifiers,
            consumed_roots,
            created,
            accounts: CallAccounts {
                segments: call_segments,
                preceding: vec![],
            },
        }
    }

    /// What `plan_settlement` returned, briefly.
    fn outcome(result: &Result<SettlementPlan, PlanError>) -> String {
        match result {
            Ok(plan) => format!(
                "a plan with {} upload",
                if plan.upload.is_some() { "an" } else { "no" }
            ),
            Err(e) => e.to_string(),
        }
    }

    /// The adapter's instruction in `plan`'s settlement transaction, and how
    /// many named accounts precede its remaining ones: `settle_from_txdata`
    /// names the upload as well.
    fn settle_instruction(plan: &SettlementPlan) -> (&Instruction, usize) {
        let named = if plan.upload.is_some() { 11 } else { 10 };
        (plan.settle.instructions.last().unwrap(), named)
    }

    /// The size of the inline transaction carrying an input of `len` bytes,
    /// with nothing else in it.
    fn inline_size(state: &PAStateAccount, len: usize) -> usize {
        let input = vec![0u8; len];
        let plan = plan_settlement(request(state, &input, &[], &[], &[], vec![]), 7, 500).unwrap();
        assert!(plan.upload.is_none(), "an input of {len} bytes uploads");
        transaction_size(&plan.settle.message(&key(8), Hash::default()).unwrap())
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
            let plan = plan_settlement(
                request(
                    &state,
                    &input,
                    &nullifiers,
                    &roots,
                    &[],
                    vec![segment.clone()],
                ),
                7,
                500,
            )
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
        let plan = plan_settlement(
            request(&state, b"input", &[], &[], &created, vec![]),
            7,
            500,
        )
        .unwrap();
        let expected_root = CommitmentTreeState::over(&[[3; 32], [5; 32], [6; 32]])
            .unwrap()
            .root;
        assert_eq!(plan.new_root, Some(expected_root));
        let (pa_state, _) = derive_pa_state_pda(&key(9));
        let marker = derive_root_marker_pda(&key(9), &pa_state, &expected_root).0;
        // new_root_marker follows pa_state, the payer and the system program.
        let (settle, _) = settle_instruction(&plan);
        assert_eq!(settle.accounts[3], AccountMeta::new(marker, false));

        let none =
            plan_settlement(request(&state, b"input", &[], &[], &[], vec![]), 7, 500).unwrap();
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
        req.accounts.preceding = vec![preceding.clone()];
        let plan = plan_settlement(req, 7, 500).unwrap();
        assert!(plan.upload.is_none(), "a 5-byte input uploads");
        assert_eq!(plan.transaction_count(), 1);
        assert_eq!(plan.settle.config, SETTLE_TRANSACTION_CONFIG);
        let transaction = &plan.settle.instructions;
        // The preceding instructions first, then `settle` with the input.
        assert_eq!(transaction.len(), 2);
        assert_eq!(transaction[0], preceding);
        let settle = &transaction[1];
        assert_eq!(&settle.data[..8], args::Settle::DISCRIMINATOR);
        assert_eq!(settle.data[8..12], 5u32.to_le_bytes());
        assert_eq!(&settle.data[12..], b"input");
        let router_program = state.verifier_router;
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
        let plan = plan_settlement(request(&state, &input, &[], &[], &[], vec![]), 7, 500).unwrap();
        assert!(
            plan.upload.is_some(),
            "an input one byte over the inline limit is uploaded"
        );
    }

    #[test]
    fn a_settlement_too_large_for_one_transaction_is_uploaded_in_chunks_at_their_offsets() {
        let state = state_with(&[]);
        let input: Vec<u8> = (0..(2 * TXDATA_CHUNK_SIZE + 5)).map(|i| i as u8).collect();
        let preceding = Instruction::new_with_bytes(key(30), b"check", vec![]);
        let mut req = request(&state, &input, &[], &[], &[], vec![]);
        req.accounts.preceding = vec![preceding.clone()];
        let plan = plan_settlement(req, 7, 500).unwrap();
        // The init, three writes, the settlement and the close.
        assert_eq!(plan.transaction_count(), 6);
        let Some(Upload {
            init,
            writes,
            close,
        }) = plan.upload
        else {
            panic!("an input of two chunks settles inline");
        };
        let upload_transaction = |ix| PlannedTransaction {
            instructions: vec![ix],
            config: UPLOAD_TRANSACTION_CONFIG,
        };
        let (tx_data, _) = derive_tx_data_pda(&key(9), &key(8), 7);
        let (pa_state, _) = derive_pa_state_pda(&key(9));
        assert_eq!(
            init,
            upload_transaction(txdata_init_ix(
                &key(9),
                &pa_state,
                &tx_data,
                &key(8),
                7,
                input.len() as u32,
                500
            ))
        );
        // Two full chunks, then the 5-byte rest.
        let c = TXDATA_CHUNK_SIZE;
        let expected: Vec<PlannedTransaction> = [(0, c), (c, 2 * c), (2 * c, 2 * c + 5)]
            .iter()
            .map(|&(start, end)| {
                upload_transaction(txdata_write_ix(
                    &key(9),
                    &tx_data,
                    &key(8),
                    7,
                    start as u32,
                    &input[start..end],
                ))
            })
            .collect();
        assert_eq!(writes, expected);
        // The preceding instructions first, then `settle_from_txdata` of the upload.
        assert_eq!(plan.settle.config, SETTLE_TRANSACTION_CONFIG);
        let settle = &plan.settle.instructions;
        assert_eq!(settle.len(), 2);
        assert_eq!(settle[0], preceding);
        assert_eq!(&settle[1].data[..8], args::SettleFromTxdata::DISCRIMINATOR);
        assert_eq!(
            settle[1].accounts[1],
            AccountMeta::new_readonly(tx_data, false)
        );
        assert_eq!(
            close,
            upload_transaction(txdata_close_ix(&key(9), &tx_data, &key(8), &key(8), 7))
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
        let result = plan_settlement(
            request(&state, b"input", &nullifiers, &[], &[], vec![]),
            7,
            500,
        );
        assert!(
            matches!(result, Err(PlanError::TooManyAccounts { accounts }) if accounts > usize::from(v1::MAX_ADDRESSES)),
            "{}",
            outcome(&result)
        );
    }

    #[test]
    fn an_upload_whose_settlement_names_one_account_too_many_is_refused() {
        let state = state_with(&[]);
        let nullifiers_for = |n: usize| -> Vec<[u8; 32]> {
            (0..n)
                .map(|i| {
                    let mut nf = [0u8; 32];
                    nf[..8].copy_from_slice(&(i as u64).to_le_bytes());
                    nf
                })
                .collect()
        };
        let accounts = |nullifiers: &[[u8; 32]], input: &[u8]| {
            let plan =
                plan_settlement(request(&state, input, nullifiers, &[], &[], vec![]), 7, 500)
                    .unwrap();
            assert!(plan.upload.is_none(), "a 5-byte input uploads");
            plan.settle
                .message(&key(8), Hash::default())
                .unwrap()
                .account_keys
                .len()
        };
        // Enough nullifiers that `settle` names exactly the most accounts a
        // transaction locks; `settle_from_txdata` names the upload as well.
        let nullifiers = nullifiers_for(usize::from(v1::MAX_ADDRESSES) - accounts(&[], b"input"));
        assert_eq!(
            accounts(&nullifiers, b"input"),
            usize::from(v1::MAX_ADDRESSES)
        );
        let input = vec![0u8; v1::MAX_TRANSACTION_SIZE];
        let result = plan_settlement(
            request(&state, &input, &nullifiers, &[], &[], vec![]),
            7,
            500,
        );
        assert!(
            matches!(result, Err(PlanError::TooManyAccounts { accounts }) if accounts == usize::from(v1::MAX_ADDRESSES) + 1),
            "an upload whose settlement names {} accounts is planned: {}",
            usize::from(v1::MAX_ADDRESSES) + 1,
            outcome(&result)
        );
    }

    #[test]
    fn a_settlement_transaction_too_large_even_from_an_upload_is_refused() {
        let state = state_with(&[]);
        // Preceding instructions that alone overflow a transaction.
        let mut req = request(&state, b"input", &[], &[], &[], vec![]);
        req.accounts.preceding = vec![Instruction::new_with_bytes(
            key(30),
            &vec![0u8; v1::MAX_TRANSACTION_SIZE],
            vec![],
        )];
        let result = plan_settlement(req, 7, 500);
        assert!(
            matches!(result, Err(PlanError::TooLarge { bytes }) if bytes > v1::MAX_TRANSACTION_SIZE),
            "a settlement transaction over {} bytes is planned: {}",
            v1::MAX_TRANSACTION_SIZE,
            outcome(&result)
        );
    }

    #[test]
    fn a_simulation_is_the_settlement_transaction_with_simulate_execute_for_settle() {
        let state = state_with(&[[3; 32]]);
        let nullifiers = [[10; 32]];
        let roots = [state.root];
        let created = [[5; 32]];
        let segment = vec![AccountMeta::new(key(21), false)];
        let preceding = Instruction::new_with_bytes(key(30), b"check", vec![]);
        let request = || {
            let mut req = request(
                &state,
                b"input",
                &nullifiers,
                &roots,
                &created,
                vec![segment.clone()],
            );
            req.accounts.preceding = vec![preceding.clone()];
            req
        };
        let settlement = plan_settlement(request(), 7, 500).unwrap();
        assert!(settlement.upload.is_none(), "a 5-byte input uploads");
        for skip in [false, true] {
            let simulation = plan_simulation(request(), skip).unwrap();
            assert_eq!(simulation.config, settlement.settle.config);
            let [check, simulate] = &simulation.instructions[..] else {
                panic!("{} instructions", simulation.instructions.len());
            };
            let [_, settle] = &settlement.settle.instructions[..] else {
                panic!("{} instructions", settlement.settle.instructions.len());
            };
            assert_eq!(check, &preceding);
            assert_eq!(simulate.accounts, settle.accounts);
            // The request's 5-byte input, then the skip flag.
            assert_eq!(
                simulate.data,
                [
                    args::SimulateExecute::DISCRIMINATOR,
                    &5u32.to_le_bytes(),
                    b"input",
                    &[u8::from(skip)],
                ]
                .concat()
            );
        }
    }

    #[test]
    fn a_simulation_too_large_for_one_transaction_is_refused() {
        let state = state_with(&[]);
        let input = vec![0u8; v1::MAX_TRANSACTION_SIZE];
        let result = plan_simulation(request(&state, &input, &[], &[], &[], vec![]), true);
        assert!(
            matches!(result, Err(PlanError::TooLarge { bytes }) if bytes > v1::MAX_TRANSACTION_SIZE),
            "a simulation over {} bytes is planned: {}",
            v1::MAX_TRANSACTION_SIZE,
            match &result {
                Ok(_) => "a transaction".to_string(),
                Err(e) => e.to_string(),
            }
        );
    }

    #[test]
    fn a_full_chunk_is_the_largest_whose_write_transaction_fits() {
        let size = |len: usize| {
            let write = txdata_write_ix(&key(9), &key(7), &key(8), 7, 0, &vec![0u8; len]);
            let transaction = PlannedTransaction {
                instructions: vec![write],
                config: UPLOAD_TRANSACTION_CONFIG,
            };
            transaction_size(&transaction.message(&key(8), Hash::default()).unwrap())
        };
        assert!(size(TXDATA_CHUNK_SIZE) <= v1::MAX_TRANSACTION_SIZE);
        assert!(size(TXDATA_CHUNK_SIZE + 1) > v1::MAX_TRANSACTION_SIZE);
    }

    #[test]
    fn the_transaction_size_is_the_serialized_message_and_its_signatures() {
        let state = state_with(&[]);
        let plan =
            plan_settlement(request(&state, b"input", &[], &[], &[], vec![]), 7, 500).unwrap();
        let message = plan.settle.message(&key(8), Hash::default()).unwrap();
        assert_eq!(
            transaction_size(&message),
            message.serialize().len() + 64 * usize::from(message.header.num_required_signatures)
        );
    }
}
