//! Program-derived-address helpers for the PA.
//!
//! These mirror the seed schemas baked into the on-chain programs. They are pure
//! functions: same inputs always produce the same `(Pubkey, bump)` pair.

use solana_pubkey::Pubkey;
use solana_sdk_ids::bpf_loader_upgradeable;

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

// ---- Upgrade authority -------------------------------------------------------

/// The PDA the adapter's `initialize` makes the program's upgrade authority.
/// Seed: `["upgrade_authority"]`.
pub fn derive_upgrade_authority_pda(pa_program: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"upgrade_authority"], pa_program)
}

/// The upgradeable loader's ProgramData account of `program`, where the loader
/// records its upgrade authority.
pub fn derive_program_data_address(program: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[program.as_ref()], &bpf_loader_upgradeable::id()).0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::program_ids::PA_PROGRAM_ID;
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
        let selector = [0xff; 4];
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
