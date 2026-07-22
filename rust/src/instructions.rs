//! Builders for the PA's `txdata_*` and `settle_from_txdata` instructions.
//!
//! Each builder serializes the Anchor discriminator + arguments and lays out
//! the accounts in the order the PA program expects. The verifier-router
//! account fan-out (4 accounts) lives in `derive_verifier_router_pdas` in the
//! `pda` module.

use solana_program::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
};
use solana_sdk_ids::system_program;

use crate::{
    discriminator::anchor_instruction_disc,
    external_call::SolanaExternalCall,
    pda::{derive_nullifier_pda, derive_root_marker_pda},
};

/// Proof-derived inputs for the PA's canonical settlement account layout.
///
/// Nullifier and root marker addresses are derived by this crate. External-call
/// program and account identities, writable capabilities, and ordering come
/// directly from the proof-bound [`SolanaExternalCall`] values. No signer
/// privilege is forwarded to a proof-selected CPI.
pub struct SettlementAccountInputs {
    pub nullifiers: Vec<[u8; 32]>,
    pub external_calls: Vec<SolanaExternalCall>,
    /// Unique, ordered consumed roots that are neither the current root nor the
    /// padding root and therefore require historical marker accounts.
    pub historical_roots: Vec<[u8; 32]>,
    pub produced_root: [u8; 32],
}

fn settlement_account_metas(
    pa_program: &Pubkey,
    pa_state: &Pubkey,
    inputs: &SettlementAccountInputs,
) -> Vec<AccountMeta> {
    let external_account_count = inputs
        .external_calls
        .iter()
        .map(|call| call.accounts.len() + 1)
        .sum::<usize>();
    let mut accounts = Vec::with_capacity(
        inputs.nullifiers.len() + external_account_count + inputs.historical_roots.len() + 1,
    );

    for nullifier in &inputs.nullifiers {
        let (marker, _) = derive_nullifier_pda(pa_program, pa_state, nullifier);
        accounts.push(AccountMeta::new(marker, false));
    }

    for call in &inputs.external_calls {
        accounts.push(AccountMeta::new_readonly(
            Pubkey::new_from_array(call.program_id),
            false,
        ));
        accounts.extend(call.accounts.iter().map(|meta| {
            let key = Pubkey::new_from_array(meta.pubkey);
            if meta.is_writable {
                AccountMeta::new(key, false)
            } else {
                AccountMeta::new_readonly(key, false)
            }
        }));
    }

    for root in &inputs.historical_roots {
        let (marker, _) = derive_root_marker_pda(pa_program, pa_state, root);
        accounts.push(AccountMeta::new_readonly(marker, false));
    }

    let (produced_root_marker, _) =
        derive_root_marker_pda(pa_program, pa_state, &inputs.produced_root);
    accounts.push(AccountMeta::new(produced_root_marker, false));
    accounts
}

