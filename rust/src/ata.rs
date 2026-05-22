//! SPL Associated Token Account helpers.
//!
//! ATA *derivation* lives in the `pda` module. This module provides the
//! Associated-Token-Account-program instruction builders that integrators need
//! when constructing settle transactions or wrap/unwrap flows.

use solana_program::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
};
use solana_sdk_ids::system_program;

use crate::pda::derive_associated_token_address;
use crate::program_ids::{ASSOCIATED_TOKEN_PROGRAM_ID, SPL_TOKEN_PROGRAM_ID};

/// Build an idempotent ATA-creation instruction. No-op if the ATA already
/// exists; creates it otherwise. Funder pays rent.
///
/// Must run before any unwrap settlement whose recipient ATA may not yet exist
/// (the forwarder's SPL `Transfer` requires the destination to exist).
pub fn create_ata_idempotent_ix(
    funder: &Pubkey,
    wallet: &Pubkey,
    token_mint: &Pubkey,
) -> Instruction {
    let ata = derive_associated_token_address(wallet, token_mint);
    // Instruction byte `1` is `CreateIdempotent` in the SPL Associated Token
    // Account program's instruction enum.
    Instruction {
        program_id: ASSOCIATED_TOKEN_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(*funder, true),               // funding account
            AccountMeta::new(ata, false),                  // associated token account
            AccountMeta::new_readonly(*wallet, false),     // wallet
            AccountMeta::new_readonly(*token_mint, false), // token mint
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new_readonly(SPL_TOKEN_PROGRAM_ID, false),
        ],
        data: vec![1],
    }
}
