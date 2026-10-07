//! The transactions of one settlement, and the adapter's part of a
//! deployment's settlement lookup table.

use std::collections::BTreeSet;

use solana_compute_budget_interface::ComputeBudgetInstruction;
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;
use solana_sdk_ids::system_program;

use crate::accounts::PAStateAccount;
use crate::constants::{SETTLE_COMPUTE_UNIT_LIMIT, SETTLE_HEAP_FRAME_BYTES, TXDATA_CHUNK_SIZE};
use crate::instructions::{
    settle_from_txdata_ix, txdata_close_ix, txdata_init_ix, txdata_write_ix,
};
use crate::merkle::{CommitmentTreeState, MerkleError, PADDING_LEAF};
use crate::pda::{
    derive_event_authority_pda, derive_nullifier_pda, derive_pa_state_pda, derive_root_marker_pda,
    derive_tx_data_pda, derive_verifier_router_pdas,
};

/// What one settlement through a transaction-data upload needs.
pub struct SettlementRequest<'a> {
    pub pa_program: Pubkey,
    /// Pays for and owns the upload, and signs every transaction.
    pub payer: Pubkey,
    /// Tells the payer's concurrent uploads apart.
    pub upload_id: u64,
    /// The slot after which the upload may be reclaimed.
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
}

/// The instructions of one settlement, each list one transaction.
#[derive(Debug)]
pub struct SettlementPlan {
    /// Creates the upload.
    pub init: Instruction,
    /// Fills the upload, one chunk per transaction.
    pub writes: Vec<Instruction>,
    /// The settlement: its compute budget, then `settle_from_txdata`. A caller
    /// with instructions of its own (an ed25519 signature check, an account
    /// the settlement needs) puts them first.
    pub settle: Vec<Instruction>,
    /// Closes the upload, whatever the settlement's outcome.
    pub close: Instruction,
    /// The root the adapter holds after the settlement; `None` when it creates
    /// nothing and so produces no root.
    pub new_root: Option<[u8; 32]>,
}

/// The transactions that upload `request.input`, settle it and close the
/// upload.
///
/// `settle_from_txdata`'s remaining accounts are the nullifier PDAs, then the
/// call segments, then the root marker of every consumed root other than the
/// padding leaf. The adapter accepts its current root without a marker; the
/// marker is passed anyway, so the settlement still lands when another one
/// moves the root first.
pub fn plan_settlement(request: SettlementRequest<'_>) -> Result<SettlementPlan, MerkleError> {
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
    } = request;
    let (pa_state, _) = derive_pa_state_pda(&pa_program);
    let (tx_data, _) = derive_tx_data_pda(&pa_program, &payer, upload_id);

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
    let settle = vec![
        ComputeBudgetInstruction::set_compute_unit_limit(SETTLE_COMPUTE_UNIT_LIMIT),
        ComputeBudgetInstruction::request_heap_frame(SETTLE_HEAP_FRAME_BYTES),
        settle_from_txdata_ix(
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
        ),
    ];

    Ok(SettlementPlan {
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
        new_root,
    })
}

