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
use crate::pda::derive_event_authority_pda;

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
/// new-root marker is NOT part of it: it is a required named account, passed
/// here as `new_root_marker` (writable) — the marker PDA of the
/// post-settlement root.
///
/// The PA emits its events as self-invocations (`#[event_cpi]`), which adds
/// two named accounts after `verifier_program`: the event authority PDA and
/// the PA program itself. Both are derived from `pa_program` here, so the
/// caller never supplies them.
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
        AccountMeta::new_readonly(derive_event_authority_pda(pa_program).0, false),
        AccountMeta::new_readonly(*pa_program, false),
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
    use crate::pda::derive_event_authority_pda;

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
    fn settle_from_txdata_account_layout_matches_v2_pa() {
        // The V2 PA's SettleFromTxData accounts struct, in declaration order:
        // pa_state(w), tx_data, authority(s), system_program,
        // new_root_marker(w), verifier_router_program, router,
        // verifier_entry, verifier_program, then the two accounts
        // `#[event_cpi]` appends: event_authority (PDA of
        // ["__event_authority"] under the PA) and program (the PA itself).
        // Remaining accounts follow. A builder missing the last two feeds
        // the first nullifier PDA where the PA expects the event authority.
        let keys: Vec<Pubkey> = (0..9).map(|_| Pubkey::new_unique()).collect();
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
            &keys[8],
            vec![],
        );
        assert_eq!(ix.accounts.len(), 11);
        let (event_authority, _) = derive_event_authority_pda(&keys[0]);
        let expect = [
            (keys[1], true, false),               // pa_state: writable
            (keys[2], false, false),              // tx_data
            (keys[3], true, true),                // authority: writable signer (pays marker rent)
            (system_program::id(), false, false), // system_program
            (keys[4], true, false),               // new_root_marker: writable
            (keys[5], false, false),              // verifier_router_program
            (keys[6], false, false),              // router
            (keys[7], false, false),              // verifier_entry
            (keys[8], false, false),              // verifier_program
            (event_authority, false, false),      // event_authority
            (keys[0], false, false),              // program
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
    fn remaining_accounts_follow_the_event_cpi_accounts() {
        let pa = Pubkey::new_unique();
        let nullifier_pda = Pubkey::new_unique();
        let ix = settle_from_txdata_ix(
            &pa,
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            1,
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            vec![AccountMeta::new(nullifier_pda, false)],
        );
        assert_eq!(ix.accounts.len(), 12);
        assert_eq!(
            ix.accounts[10].pubkey, pa,
            "program precedes remaining accounts"
        );
        assert_eq!(ix.accounts[11].pubkey, nullifier_pda);
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
