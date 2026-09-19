//! SPL Associated Token Account helpers.
//!
//! ATA *derivation* lives in the `pda` module. This module provides the
//! Associated-Token-Account-program instruction builders that integrators need
//! when constructing settle transactions or wrap/unwrap flows.

use solana_program::{instruction::Instruction, pubkey::Pubkey};
use spl_associated_token_account_client::instruction::create_associated_token_account_idempotent;

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
    create_associated_token_account_idempotent(funder, wallet, token_mint, &spl_token::id())
}