/// The accounts the adapter's part of every settlement carries that a lookup
/// table can hold (neither a signer nor an invoked program): its state, the
/// system program, the verifier router, the router's state and its entry for
/// the selector, the verifier program, and the adapter's event authority. Each
/// forwarder a deployment serves adds its own.
pub fn adapter_settlement_lookup_keys(
    pa_program: &Pubkey,
    verifier_router: &Pubkey,
    proof_selector: [u8; 4],
    verifier_program: &Pubkey,
) -> Vec<Pubkey> {
    let (router, verifier_entry) = derive_verifier_router_pdas(verifier_router, proof_selector);
    vec![
        derive_pa_state_pda(pa_program).0,
        system_program::id(),
        *verifier_router,
        router,
        verifier_entry,
        *verifier_program,
        derive_event_authority_pda(pa_program).0,
    ]
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
        }
    }

    /// `settle_from_txdata`'s named accounts, which come before the remaining ones.
    const SETTLE_NAMED_ACCOUNTS: usize = 11;

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
        let plan = plan_settlement(request(
            &state,
            b"input",
            &nullifiers,
            &roots,
            &[],
            vec![segment.clone()],
        ))
        .unwrap();

        let (pa_state, _) = derive_pa_state_pda(&key(9));
        let settle = plan.settle.last().unwrap();
        let mut expected: Vec<AccountMeta> = nullifiers
            .iter()
            .map(|nf| AccountMeta::new(derive_nullifier_pda(&key(9), &pa_state, nf).0, false))
            .collect();
        expected.extend(segment);
        // One marker per distinct root other than the padding leaf, the current root included.
        let mut markers: Vec<[u8; 32]> = vec![state.root, earlier_root];
        markers.sort();
        expected.extend(markers.iter().map(|root| {
            AccountMeta::new_readonly(derive_root_marker_pda(&key(9), &pa_state, root).0, false)
        }));
        assert_eq!(settle.accounts[SETTLE_NAMED_ACCOUNTS..], expected[..]);
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
        // new_root_marker is settle_from_txdata's fifth account.
        assert_eq!(plan.settle[2].accounts[4], AccountMeta::new(marker, false));

        let none = plan_settlement(request(&state, b"input", &[], &[], &[], vec![])).unwrap();
        assert_eq!(none.new_root, None);
        // Anchor marks an absent optional account by the program's own address.
        assert_eq!(
            none.settle[2].accounts[4],
            AccountMeta::new_readonly(key(9), false)
        );
    }

    #[test]
    fn the_upload_is_written_in_chunks_at_their_offsets() {
        let state = state_with(&[]);
        let input: Vec<u8> = (0..(2 * TXDATA_CHUNK_SIZE + 5)).map(|i| i as u8).collect();
        let plan = plan_settlement(request(&state, &input, &[], &[], &[], vec![])).unwrap();
        let (tx_data, _) = derive_tx_data_pda(&key(9), &key(8), 7);
        let (pa_state, _) = derive_pa_state_pda(&key(9));
        assert_eq!(
            plan.init,
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
        assert_eq!(plan.writes, expected);
        assert_eq!(
            plan.close,
            txdata_close_ix(&key(9), &tx_data, &key(8), &key(8), 7)
        );
    }

    #[test]
    fn the_settlement_sets_its_compute_budget_and_names_the_entry_for_the_adapters_selector() {
        let state = state_with(&[]);
        let plan = plan_settlement(request(&state, b"input", &[], &[], &[], vec![])).unwrap();
        assert_eq!(plan.settle.len(), 3);
        assert_eq!(
            plan.settle[0],
            ComputeBudgetInstruction::set_compute_unit_limit(SETTLE_COMPUTE_UNIT_LIMIT)
        );
        assert_eq!(
            plan.settle[1],
            ComputeBudgetInstruction::request_heap_frame(SETTLE_HEAP_FRAME_BYTES)
        );
        let router_program = Pubkey::new_from_array(state.verifier_router);
        let (router, entry) = derive_verifier_router_pdas(&router_program, MOCK_SELECTOR);
        let settle = &plan.settle[2];
        assert_eq!(
            settle.data[..8],
            crate::discriminator::anchor_instruction_disc("settle_from_txdata")
        );
        for k in [router_program, router, entry, key(6)] {
            assert!(
                settle.accounts.iter().any(|a| a.pubkey == k),
                "settle names {k}"
            );
        }
    }

    #[test]
    fn the_adapters_lookup_keys_are_its_state_router_entry_verifier_and_event_authority() {
        let pa = key(9);
        let (router, entry) = derive_verifier_router_pdas(&key(2), MOCK_SELECTOR);
        assert_eq!(
            adapter_settlement_lookup_keys(&pa, &key(2), MOCK_SELECTOR, &key(6)),
            vec![
                derive_pa_state_pda(&pa).0,
                system_program::id(),
                key(2),
                router,
                entry,
                key(6),
                derive_event_authority_pda(&pa).0,
            ]
        );
    }
}
