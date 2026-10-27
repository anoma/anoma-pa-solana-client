//! Program-derived-address helpers for the PA.
//!
//! These mirror the seed schemas baked into the on-chain programs. They are pure
//! functions: same inputs always produce the same `(Pubkey, bump)` pair.

use solana_pubkey::Pubkey;
use solana_sdk_ids::bpf_loader_upgradeable;

use crate::generated::protocol_adapter::constants::{NULLIFIER_SEED, ROOT_SEED};

// ---- PA program PDAs ---------------------------------------------------------
//
// The PDAs the adapter's instructions take as remaining accounts, from the
// seeds its IDL declares (`generated::protocol_adapter::constants`). Its
// state, upload and upgrade authority PDAs are derived from the IDL's
// `PA_STATE_SEED`, `TX_DATA_SEED` and `UPGRADE_AUTHORITY_SEED`.

/// Derive a nullifier marker PDA. Seed: `["nullifier", pa_state, nullifier_bytes]`.
pub fn derive_nullifier_pda(
    pa_program: &Pubkey,
    pa_state: &Pubkey,
    nullifier: &[u8; 32],
) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[NULLIFIER_SEED, pa_state.as_ref(), nullifier], pa_program)
}

/// Derive a root marker PDA. Seed: `["root", pa_state, root_bytes]`.
pub fn derive_root_marker_pda(
    pa_program: &Pubkey,
    pa_state: &Pubkey,
    root: &[u8; 32],
) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[ROOT_SEED, pa_state.as_ref(), root], pa_program)
}

/// Derive a program's event authority PDA. Seed: `["__event_authority"]`.
///
/// Anchor's `#[event_cpi]` signs each event self-invocation with this PDA and
/// requires it, followed by the program's own address, after an
/// instruction's other named accounts: for the PA, `settle` and
/// `settle_from_txdata`. Any program that emits events this way derives its
/// event authority with the same seed.
pub fn derive_event_authority_pda(program: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"__event_authority"], program)
}

// ---- Verifier router PDAs ----------------------------------------------------

/// Derive the verifier-router state PDA and the router's verifier-entry PDA
/// for `selector`.
///
/// The router PDA holds the registry of verifier programs; the entry PDA points
/// to the verifier registered under `selector`, the proof selector the adapter
/// was initialized with (`PAStateAccount::proof_selector`).
pub fn derive_verifier_router_pdas(
    verifier_router_program: &Pubkey,
    selector: [u8; 4],
) -> (Pubkey, Pubkey) {
    let (router, _) = Pubkey::find_program_address(&[b"router"], verifier_router_program);
    (
        router,
        derive_verifier_entry_pda(verifier_router_program, selector),
    )
}

/// The router's verifier-entry PDA for `selector`. Seed: `["verifier", selector]`.
pub fn derive_verifier_entry_pda(verifier_router_program: &Pubkey, selector: [u8; 4]) -> Pubkey {
    Pubkey::find_program_address(&[b"verifier", &selector], verifier_router_program).0
}

// ---- Upgradeable loader ------------------------------------------------------

/// The upgradeable loader's ProgramData account of `program`, where the loader
/// records its upgrade authority.
pub fn derive_program_data_address(program: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[program.as_ref()], &bpf_loader_upgradeable::id()).0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn event_authority_pda_matches_web3js_for_devnet_v2() {
        // Independent pin: @solana/web3.js findProgramAddressSync with seed
        // "__event_authority" under the program 5zeqkB3k….
        let pa = Pubkey::from_str("5zeqkB3kc9fd1RvaXB2GeMB53Jgf98QJtaFK38e6tTsc").unwrap();
        let (event_authority, bump) = derive_event_authority_pda(&pa);
        assert_eq!(
            event_authority.to_string(),
            "5ZycgCWUwuJzmVnvxtsTcb4C7Zjh8y66XcpPpwreZDRM"
        );
        assert_eq!(bump, 255);
    }

    #[test]
    fn the_verifier_entry_is_the_routers_entry_for_the_given_selector() {
        // An adapter initialized with the mock selector settles through the
        // router's entry for 0xffffffff, not the Groth16 one.
        let router_program = Pubkey::new_from_array([2; 32]);
        let selector = crate::MOCK_SELECTOR;
        let (router, entry) = derive_verifier_router_pdas(&router_program, selector);
        assert_eq!(
            router,
            Pubkey::find_program_address(&[b"router"], &router_program).0
        );
        assert_eq!(
            entry,
            Pubkey::find_program_address(&[b"verifier", &selector], &router_program).0
        );
    }
}
