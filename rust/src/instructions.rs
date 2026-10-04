//! Builders for the PA's `initialize`, `txdata_*` and `settle_from_txdata`
//! instructions.
//!
//! Each builder serializes the Anchor discriminator + arguments and lays out
//! the accounts in the order the PA program expects. The verifier-router
//! account fan-out (4 accounts) lives in `derive_verifier_router_pdas` in the
//! `pda` module.

use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;
use solana_sdk_ids::{bpf_loader_upgradeable, system_program};

use crate::discriminator::anchor_instruction_disc;
use crate::pda::{
    derive_event_authority_pda, derive_pa_state_pda, derive_program_data_address,
    derive_upgrade_authority_pda, derive_verifier_entry_pda,
};

/// Build the PA's `initialize`: `payer`, the program's upgrade authority, sets
/// the owner, verifier router and proof selector, and hands the upgrade
/// authority to the program.
pub fn initialize_ix(
    pa_program: &Pubkey,
    payer: &Pubkey,
    initial_owner: &Pubkey,
    verifier_router: &Pubkey,
    proof_selector: [u8; 4],
) -> Instruction {
    let mut data = anchor_instruction_disc("initialize").to_vec();
    data.extend_from_slice(initial_owner.as_ref());
    data.extend_from_slice(verifier_router.as_ref());
    data.extend_from_slice(&proof_selector);
    Instruction {
        program_id: *pa_program,
        accounts: vec![
            AccountMeta::new(derive_pa_state_pda(pa_program).0, false),
            AccountMeta::new(*payer, true),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new(derive_program_data_address(pa_program), false),
            AccountMeta::new_readonly(derive_upgrade_authority_pda(pa_program).0, false),
            AccountMeta::new_readonly(bpf_loader_upgradeable::id(), false),
            AccountMeta::new_readonly(
                derive_verifier_entry_pda(verifier_router, proof_selector),
                false,
            ),
            AccountMeta::new_readonly(derive_event_authority_pda(pa_program).0, false),
            AccountMeta::new_readonly(*pa_program, false),
        ],
        data,
    }
}

/// Build the PA's `pause`: the owner `authority` stops settlement, the
/// emergency stop a forwarder's committee instructions wait for.
pub fn pause_ix(pa_program: &Pubkey, authority: &Pubkey) -> Instruction {
    Instruction {
        program_id: *pa_program,
        accounts: vec![
            AccountMeta::new(derive_pa_state_pda(pa_program).0, false),
            AccountMeta::new_readonly(*authority, true),
            AccountMeta::new_readonly(derive_event_authority_pda(pa_program).0, false),
            AccountMeta::new_readonly(*pa_program, false),
        ],
        data: anchor_instruction_disc("pause").to_vec(),
    }
}

