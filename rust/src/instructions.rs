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

use crate::discriminator::anchor_instruction_disc;

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
/// `remaining_accounts` is the caller-assembled slice covering nullifier PDAs,
/// per-call forwarder CPI segments, and historical root markers. The
/// new-root marker is NOT part of it: since the STATE-03 remediation it is a
/// required named account, passed here as `new_root_marker` (writable) —
/// the marker PDA of the post-settlement root.
#[allow(clippy::too_many_arguments)]
pub fn settle_from_txdata_ix(
    pa_program: &Pubkey,
    pa_state: &Pubkey,
    tx_data: &Pubkey,
    authority: &Pubkey,
    upload_id: u64,
    new_root_marker: &Pubkey,
    verifier_router_program: &Pubkey,
    router: &Pubkey,
    verifier_entry: &Pubkey,
    verifier_program: &Pubkey,
    remaining_accounts: Vec<AccountMeta>,
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
        AccountMeta::new(*new_root_marker, false),
        AccountMeta::new_readonly(*verifier_router_program, false),
        AccountMeta::new_readonly(*router, false),
        AccountMeta::new_readonly(*verifier_entry, false),
        AccountMeta::new_readonly(*verifier_program, false),
    ];
    accounts.extend(remaining_accounts);

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
    fn settle_from_txdata_account_layout_matches_remediated_pa() {
        // The PA's SettleFromTxData accounts struct, in declaration order:
        // pa_state(w), tx_data, authority(s), system_program,
        // new_root_marker(w), verifier_router_program, router,
        // verifier_entry, verifier_program. STATE-03 inserted
        // new_root_marker after system_program; a builder without it (or
        // with it in remaining_accounts) is off by one from there on.
        let keys: Vec<Pubkey> = (0..8).map(|_| Pubkey::new_unique()).collect();
        let ix = settle_from_txdata_ix(
            &keys[0],
            &keys[1],
            &keys[2],
            &keys[3],
            42,
            &keys[4],
            &keys[5],
            &keys[6],
            &keys[7],
            &Pubkey::new_unique(),
            vec![],
        );
        assert_eq!(ix.accounts.len(), 9);
        let expect = [
            (keys[1], true, false),               // pa_state: writable
            (keys[2], false, false),              // tx_data
            (keys[3], true, true),                // authority: writable signer (pays marker rent)
            (system_program::id(), false, false), // system_program
            (keys[4], true, false),               // new_root_marker: writable
            (keys[5], false, false),              // verifier_router_program
            (keys[6], false, false),              // router
            (keys[7], false, false),              // verifier_entry
        ];
        for (i, (key, writable, signer)) in expect.iter().enumerate() {
            assert_eq!(ix.accounts[i].pubkey, *key, "account {i} pubkey");
            assert_eq!(
                ix.accounts[i].is_writable, *writable,
                "account {i} writable"
            );
            assert_eq!(ix.accounts[i].is_signer, *signer, "account {i} signer");
        }
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
}
