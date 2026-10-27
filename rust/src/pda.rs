//! Program-derived-address helpers for the PA.
//!
//! These mirror the seed schemas baked into the on-chain programs. They are pure
//! functions: same inputs always produce the same `(Pubkey, bump)` pair.

use solana_pubkey::Pubkey;
use solana_sdk_ids::bpf_loader_upgradeable;

use crate::generated::protocol_adapter::constants::{
    NULLIFIER_SEED, PA_STATE_SEED, ROOT_SEED, TX_DATA_SEED, UPGRADE_AUTHORITY_SEED,
};

// ---- PA program PDAs ---------------------------------------------------------
//
// From the seeds the adapter's IDL declares
// (`generated::protocol_adapter::constants`): the generated client takes every
// account as an address and derives none.

/// Derive the global PA state PDA. Seed: `[PA_STATE_SEED]`.
pub fn derive_pa_state_pda(pa_program: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[PA_STATE_SEED], pa_program)
}

/// Derive `authority`'s upload `upload_id`. Seed: `[TX_DATA_SEED, authority,
/// upload_id little-endian]`.
pub fn derive_tx_data_pda(pa_program: &Pubkey, authority: &Pubkey, upload_id: u64) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[TX_DATA_SEED, authority.as_ref(), &upload_id.to_le_bytes()],
        pa_program,
    )
}

/// The PDA the adapter's `initialize` makes the program's upgrade authority.
/// Seed: `[UPGRADE_AUTHORITY_SEED]`.
pub fn derive_upgrade_authority_pda(pa_program: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[UPGRADE_AUTHORITY_SEED], pa_program)
}

/// Derive a nullifier marker PDA, a settlement's remaining account. Seed:
/// `[NULLIFIER_SEED, pa_state, nullifier_bytes]`.
pub fn derive_nullifier_pda(
    pa_program: &Pubkey,
    pa_state: &Pubkey,
    nullifier: &[u8; 32],
) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[NULLIFIER_SEED, pa_state.as_ref(), nullifier], pa_program)
}