/// Build the PA's `set_kind_table_commitment`: the owner `authority` replaces
/// the kind-table commitment settled transactions must be proven against.
pub fn set_kind_table_commitment_ix(
    pa_program: &Pubkey,
    authority: &Pubkey,
    new_kind_table_commitment: [u8; 32],
) -> Instruction {
    let mut data = anchor_instruction_disc("set_kind_table_commitment").to_vec();
    data.extend_from_slice(&new_kind_table_commitment);
    Instruction {
        program_id: *pa_program,
        accounts: vec![
            AccountMeta::new(derive_pa_state_pda(pa_program).0, false),
            AccountMeta::new_readonly(*authority, true),
            AccountMeta::new_readonly(derive_event_authority_pda(pa_program).0, false),
            AccountMeta::new_readonly(*pa_program, false),
        ],
        data,
    }
}

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
/// new-root marker is NOT part of it: it is an optional named account,
/// `new_root_marker` — the marker PDA of the post-settlement root for a
/// settlement that creates commitments, and `None` for one that creates
/// nothing (it produces no root). Anchor marks an absent optional account by
/// the program's own address in its slot.
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
    new_root_marker: Option<&Pubkey>,
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
        match new_root_marker {
            Some(marker) => AccountMeta::new(*marker, false),
            None => AccountMeta::new_readonly(*pa_program, false),
        },
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
            Some(&keys[4]),
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
            Some(&Pubkey::new_unique()),
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
    fn an_absent_new_root_marker_is_the_program_address() {
        // A settlement that creates nothing omits the optional marker;
        // Anchor reads the program's own address in its slot as "absent".
        let pa = Pubkey::new_unique();
        let ix = settle_from_txdata_ix(
            &pa,
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            1,
            None,
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            vec![],
        );
        assert_eq!(ix.accounts[4], AccountMeta::new_readonly(pa, false));
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

    /// Checks `ix` against the adapter IDL's instruction `name`: the data is
    /// its discriminator then `args`, and each account has the IDL's flags,
    /// its fixed address, or its PDA from the IDL's own constant seeds.
    /// Returns the address `ix` passes for each IDL account name.
    fn assert_matches_the_adapters_idl(
        ix: &Instruction,
        name: &str,
        args: &[u8],
    ) -> impl Fn(&str) -> Pubkey {
        let idl: serde_json::Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../idl/protocol_adapter.json"
        )))
        .unwrap();
        let spec = idl["instructions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|spec| spec["name"] == name)
            .unwrap()
            .clone();

        let mut data: Vec<u8> = serde_json::from_value(spec["discriminator"].clone()).unwrap();
        data.extend(args);
        assert_eq!(
            ix.data, data,
            "{name}: the discriminator, then the arguments"
        );

        let accounts = spec["accounts"].as_array().unwrap().clone();
        assert_eq!(ix.accounts.len(), accounts.len());
        for (meta, account) in ix.accounts.iter().zip(&accounts) {
            let account_name = account["name"].as_str().unwrap();
            assert_eq!(
                meta.is_writable,
                account["writable"] == true,
                "{account_name} writable"
            );
            assert_eq!(
                meta.is_signer,
                account["signer"] == true,
                "{account_name} signer"
            );
            if let Some(address) = account["address"].as_str() {
                assert_eq!(meta.pubkey.to_string(), address, "{account_name} address");
            }
            if let Some(seeds) = account["pda"]["seeds"].as_array() {
                let seeds: Vec<Vec<u8>> = seeds
                    .iter()
                    .map(|s| serde_json::from_value(s["value"].clone()).unwrap())
                    .collect();
                let seeds: Vec<&[u8]> = seeds.iter().map(Vec::as_slice).collect();
                assert_eq!(
                    meta.pubkey,
                    Pubkey::find_program_address(&seeds, &ix.program_id).0,
                    "{account_name} PDA"
                );
            }
        }
        let addresses = ix
            .accounts
            .iter()
            .map(|meta| meta.pubkey)
            .collect::<Vec<_>>();
        move |name: &str| {
            let i = accounts.iter().position(|a| a["name"] == name).unwrap();
            addresses[i]
        }
    }

    #[test]
    fn initialize_matches_the_adapters_idl() {
        let pa = crate::program_ids::PA_PROGRAM_ID;
        let (payer, owner, router) = (
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
        );
        let selector = [0xff; 4];
        let ix = initialize_ix(&pa, &payer, &owner, &router, selector);

        let args = [owner.to_bytes().as_slice(), &router.to_bytes(), &selector].concat();
        let by_name = assert_matches_the_adapters_idl(&ix, "initialize", &args);
        assert_eq!(by_name("payer"), payer);
        assert_eq!(
            by_name("verifier_entry"),
            derive_verifier_entry_pda(&router, selector)
        );
        assert_eq!(
            by_name("event_authority"),
            derive_event_authority_pda(&pa).0
        );
        assert_eq!(by_name("program"), pa);
    }

    #[test]
    fn pause_matches_the_adapters_idl() {
        let pa = crate::program_ids::PA_PROGRAM_ID;
        let authority = Pubkey::new_unique();
        let ix = pause_ix(&pa, &authority);

        let by_name = assert_matches_the_adapters_idl(&ix, "pause", &[]);
        assert_eq!(by_name("authority"), authority);
        assert_eq!(
            by_name("event_authority"),
            derive_event_authority_pda(&pa).0
        );
        assert_eq!(by_name("program"), pa);
    }

    #[test]
    fn set_kind_table_commitment_matches_the_adapters_idl() {
        let pa = crate::program_ids::PA_PROGRAM_ID;
        let authority = Pubkey::new_unique();
        let commitment = [7; 32];
        let ix = set_kind_table_commitment_ix(&pa, &authority, commitment);

        let by_name =
            assert_matches_the_adapters_idl(&ix, "set_kind_table_commitment", &commitment);
        assert_eq!(by_name("authority"), authority);
        assert_eq!(
            by_name("event_authority"),
            derive_event_authority_pda(&pa).0
        );
        assert_eq!(by_name("program"), pa);
    }
}