/// Build a PA `txdata_init` instruction.
pub fn txdata_init_ix(
    pa_program: &Pubkey,
    pa_state: &Pubkey,
    tx_data: &Pubkey,
    authority: &Pubkey,
    upload_id: u64,
    capacity: u32,
    expires_slot: u64,
) -> Instruction {
    let disc = anchor_instruction_disc("txdata_init");
    let mut data = Vec::with_capacity(8 + 8 + 4 + 8);
    data.extend_from_slice(&disc);
    data.extend_from_slice(&upload_id.to_le_bytes());
    data.extend_from_slice(&capacity.to_le_bytes());
    data.extend_from_slice(&expires_slot.to_le_bytes());

    Instruction {
        program_id: *pa_program,
        accounts: vec![
            AccountMeta::new_readonly(*pa_state, false),
            AccountMeta::new(*tx_data, false),
            AccountMeta::new(*authority, true),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
        data,
    }
}

/// Build a PA `txdata_write` instruction for a single chunk.
///
/// `chunk.len()` is bounded by `TXDATA_CHUNK_SIZE` so the resulting tx fits
/// within Solana's 1232-byte transaction limit.
pub fn txdata_write_ix(
    pa_program: &Pubkey,
    tx_data: &Pubkey,
    authority: &Pubkey,
    upload_id: u64,
    offset: u32,
    chunk: &[u8],
) -> Instruction {
    let disc = anchor_instruction_disc("txdata_write");
    let mut data = Vec::with_capacity(8 + 8 + 4 + 4 + chunk.len());
    data.extend_from_slice(&disc);
    data.extend_from_slice(&upload_id.to_le_bytes());
    data.extend_from_slice(&offset.to_le_bytes());
    // Anchor serializes Vec<u8> as 4-byte LE length prefix + bytes.
    data.extend_from_slice(&(chunk.len() as u32).to_le_bytes());
    data.extend_from_slice(chunk);

    Instruction {
        program_id: *pa_program,
        accounts: vec![
            AccountMeta::new(*tx_data, false),
            AccountMeta::new_readonly(*authority, true),
        ],
        data,
    }
}

/// Build a PA `settle_from_txdata` instruction.
///
/// The remaining accounts are derived in the PA's sole accepted order:
/// nullifier markers, proof-bound external-call segments, ordered historical
/// root markers, and exactly one produced-root marker.
#[allow(clippy::too_many_arguments)]
pub fn settle_from_txdata_ix(
    pa_program: &Pubkey,
    pa_state: &Pubkey,
    tx_data: &Pubkey,
    authority: &Pubkey,
    upload_id: u64,
    verifier_router_program: &Pubkey,
    router: &Pubkey,
    verifier_entry: &Pubkey,
    verifier_program: &Pubkey,
    settlement_inputs: &SettlementAccountInputs,
) -> Instruction {
    let disc = anchor_instruction_disc("settle_from_txdata");
    let mut data = Vec::with_capacity(8 + 8);
    data.extend_from_slice(&disc);
    data.extend_from_slice(&upload_id.to_le_bytes());

    let mut accounts = vec![
        AccountMeta::new(*pa_state, false),
        AccountMeta::new_readonly(*tx_data, false),
        AccountMeta::new(*authority, true),
        AccountMeta::new_readonly(system_program::id(), false),
        AccountMeta::new_readonly(*verifier_router_program, false),
        AccountMeta::new_readonly(*router, false),
        AccountMeta::new_readonly(*verifier_entry, false),
        AccountMeta::new_readonly(*verifier_program, false),
    ];
    accounts.extend(settlement_account_metas(
        pa_program,
        pa_state,
        settlement_inputs,
    ));

    Instruction {
        program_id: *pa_program,
        accounts,
        data,
    }
}

/// Build a PA `txdata_close` instruction (reclaims rent from the upload PDA).
pub fn txdata_close_ix(
    pa_program: &Pubkey,
    tx_data: &Pubkey,
    authority: &Pubkey,
    refund: &Pubkey,
    upload_id: u64,
) -> Instruction {
    let disc = anchor_instruction_disc("txdata_close");
    let mut data = Vec::with_capacity(8 + 8);
    data.extend_from_slice(&disc);
    data.extend_from_slice(&upload_id.to_le_bytes());

    Instruction {
        program_id: *pa_program,
        accounts: vec![
            AccountMeta::new(*tx_data, false),
            AccountMeta::new_readonly(*authority, true),
            AccountMeta::new(*refund, false),
        ],
        data,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn txdata_init_disc_is_first_8_bytes() {
        let ix = txdata_init_ix(
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            1,
            900,
            100,
        );
        assert_eq!(&ix.data[..8], &anchor_instruction_disc("txdata_init"));
    }

    #[test]
    fn txdata_write_includes_length_prefix() {
        let chunk = [1u8, 2, 3, 4, 5];
        let ix = txdata_write_ix(
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            7,
            0,
            &chunk,
        );
        // 8 disc + 8 upload_id + 4 offset + 4 vec_len + 5 chunk
        assert_eq!(ix.data.len(), 29);
        // Length prefix at offset 20 is 5 (Anchor Vec<u8> length).
        assert_eq!(&ix.data[20..24], &5u32.to_le_bytes());
    }

    #[test]
    fn settle_accounts_are_derived_in_canonical_order() {
        let pa_program = Pubkey::new_unique();
        let pa_state = Pubkey::new_unique();
        let nullifier = [1u8; 32];
        let external_program = [2u8; 32];
        let writable_external_account = [3u8; 32];
        let readonly_external_account = [4u8; 32];
        let historical_root = [5u8; 32];
        let produced_root = [6u8; 32];
        let external_call = SolanaExternalCall {
            program_id: external_program,
            instruction_data: vec![7],
            expected_output: vec![8],
            output_mode: crate::external_call::OutputMode::ReturnData,
            accounts: vec![
                crate::external_call::SolanaAccountMeta {
                    pubkey: writable_external_account,
                    is_writable: true,
                },
                crate::external_call::SolanaAccountMeta {
                    pubkey: readonly_external_account,
                    is_writable: false,
                },
            ],
        };
        let inputs = SettlementAccountInputs {
            nullifiers: vec![nullifier],
            external_calls: vec![external_call],
            historical_roots: vec![historical_root],
            produced_root,
        };
        let ix = settle_from_txdata_ix(
            &pa_program,
            &pa_state,
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            1,
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &inputs,
        );

        let (nullifier_marker, _) = derive_nullifier_pda(&pa_program, &pa_state, &nullifier);
        let (historical_root_marker, _) =
            derive_root_marker_pda(&pa_program, &pa_state, &historical_root);
        let (produced_root_marker, _) =
            derive_root_marker_pda(&pa_program, &pa_state, &produced_root);
        assert_eq!(
            &ix.accounts[8..],
            &[
                AccountMeta::new(nullifier_marker, false),
                AccountMeta::new_readonly(Pubkey::new_from_array(external_program), false),
                AccountMeta::new(Pubkey::new_from_array(writable_external_account), false),
                AccountMeta::new_readonly(Pubkey::new_from_array(readonly_external_account), false,),
                AccountMeta::new_readonly(historical_root_marker, false),
                AccountMeta::new(produced_root_marker, false),
            ]
        );
        assert!(ix.accounts[8..].iter().all(|meta| !meta.is_signer));
    }
}
