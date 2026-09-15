//! Program-derived-address helpers for the PA and the SPL Token Forwarder.
//!
//! These mirror the seed schemas baked into the on-chain programs. They are pure
//! functions: same inputs always produce the same `(Pubkey, bump)` pair.

use solana_program::pubkey::Pubkey;

use crate::constants::GROTH16_VERIFIER_SELECTOR;
use crate::program_ids::{ASSOCIATED_TOKEN_PROGRAM_ID, SPL_TOKEN_PROGRAM_ID};

// ---- PA program PDAs ---------------------------------------------------------

/// Derive the global PA state PDA. Seed: `["pa_state"]`.
pub fn derive_pa_state_pda(pa_program: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"pa_state"], pa_program)
}

/// Derive a per-authority+upload `tx_data` PDA. Seed: `["tx_data", authority, upload_id_le]`.
pub fn derive_tx_data_pda(pa_program: &Pubkey, authority: &Pubkey, upload_id: u64) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[b"tx_data", authority.as_ref(), &upload_id.to_le_bytes()],
        pa_program,
    )
}

/// Derive a nullifier marker PDA. Seed: `["nullifier", pa_state, nullifier_bytes]`.
pub fn derive_nullifier_pda(
    pa_program: &Pubkey,
    pa_state: &Pubkey,
    nullifier: &[u8; 32],
) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"nullifier", pa_state.as_ref(), nullifier], pa_program)
}

/// Derive a root marker PDA. Seed: `["root", pa_state, root_bytes]`.
pub fn derive_root_marker_pda(
    pa_program: &Pubkey,
    pa_state: &Pubkey,
    root: &[u8; 32],
) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"root", pa_state.as_ref(), root], pa_program)
}

/// Derive the PA's event authority PDA. Seed: `["__event_authority"]`.
///
/// Anchor's `#[event_cpi]` signs each event self-invocation with this PDA and
/// requires it, followed by the program's own address, as the last two named
/// accounts of `settle` and `settle_from_txdata`.
pub fn derive_event_authority_pda(pa_program: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"__event_authority"], pa_program)
}

// ---- Forwarder PDAs ----------------------------------------------------------

/// Derive the forwarder's global config PDA. Seed: `["config"]`.
pub fn derive_forwarder_config_pda(forwarder_program: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"config"], forwarder_program)
}

/// Derive the forwarder's escrow PDA for a given mint. Seed: `["escrow", mint]`.
///
/// This PDA is both the authority on the escrow's Associated Token Account and the
/// delegate users must name in their SPL `Approve` instruction before a wrap.
pub fn derive_forwarder_escrow_pda(
    forwarder_program: &Pubkey,
    token_mint: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"escrow", token_mint.as_ref()], forwarder_program)
}

/// Derive the forwarder's nonce bitmap PDA for a (user, word_index) pair.
///
/// Seed: `["nonce_bitmap", user, word_index_le]`. `word_index = nonce / 256`.
pub fn derive_nonce_bitmap_pda(
    forwarder_program: &Pubkey,
    user: &Pubkey,
    word_index: u64,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[b"nonce_bitmap", user.as_ref(), &word_index.to_le_bytes()],
        forwarder_program,
    )
}

// ---- SPL Associated Token Account --------------------------------------------

/// Derive the SPL Associated Token Account address for a wallet and mint.
///
/// Note: ATA derivation does *not* return the bump because the ATA program ignores
/// it during account creation. Only the address is consumed by integrators.
pub fn derive_associated_token_address(wallet: &Pubkey, token_mint: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[
            wallet.as_ref(),
            SPL_TOKEN_PROGRAM_ID.as_ref(),
            token_mint.as_ref(),
        ],
        &ASSOCIATED_TOKEN_PROGRAM_ID,
    )
    .0
}

// ---- Verifier router PDAs ----------------------------------------------------

/// Derive the verifier-router state PDA and the Groth16 verifier-entry PDA.
///
/// The router PDA holds the registry of verifier programs; the entry PDA points
/// to the specific verifier implementation matched by `GROTH16_VERIFIER_SELECTOR`.
pub fn derive_verifier_router_pdas(verifier_router_program: &Pubkey) -> (Pubkey, Pubkey) {
    let (router, _) = Pubkey::find_program_address(&[b"router"], verifier_router_program);
    let (entry, _) = Pubkey::find_program_address(
        &[b"verifier", &GROTH16_VERIFIER_SELECTOR],
        verifier_router_program,
    );
    (router, entry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::program_ids::{FORWARDER_PROGRAM_ID, PA_PROGRAM_ID};
    use std::str::FromStr;

    #[test]
    fn pa_state_pda_is_deterministic() {
        let (a, _) = derive_pa_state_pda(&PA_PROGRAM_ID);
        let (b, _) = derive_pa_state_pda(&PA_PROGRAM_ID);
        assert_eq!(a, b);
    }

    #[test]
    fn event_authority_pda_matches_web3js_for_devnet_v2() {
        // Independent pin: @solana/web3.js findProgramAddressSync with seed
        // "__event_authority" under the devnet V2 adapter. The same program's
        // pa_state PDA from that derivation matches docs/DEVNET_DEPLOYMENT.md.
        let pa = Pubkey::from_str("28Hvr1YFv2ouGN2fS99aF3ZzYXzkncJVVaHcZNhquLFT").unwrap();
        let (event_authority, bump) = derive_event_authority_pda(&pa);
        assert_eq!(
            event_authority.to_string(),
            "9G3rrSgAHcJCW75RFGXnZme7hNZNXmgiphDLFGbxpSbv"
        );
        assert_eq!(bump, 255);
    }

    #[test]
    fn forwarder_escrow_pda_is_per_mint() {
        let mint1 = Pubkey::new_unique();
        let mint2 = Pubkey::new_unique();
        let (e1, _) = derive_forwarder_escrow_pda(&FORWARDER_PROGRAM_ID, &mint1);
        let (e2, _) = derive_forwarder_escrow_pda(&FORWARDER_PROGRAM_ID, &mint2);
        assert_ne!(e1, e2);
    }

    #[test]
    fn ata_derivation_matches_pda_construction() {
        // Sanity-check that our ATA derivation matches the canonical seed order.
        let wallet = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let ata = derive_associated_token_address(&wallet, &mint);
        let (expected, _) = Pubkey::find_program_address(
            &[
                wallet.as_ref(),
                SPL_TOKEN_PROGRAM_ID.as_ref(),
                mint.as_ref(),
            ],
            &ASSOCIATED_TOKEN_PROGRAM_ID,
        );
        assert_eq!(ata, expected);
    }
}
