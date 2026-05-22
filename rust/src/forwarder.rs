//! Builders for the forwarder CPI account segments inside a settle transaction's
//! `remaining_accounts`. Ordering is owned by the forwarder program; integrators
//! must use these builders rather than hand-rolling the slice.

use solana_program::{instruction::AccountMeta, pubkey::Pubkey, sysvar};
use solana_sdk_ids::system_program;

use crate::constants::{FORWARDER_UNWRAP_NUM_ACCOUNTS, FORWARDER_WRAP_NUM_ACCOUNTS};
use crate::pda::{
    derive_associated_token_address, derive_forwarder_config_pda, derive_forwarder_escrow_pda,
    derive_nonce_bitmap_pda,
};
use crate::program_ids::SPL_TOKEN_PROGRAM_ID;

/// Word index for the nonce bitmap PDA seed. Each bitmap covers 256 nonces.
const NONCES_PER_WORD: u64 = 256;

/// Build the 12-account wrap forwarder CPI segment.
///
/// Order: `[forwarder_program, config, ix_sysvar, clock, user_ata, escrow_ata,
/// escrow_pda, nonce_bitmap_pda, token_program, system_program, payer, mint]`.
pub fn build_wrap_forwarder_accounts(
    forwarder_program: &Pubkey,
    payer: &Pubkey,
    user: &Pubkey,
    token_mint: &Pubkey,
    nonce: u64,
) -> Vec<AccountMeta> {
    let (config_pda, _) = derive_forwarder_config_pda(forwarder_program);
    let (escrow_pda, _) = derive_forwarder_escrow_pda(forwarder_program, token_mint);
    let user_ata = derive_associated_token_address(user, token_mint);
    let escrow_ata = derive_associated_token_address(&escrow_pda, token_mint);
    let word_index = nonce / NONCES_PER_WORD;
    let (nonce_bitmap_pda, _) = derive_nonce_bitmap_pda(forwarder_program, user, word_index);

    let accounts = vec![
        AccountMeta::new_readonly(*forwarder_program, false), // segment marker
        AccountMeta::new_readonly(config_pda, false),         // config
        AccountMeta::new_readonly(sysvar::instructions::id(), false), // ix sysvar
        AccountMeta::new_readonly(sysvar::clock::id(), false), // clock
        AccountMeta::new(user_ata, false),                    // user ATA
        AccountMeta::new(escrow_ata, false),                  // escrow ATA
        AccountMeta::new_readonly(escrow_pda, false),         // escrow PDA
        AccountMeta::new(nonce_bitmap_pda, false),            // nonce bitmap
        AccountMeta::new_readonly(SPL_TOKEN_PROGRAM_ID, false), // token program
        AccountMeta::new_readonly(system_program::id(), false), // system program
        AccountMeta::new(*payer, false),                      // payer for nonce bitmap
        AccountMeta::new_readonly(*token_mint, false),        // mint
    ];
    debug_assert_eq!(accounts.len(), FORWARDER_WRAP_NUM_ACCOUNTS as usize);
    accounts
}

/// Build the 9-account unwrap forwarder CPI segment.
///
/// Order: `[forwarder_program, config, ix_sysvar, clock, escrow_ata,
/// recipient_ata, escrow_pda, token_program, mint]`.
pub fn build_unwrap_forwarder_accounts(
    forwarder_program: &Pubkey,
    recipient: &Pubkey,
    token_mint: &Pubkey,
) -> Vec<AccountMeta> {
    let (config_pda, _) = derive_forwarder_config_pda(forwarder_program);
    let (escrow_pda, _) = derive_forwarder_escrow_pda(forwarder_program, token_mint);
    let escrow_ata = derive_associated_token_address(&escrow_pda, token_mint);
    let recipient_ata = derive_associated_token_address(recipient, token_mint);

    let accounts = vec![
        AccountMeta::new_readonly(*forwarder_program, false), // segment marker
        AccountMeta::new_readonly(config_pda, false),         // config
        AccountMeta::new_readonly(sysvar::instructions::id(), false), // ix sysvar
        AccountMeta::new_readonly(sysvar::clock::id(), false), // clock
        AccountMeta::new(escrow_ata, false),                  // escrow ATA
        AccountMeta::new(recipient_ata, false),               // recipient ATA
        AccountMeta::new_readonly(escrow_pda, false),         // escrow PDA
        AccountMeta::new_readonly(SPL_TOKEN_PROGRAM_ID, false), // token program
        AccountMeta::new_readonly(*token_mint, false),        // mint
    ];
    debug_assert_eq!(accounts.len(), FORWARDER_UNWRAP_NUM_ACCOUNTS as usize);
    accounts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_segment_has_12_accounts() {
        let accs = build_wrap_forwarder_accounts(
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            0,
        );
        assert_eq!(accs.len(), FORWARDER_WRAP_NUM_ACCOUNTS as usize);
    }

    #[test]
    fn unwrap_segment_has_9_accounts() {
        let accs = build_unwrap_forwarder_accounts(
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
        );
        assert_eq!(accs.len(), FORWARDER_UNWRAP_NUM_ACCOUNTS as usize);
    }
}