/// Derive a root marker PDA, a settlement's remaining account. Seed:
/// `[ROOT_SEED, pa_state, root_bytes]`.
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
///
/// The router's IDL declares its seeds only in its instructions' accounts,
/// not as constants.
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
    use serde_json::Value;
    use std::str::FromStr;

    fn idl(file: &str) -> Value {
        let path = format!("{}/idls/{file}", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()
    }

    /// The address the IDL's `pda.seeds` give `account` of `instruction`
    /// under `program`: const seeds as the IDL gives them, `account` seeds
    /// the address `accounts` names for that path, `arg` seeds the value
    /// `args` names for the instruction arg the path resolves to, encoded as
    /// that arg's IDL type is on the wire (integers little-endian, a `pubkey`
    /// given in base58 as its 32 bytes).
    ///
    /// An `arg` seed's path is a name in the accounts struct's
    /// `#[instruction(...)]`, which Anchor decodes positionally from the
    /// instruction data, while the IDL names each arg after the handler's
    /// parameter. The two differ only when the handler leaves the parameter
    /// unused, which Rust spells with a leading `_` (`txdata_close_expired`'s
    /// `_authority` is its accounts' `authority`), so the path resolves to the
    /// arg of its own name, or else to the arg of that name with a leading `_`.
    fn idl_pda(
        idl: &Value,
        instruction: &str,
        account: &str,
        program: &Pubkey,
        accounts: &[(&str, Pubkey)],
        args: &[(&str, Value)],
    ) -> Pubkey {
        let ix = idl["instructions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|ix| ix["name"] == instruction)
            .unwrap_or_else(|| panic!("the IDL has no instruction {instruction}"));
        let pda = &ix["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["name"] == account)
            .unwrap_or_else(|| panic!("{instruction} has no account {account}"))["pda"];
        assert!(
            pda.get("program").is_none(),
            "{account}: a PDA of another program"
        );
        let seeds: Vec<Vec<u8>> = pda["seeds"]
            .as_array()
            .unwrap_or_else(|| panic!("{instruction}.{account} is not a PDA"))
            .iter()
            .map(|seed| {
                let path = seed["path"].as_str();
                match seed["kind"].as_str().unwrap() {
                    "const" => serde_json::from_value(seed["value"].clone()).unwrap(),
                    "account" => {
                        let path = path.unwrap();
                        let (_, key) = accounts
                            .iter()
                            .find(|(name, _)| *name == path)
                            .unwrap_or_else(|| panic!("no address given for {path}"));
                        key.to_bytes().to_vec()
                    }
                    "arg" => {
                        let path = path.unwrap();
                        let ix_args = ix["args"].as_array().unwrap();
                        let unused = format!("_{path}");
                        let arg = ix_args
                            .iter()
                            .find(|a| a["name"] == path)
                            .or_else(|| ix_args.iter().find(|a| a["name"] == unused.as_str()))
                            .unwrap_or_else(|| {
                                panic!("{instruction} has no arg {path} or {unused}")
                            });
                        let (name, ty) = (arg["name"].as_str().unwrap(), &arg["type"]);
                        let (_, value) = args
                            .iter()
                            .find(|(given, _)| *given == name)
                            .unwrap_or_else(|| panic!("no value given for {name}"));
                        let int = || value.as_u64().unwrap();
                        match ty.as_str() {
                            Some("u8") => vec![u8::try_from(int()).unwrap()],
                            Some("u32") => u32::try_from(int()).unwrap().to_le_bytes().to_vec(),
                            Some("u64") => int().to_le_bytes().to_vec(),
                            Some("pubkey") => Pubkey::from_str(value.as_str().unwrap())
                                .unwrap()
                                .to_bytes()
                                .to_vec(),
                            _ if ty["array"][0] == "u8" => {
                                serde_json::from_value(value.clone()).unwrap()
                            }
                            _ => panic!("no encoding for the IDL arg type {ty}"),
                        }
                    }
                    kind => panic!("no evaluation for a {kind} seed"),
                }
            })
            .collect();
        let seeds: Vec<&[u8]> = seeds.iter().map(Vec::as_slice).collect();
        Pubkey::find_program_address(&seeds, program).0
    }

    #[test]
    fn the_adapter_pdas_are_the_ones_its_idl_declares() {
        let idl = idl("protocol_adapter.json");
        let pa = Pubkey::new_from_array([9; 32]);
        let authority = Pubkey::new_from_array([1; 32]);
        let upload_id = 0x0102_0304_0506_0708u64;
        assert_eq!(
            derive_pa_state_pda(&pa).0,
            idl_pda(&idl, "initialize", "pa_state", &pa, &[], &[])
        );
        assert_eq!(
            derive_tx_data_pda(&pa, &authority, upload_id).0,
            idl_pda(
                &idl,
                "txdata_init",
                "tx_data",
                &pa,
                &[("authority", authority)],
                &[("upload_id", upload_id.into())],
            )
        );
        assert_eq!(
            derive_tx_data_pda(&pa, &authority, upload_id).0,
            idl_pda(
                &idl,
                "txdata_close_expired",
                "tx_data",
                &pa,
                &[],
                &[
                    ("upload_id", upload_id.into()),
                    ("_authority", authority.to_string().into()),
                ],
            )
        );
        assert_eq!(
            derive_upgrade_authority_pda(&pa).0,
            idl_pda(&idl, "upgrade", "upgrade_authority", &pa, &[], &[])
        );
    }

    #[test]
    fn the_router_pdas_are_the_ones_its_idl_declares() {
        let idl = idl("verifier_router.json");
        let router_program = Pubkey::new_from_array([2; 32]);
        let selector = crate::MOCK_SELECTOR;
        let (router, entry) = derive_verifier_router_pdas(&router_program, selector);
        assert_eq!(
            router,
            idl_pda(&idl, "add_verifier", "router", &router_program, &[], &[])
        );
        assert_eq!(
            entry,
            idl_pda(
                &idl,
                "add_verifier",
                "verifier_entry",
                &router_program,
                &[],
                &[("selector", selector.to_vec().into())],
            )
        );
    }

    #[test]
    fn event_authority_pda_matches_web3js() {
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
}
