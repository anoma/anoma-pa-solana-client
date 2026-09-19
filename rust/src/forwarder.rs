//! Builders for the forwarder's CPI account segments inside a settle
//! transaction's `remaining_accounts`, and for the forwarder's own
//! `init_nonce_bitmap` instruction. Ordering is owned by the forwarder program;
//! integrators must use these builders rather than hand-rolling the slice.

use solana_program::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    sysvar,
};
use solana_sdk_ids::system_program;

use crate::constants::{FORWARDER_UNWRAP_NUM_ACCOUNTS, FORWARDER_WRAP_NUM_ACCOUNTS};
use crate::discriminator::anchor_instruction_disc;
use crate::pda::{
    derive_associated_token_address, derive_forwarder_config_pda, derive_forwarder_escrow_pda,
    derive_nonce_bitmap_pda,
};

/// Nonces per nonce-bitmap account. The bitmap for `nonce` is the one for
/// word `nonce / NONCES_PER_WORD`.
pub const NONCES_PER_WORD: u64 = 256;

/// The nonce-bitmap word a wrap nonce falls in.
pub fn nonce_word_index(nonce: u64) -> u64 {
    nonce / NONCES_PER_WORD
}

/// Build the forwarder's permissionless `init_nonce_bitmap` instruction, which
/// creates `user`'s bitmap for `word_index` with `payer` funding the rent.
///
/// A wrap needs the bitmap for its nonce's word to exist, so a settlement
/// whose word has no bitmap yet (the account at
/// `derive_nonce_bitmap_pda(forwarder_program, user, word_index)` is absent)
/// carries this instruction before the settle instruction. It fits in the
/// settlement transaction after the ed25519 instruction.
pub fn init_nonce_bitmap_ix(
    forwarder_program: &Pubkey,
    payer: &Pubkey,
    user: &Pubkey,
    word_index: u64,
) -> Instruction {
    let (nonce_bitmap_pda, _) = derive_nonce_bitmap_pda(forwarder_program, user, word_index);
    let mut data = Vec::with_capacity(8 + 32 + 8);
    data.extend_from_slice(&anchor_instruction_disc("init_nonce_bitmap"));
    data.extend_from_slice(user.as_ref());
    data.extend_from_slice(&word_index.to_le_bytes());
    Instruction {
        program_id: *forwarder_program,
        accounts: vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new(nonce_bitmap_pda, false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
        data,
    }
}

/// Build the wrap forwarder CPI segment.
///
/// Order: `[forwarder_program, config, ix_sysvar, user_ata, escrow_ata,
/// escrow_pda, nonce_bitmap_pda, token_program]`.
pub fn build_wrap_forwarder_accounts(
    forwarder_program: &Pubkey,
    user: &Pubkey,
    token_mint: &Pubkey,
    nonce: u64,
) -> Vec<AccountMeta> {
    let (config_pda, _) = derive_forwarder_config_pda(forwarder_program);
    let (escrow_pda, _) = derive_forwarder_escrow_pda(forwarder_program, token_mint);
    let user_ata = derive_associated_token_address(user, token_mint);
    let escrow_ata = derive_associated_token_address(&escrow_pda, token_mint);
    let (nonce_bitmap_pda, _) =
        derive_nonce_bitmap_pda(forwarder_program, user, nonce_word_index(nonce));

    let accounts = vec![
        AccountMeta::new_readonly(*forwarder_program, false), // segment marker
        AccountMeta::new_readonly(config_pda, false),         // config
        AccountMeta::new_readonly(sysvar::instructions::id(), false), // ix sysvar
        AccountMeta::new(user_ata, false),                    // user ATA
        AccountMeta::new(escrow_ata, false),                  // escrow ATA
        AccountMeta::new_readonly(escrow_pda, false),         // escrow PDA
        AccountMeta::new(nonce_bitmap_pda, false),            // nonce bitmap
        AccountMeta::new_readonly(spl_token::id(), false),    // token program
    ];
    debug_assert_eq!(accounts.len(), FORWARDER_WRAP_NUM_ACCOUNTS as usize);
    accounts
}

/// Build the unwrap forwarder CPI segment.
///
/// Order: `[forwarder_program, config, ix_sysvar, escrow_ata, recipient_ata,
/// escrow_pda, token_program]`.
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
        AccountMeta::new(escrow_ata, false),                  // escrow ATA
        AccountMeta::new(recipient_ata, false),               // recipient ATA
        AccountMeta::new_readonly(escrow_pda, false),         // escrow PDA
        AccountMeta::new_readonly(spl_token::id(), false),    // token program
    ];
    debug_assert_eq!(accounts.len(), FORWARDER_UNWRAP_NUM_ACCOUNTS as usize);
    accounts
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn canonical_spl_token_program_id() -> Pubkey {
        Pubkey::from_str("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA").unwrap()
    }

    fn keys(accounts: &[AccountMeta]) -> Vec<Pubkey> {
        accounts.iter().map(|a| a.pubkey).collect()
    }

    #[test]
    fn wrap_segment_is_the_forwarder_layout() {
        let forwarder = Pubkey::new_unique();
        let user = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let nonce = 300;
        let accs = build_wrap_forwarder_accounts(&forwarder, &user, &mint, nonce);
        assert_eq!(accs.len(), FORWARDER_WRAP_NUM_ACCOUNTS as usize);

        let escrow_pda = derive_forwarder_escrow_pda(&forwarder, &mint).0;
        assert_eq!(
            keys(&accs),
            vec![
                forwarder,
                derive_forwarder_config_pda(&forwarder).0,
                sysvar::instructions::id(),
                derive_associated_token_address(&user, &mint),
                derive_associated_token_address(&escrow_pda, &mint),
                escrow_pda,
                derive_nonce_bitmap_pda(&forwarder, &user, 1).0, // nonce 300 is in word 1
                canonical_spl_token_program_id(),
            ]
        );
        let writable: Vec<bool> = accs.iter().map(|a| a.is_writable).collect();
        assert_eq!(
            writable,
            [false, false, false, true, true, false, true, false],
            "the user ATA, escrow ATA and nonce bitmap are written"
        );
        assert!(accs.iter().all(|a| !a.is_signer));
    }

    #[test]
    fn unwrap_segment_is_the_forwarder_layout() {
        let forwarder = Pubkey::new_unique();
        let recipient = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let accs = build_unwrap_forwarder_accounts(&forwarder, &recipient, &mint);
        assert_eq!(accs.len(), FORWARDER_UNWRAP_NUM_ACCOUNTS as usize);

        let escrow_pda = derive_forwarder_escrow_pda(&forwarder, &mint).0;
        assert_eq!(
            keys(&accs),
            vec![
                forwarder,
                derive_forwarder_config_pda(&forwarder).0,
                sysvar::instructions::id(),
                derive_associated_token_address(&escrow_pda, &mint),
                derive_associated_token_address(&recipient, &mint),
                escrow_pda,
                canonical_spl_token_program_id(),
            ]
        );
        let writable: Vec<bool> = accs.iter().map(|a| a.is_writable).collect();
        assert_eq!(
            writable,
            [false, false, false, true, true, false, false],
            "the escrow ATA and recipient ATA are written"
        );
        assert!(accs.iter().all(|a| !a.is_signer));
    }

    #[test]
    fn nonce_word_index_covers_256_nonces_per_word() {
        assert_eq!(nonce_word_index(0), 0);
        assert_eq!(nonce_word_index(255), 0);
        assert_eq!(nonce_word_index(256), 1);
        assert_eq!(nonce_word_index(u64::MAX), u64::MAX / 256);
    }

    #[test]
    fn init_nonce_bitmap_ix_matches_the_forwarder_idl() {
        let forwarder = Pubkey::new_unique();
        let payer = Pubkey::new_unique();
        let user = Pubkey::new_unique();
        let ix = init_nonce_bitmap_ix(&forwarder, &payer, &user, 7);

        assert_eq!(ix.program_id, forwarder);
        // Discriminator from idl/spl_token_forwarder.json, then the two args.
        assert_eq!(&ix.data[..8], &[214, 13, 125, 121, 72, 220, 241, 42]);
        assert_eq!(&ix.data[8..40], user.as_ref());
        assert_eq!(&ix.data[40..], &7u64.to_le_bytes());

        let expected_bitmap = derive_nonce_bitmap_pda(&forwarder, &user, 7).0;
        assert_eq!(
            ix.accounts,
            vec![
                AccountMeta::new(payer, true),
                AccountMeta::new(expected_bitmap, false),
                AccountMeta::new_readonly(system_program::id(), false),
            ]
        );
    }
}
