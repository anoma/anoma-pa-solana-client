//! Builders for the adapter's instructions, over the generated
//! `client::accounts` and `client::args` of `generated::protocol_adapter`.
//! Each takes the address the adapter is deployed at, which its build sets
//! (`declare_id!` from the environment), so one IDL serves every deployment.

use anchor::{InstructionData, ToAccountMetas};
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;
use solana_sdk_ids::{bpf_loader_upgradeable, system_program, sysvar};

use crate::generated::protocol_adapter::client::{accounts, args};
use crate::generated::protocol_adapter::types::DeniedLogicRef;
use crate::generated::protocol_adapter::ID;
use crate::pda::{
    derive_event_authority_pda, derive_pa_state_pda, derive_program_data_address,
    derive_tx_data_pda, derive_upgrade_authority_pda, derive_verifier_entry_pda,
};

/// The instruction of the adapter at `pa_program` with `accounts`, then
/// `remaining_accounts`, and `args`.
///
/// The generated client marks an absent optional account by the IDL's
/// address, `ID`; Anchor reads an optional account as absent when it is the
/// address of the program it runs, `pa_program`. No other account is `ID`:
/// every builder names the program itself as `pa_program`.
fn instruction(
    pa_program: &Pubkey,
    accounts: impl ToAccountMetas,
    remaining_accounts: Vec<AccountMeta>,
    args: impl InstructionData,
) -> Instruction {
    let mut metas = accounts.to_account_metas(None);
    for meta in &mut metas {
        if meta.pubkey == ID {
            meta.pubkey = *pa_program;
        }
    }
    metas.extend(remaining_accounts);
    Instruction {
        program_id: *pa_program,
        accounts: metas,
        data: args.data(),
    }
}

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
    instruction(
        pa_program,
        accounts::Initialize {
            pa_state: derive_pa_state_pda(pa_program).0,
            payer: *payer,
            system_program: system_program::id(),
            program_data: derive_program_data_address(pa_program),
            upgrade_authority: derive_upgrade_authority_pda(pa_program).0,
            bpf_loader_upgradeable: bpf_loader_upgradeable::id(),
            verifier_entry: derive_verifier_entry_pda(verifier_router, proof_selector),
            event_authority: derive_event_authority_pda(pa_program).0,
            program: *pa_program,
        },
        vec![],
        args::Initialize {
            initial_owner: *initial_owner,
            verifier_router: *verifier_router,
            proof_selector,
        },
    )
}

/// Build the PA's `pause`: the owner `authority` stops settlement until it
/// unpauses.
pub fn pause_ix(pa_program: &Pubkey, authority: &Pubkey) -> Instruction {
    instruction(
        pa_program,
        accounts::Pause {
            pa_state: derive_pa_state_pda(pa_program).0,
            authority: *authority,
            event_authority: derive_event_authority_pda(pa_program).0,
            program: *pa_program,
        },
        vec![],
        args::Pause {},
    )
}

/// Build the PA's `unpause`: the owner `authority` resumes settlement.
pub fn unpause_ix(pa_program: &Pubkey, authority: &Pubkey) -> Instruction {
    instruction(
        pa_program,
        accounts::Unpause {
            pa_state: derive_pa_state_pda(pa_program).0,
            authority: *authority,
            event_authority: derive_event_authority_pda(pa_program).0,
            program: *pa_program,
        },
        vec![],
        args::Unpause {},
    )
}

/// Build the PA's `deny_logic_refs`: the owner `authority` adds each entry
/// to its denylist and pays for the entries. A logic ref on the created side
/// only is deprecated (its resources are still consumed); one on both is
/// denied.
pub fn deny_logic_refs_ix(
    pa_program: &Pubkey,
    authority: &Pubkey,
    logic_refs: &[DeniedLogicRef],
) -> Instruction {
    instruction(
        pa_program,
        accounts::DenyLogicRefs {
            pa_state: derive_pa_state_pda(pa_program).0,
            authority: *authority,
            system_program: system_program::id(),
            event_authority: derive_event_authority_pda(pa_program).0,
            program: *pa_program,
        },
        vec![],
        args::DenyLogicRefs {
            logic_refs: logic_refs.to_vec(),
        },
    )
}

/// Build the PA's `set_kind_tree_root`: the owner `authority` replaces
/// the kind tree root settled transactions must be proven against.
pub fn set_kind_tree_root_ix(
    pa_program: &Pubkey,
    authority: &Pubkey,
    new_kind_tree_root: [u8; 32],
) -> Instruction {
    instruction(
        pa_program,
        accounts::SetKindTreeRoot {
            pa_state: derive_pa_state_pda(pa_program).0,
            authority: *authority,
            event_authority: derive_event_authority_pda(pa_program).0,
            program: *pa_program,
        },
        vec![],
        args::SetKindTreeRoot { new_kind_tree_root },
    )
}

/// Build the PA's `transfer_ownership`: the owner `authority` hands the
/// ownership to `new_owner`.
pub fn transfer_ownership_ix(
    pa_program: &Pubkey,
    authority: &Pubkey,
    new_owner: &Pubkey,
) -> Instruction {
    instruction(
        pa_program,
        accounts::TransferOwnership {
            pa_state: derive_pa_state_pda(pa_program).0,
            authority: *authority,
            event_authority: derive_event_authority_pda(pa_program).0,
            program: *pa_program,
        },
        vec![],
        args::TransferOwnership {
            new_owner: *new_owner,
        },
    )
}

/// Build the PA's `renounce_ownership`: the owner `authority` gives up the
/// ownership for good.
pub fn renounce_ownership_ix(pa_program: &Pubkey, authority: &Pubkey) -> Instruction {
    instruction(
        pa_program,
        accounts::RenounceOwnership {
            pa_state: derive_pa_state_pda(pa_program).0,
            authority: *authority,
            event_authority: derive_event_authority_pda(pa_program).0,
            program: *pa_program,
        },
        vec![],
        args::RenounceOwnership {},
    )
}

/// Build the PA's `update_expiry_config`: the owner `authority` sets the
/// bounds of an upload's expiry.
pub fn update_expiry_config_ix(
    pa_program: &Pubkey,
    authority: &Pubkey,
    new_min_expiry_slots: u64,
    new_max_expiry_slots: u64,
) -> Instruction {
    instruction(
        pa_program,
        accounts::UpdateExpiryConfig {
            pa_state: derive_pa_state_pda(pa_program).0,
            authority: *authority,
        },
        vec![],
        args::UpdateExpiryConfig {
            new_min_expiry_slots,
            new_max_expiry_slots,
        },
    )
}

/// Build the PA's `upgrade`: the owner `authority` replaces the program's
/// code with `buffer`'s, whose rent goes to `spill`.
pub fn upgrade_ix(
    pa_program: &Pubkey,
    authority: &Pubkey,
    buffer: &Pubkey,
    spill: &Pubkey,
) -> Instruction {
    instruction(
        pa_program,
        accounts::Upgrade {
            pa_state: derive_pa_state_pda(pa_program).0,
            authority: *authority,
            program_data: derive_program_data_address(pa_program),
            upgraded_program: *pa_program,
            buffer: *buffer,
            spill: *spill,
            upgrade_authority: derive_upgrade_authority_pda(pa_program).0,
            rent: sysvar::rent::id(),
            clock: sysvar::clock::id(),
            bpf_loader_upgradeable: bpf_loader_upgradeable::id(),
            event_authority: derive_event_authority_pda(pa_program).0,
            program: *pa_program,
        },
        vec![],
        args::Upgrade {},
    )
}

/// Build the PA's `risc_zero_verifier_paused`, which returns whether the
/// router has paused the verifier `verifier_entry`, the router's entry for
/// the adapter's selector, names.
pub fn risc_zero_verifier_paused_ix(pa_program: &Pubkey, verifier_entry: &Pubkey) -> Instruction {
    instruction(
        pa_program,
        accounts::RiscZeroVerifierPaused {
            pa_state: derive_pa_state_pda(pa_program).0,
            verifier_entry: *verifier_entry,
        },
        vec![],
        args::RiscZeroVerifierPaused {},
    )
}

/// Build the PA's `version`, which returns the program's version.
pub fn version_ix(pa_program: &Pubkey) -> Instruction {
    instruction(pa_program, accounts::Version {}, vec![], args::Version {})
}

/// Build a PA `txdata_init` instruction.
pub fn txdata_init_ix(
    pa_program: &Pubkey,
    authority: &Pubkey,
    upload_id: u64,
    capacity: u32,
    expires_slot: u64,
) -> Instruction {
    instruction(
        pa_program,
        accounts::TxdataInit {
            pa_state: derive_pa_state_pda(pa_program).0,
            tx_data: derive_tx_data_pda(pa_program, authority, upload_id).0,
            authority: *authority,
            system_program: system_program::id(),
        },
        vec![],
        args::TxdataInit {
            upload_id,
            capacity,
            expires_slot,
        },
    )
}

/// Build a PA `txdata_write` instruction for a single chunk.
///
/// `chunk.len()` is bounded by `TXDATA_CHUNK_SIZE`, the largest chunk whose
/// write transaction fits a v1 transaction.
pub fn txdata_write_ix(
    pa_program: &Pubkey,
    authority: &Pubkey,
    upload_id: u64,
    offset: u32,
    chunk: &[u8],
) -> Instruction {
    instruction(
        pa_program,
        accounts::TxdataWrite {
            tx_data: derive_tx_data_pda(pa_program, authority, upload_id).0,
            authority: *authority,
        },
        vec![],
        args::TxdataWrite {
            upload_id,
            offset,
            data: chunk.to_vec(),
        },
    )
}

/// Build a PA `txdata_close` instruction (reclaims rent from the upload PDA).
pub fn txdata_close_ix(
    pa_program: &Pubkey,
    authority: &Pubkey,
    refund: &Pubkey,
    upload_id: u64,
) -> Instruction {
    instruction(
        pa_program,
        accounts::TxdataClose {
            tx_data: derive_tx_data_pda(pa_program, authority, upload_id).0,
            authority: *authority,
            refund: *refund,
        },
        vec![],
        args::TxdataClose { upload_id },
    )
}

/// Build a PA `txdata_extend` instruction: `authority` moves its upload's
/// expiry to `new_expires_slot`.
pub fn txdata_extend_ix(
    pa_program: &Pubkey,
    authority: &Pubkey,
    upload_id: u64,
    new_expires_slot: u64,
) -> Instruction {
    instruction(
        pa_program,
        accounts::TxdataExtend {
            pa_state: derive_pa_state_pda(pa_program).0,
            tx_data: derive_tx_data_pda(pa_program, authority, upload_id).0,
            authority: *authority,
        },
        vec![],
        args::TxdataExtend {
            upload_id,
            new_expires_slot,
        },
    )
}

/// Build a PA `txdata_close_expired` instruction: anyone (`payer`) closes
/// `authority`'s expired upload `upload_id`, its rent going to `refund`.
pub fn txdata_close_expired_ix(
    pa_program: &Pubkey,
    payer: &Pubkey,
    refund: &Pubkey,
    upload_id: u64,
    authority: &Pubkey,
) -> Instruction {
    instruction(
        pa_program,
        accounts::TxdataCloseExpired {
            tx_data: derive_tx_data_pda(pa_program, authority, upload_id).0,
            payer: *payer,
            refund: *refund,
        },
        vec![],
        args::TxdataCloseExpired {
            upload_id,
            authority: *authority,
        },
    )
}

/// The accounts every settlement instruction (`settle`, `simulate_execute`,
/// `settle_from_txdata`) names besides its payer and its upload, then its
/// remaining accounts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettlementAccounts {
    /// The marker PDA of the root a settlement that creates commitments
    /// produces; `None` for one that creates nothing, which produces no root.
    pub new_root_marker: Option<Pubkey>,
    pub verifier_router_program: Pubkey,
    pub router: Pubkey,
    pub verifier_entry: Pubkey,
    pub verifier_program: Pubkey,
    /// The nullifier PDAs, each external call's CPI segment, and the
    /// historical root markers.
    pub remaining_accounts: Vec<AccountMeta>,
}

/// Build a PA `settle` instruction, which carries the settlement input
/// itself, paid for by `payer`.
pub fn settle_ix(
    pa_program: &Pubkey,
    payer: &Pubkey,
    input: &[u8],
    settlement: SettlementAccounts,
) -> Instruction {
    instruction(
        pa_program,
        accounts::Settle {
            pa_state: derive_pa_state_pda(pa_program).0,
            payer: *payer,
            system_program: system_program::id(),
            new_root_marker: settlement.new_root_marker,
            verifier_router_program: settlement.verifier_router_program,
            router: settlement.router,
            verifier_entry: settlement.verifier_entry,
            verifier_program: settlement.verifier_program,
            event_authority: derive_event_authority_pda(pa_program).0,
            program: *pa_program,
        },
        settlement.remaining_accounts,
        args::Settle {
            transaction_data: input.to_vec(),
        },
    )
}

/// Build a PA `simulate_execute` instruction (pa-evm's `simulateExecute`):
/// `settle_ix`'s accounts and input, and whether to skip verifying the RISC
/// Zero proof. It always fails: with `Simulated` when the settlement would
/// execute, otherwise with the error `settle` would.
pub fn simulate_execute_ix(
    pa_program: &Pubkey,
    payer: &Pubkey,
    input: &[u8],
    skip_risc_zero_proof_verification: bool,
    settlement: SettlementAccounts,
) -> Instruction {
    instruction(
        pa_program,
        accounts::SimulateExecute {
            pa_state: derive_pa_state_pda(pa_program).0,
            payer: *payer,
            system_program: system_program::id(),
            new_root_marker: settlement.new_root_marker,
            verifier_router_program: settlement.verifier_router_program,
            router: settlement.router,
            verifier_entry: settlement.verifier_entry,
            verifier_program: settlement.verifier_program,
            event_authority: derive_event_authority_pda(pa_program).0,
            program: *pa_program,
        },
        settlement.remaining_accounts,
        args::SimulateExecute {
            transaction_data: input.to_vec(),
            skip_risc_zero_proof_verification,
        },
    )
}

/// Build a PA `settle_from_txdata` instruction, which settles the input
/// `authority` uploaded to `tx_data` under `upload_id`.
pub fn settle_from_txdata_ix(
    pa_program: &Pubkey,
    authority: &Pubkey,
    upload_id: u64,
    settlement: SettlementAccounts,
) -> Instruction {
    instruction(
        pa_program,
        accounts::SettleFromTxdata {
            pa_state: derive_pa_state_pda(pa_program).0,
            tx_data: derive_tx_data_pda(pa_program, authority, upload_id).0,
            authority: *authority,
            system_program: system_program::id(),
            new_root_marker: settlement.new_root_marker,
            verifier_router_program: settlement.verifier_router_program,
            router: settlement.router,
            verifier_entry: settlement.verifier_entry,
            verifier_program: settlement.verifier_program,
            event_authority: derive_event_authority_pda(pa_program).0,
            program: *pa_program,
        },
        settlement.remaining_accounts,
        args::SettleFromTxdata { upload_id },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(n: u8) -> Pubkey {
        Pubkey::new_from_array([n; 32])
    }

    /// The entry `name` of `fixtures/instructions_fixture.json`, the
    /// instructions `tests/instructions_fixture.rs` builds for these tests'
    /// inputs (the program `key(9)`, other accounts `key(n)`).
    fn expected(name: &str) -> serde_json::Value {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../fixtures/instructions_fixture.json"
        );
        let fixture: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path}: {e}")),
        )
        .expect("the instructions fixture is JSON");
        fixture["instructions"]
            .as_array()
            .expect("the fixture lists instructions")
            .iter()
            .find(|e| e["name"] == name)
            .unwrap_or_else(|| panic!("the instructions fixture has no {name}"))
            .clone()
    }

    /// `ix` is the instructions fixture's entry `name`: its program, its data
    /// in hex, and its accounts (address, signer, writable), in order.
    fn assert_is_expected(name: &str, ix: &Instruction) {
        let built = serde_json::json!({
            "name": name,
            "program": ix.program_id.to_string(),
            "data_hex": ix.data.iter().map(|b| format!("{b:02x}")).collect::<String>(),
            "accounts": ix.accounts.iter().map(|m| serde_json::json!({
                "address": m.pubkey.to_string(),
                "signer": m.is_signer,
                "writable": m.is_writable,
            })).collect::<Vec<_>>(),
        });
        println!("{built:#}");
        assert_eq!(built, expected(name), "{name}");
    }

    /// The settlement accounts of the fixture's settlements of `key(9)`.
    fn settlement(new_root_marker: Option<Pubkey>, remaining: bool) -> SettlementAccounts {
        SettlementAccounts {
            new_root_marker,
            verifier_router_program: key(11),
            router: key(12),
            verifier_entry: key(13),
            verifier_program: key(14),
            remaining_accounts: if remaining {
                vec![
                    AccountMeta::new(key(20), false),
                    AccountMeta::new_readonly(key(21), false),
                ]
            } else {
                vec![]
            },
        }
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
            "/idls/protocol_adapter.json"
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
        assert!(ix.accounts.len() >= accounts.len(), "{name}: accounts");
        for (meta, account) in ix.accounts.iter().zip(&accounts) {
            let account_name = account["name"].as_str().unwrap();
            if account["optional"] == true && meta.pubkey == ix.program_id {
                continue;
            }
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
                if let Some(seeds) = seeds
                    .iter()
                    .map(|s| serde_json::from_value::<Vec<u8>>(s["value"].clone()).ok())
                    .collect::<Option<Vec<Vec<u8>>>>()
                {
                    let seeds: Vec<&[u8]> = seeds.iter().map(Vec::as_slice).collect();
                    assert_eq!(
                        meta.pubkey,
                        Pubkey::find_program_address(&seeds, &ix.program_id).0,
                        "{account_name} PDA"
                    );
                }
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
        assert_is_expected(
            "initialize",
            &initialize_ix(&key(9), &key(1), &key(2), &key(3), crate::MOCK_SELECTOR),
        );

        let (payer, owner, router) = (key(1), key(2), key(3));
        let selector = crate::MOCK_SELECTOR;
        let ix = initialize_ix(&ID, &payer, &owner, &router, selector);
        let args = [owner.to_bytes().as_slice(), &router.to_bytes(), &selector].concat();
        let by_name = assert_matches_the_adapters_idl(&ix, "initialize", &args);
        assert_eq!(by_name("payer"), payer);
        assert_eq!(
            by_name("verifier_entry"),
            derive_verifier_entry_pda(&router, selector)
        );
        assert_eq!(
            by_name("event_authority"),
            derive_event_authority_pda(&ID).0
        );
        assert_eq!(by_name("program"), ID);
    }

    #[test]
    fn the_inline_settlement_instructions_match_the_adapters_idl() {
        let pa = key(9);
        assert_is_expected(
            "settle",
            &settle_ix(&pa, &key(1), b"input", settlement(Some(key(10)), true)),
        );
        // An absent new-root marker is the program's own address.
        assert_is_expected(
            "settle_no_marker",
            &settle_ix(&pa, &key(1), b"input", settlement(None, false)),
        );
        assert_is_expected(
            "simulate_execute_false",
            &simulate_execute_ix(
                &pa,
                &key(1),
                b"input",
                false,
                settlement(Some(key(10)), true),
            ),
        );
        assert_is_expected(
            "simulate_execute_true",
            &simulate_execute_ix(&pa, &key(1), b"input", true, settlement(None, false)),
        );

        let accounts = settlement(Some(key(10)), false);
        let input = b"input";
        let encoded_input = [&5u32.to_le_bytes()[..], input].concat();
        let settle = settle_ix(&ID, &key(1), input, accounts.clone());
        for skip in [false, true] {
            let simulate = simulate_execute_ix(&ID, &key(1), input, skip, accounts.clone());
            let args = [&encoded_input[..], &[u8::from(skip)]].concat();
            let by_name = assert_matches_the_adapters_idl(&simulate, "simulate_execute", &args);
            assert_eq!(by_name("payer"), key(1));
            assert_eq!(by_name("new_root_marker"), key(10));
            assert_eq!(by_name("verifier_program"), key(14));
            // The same accounts as `settle`.
            assert_eq!(simulate.accounts, settle.accounts);
        }
        let by_name = assert_matches_the_adapters_idl(&settle, "settle", &encoded_input);
        assert_eq!(by_name("new_root_marker"), key(10));
    }

    #[test]
    fn the_upload_instructions_match_the_adapters_idl() {
        let pa = key(9);
        let tx_data = |pa: &Pubkey| derive_tx_data_pda(pa, &key(1), 7).0;
        assert_is_expected(
            "settle_from_txdata",
            &settle_from_txdata_ix(&pa, &key(1), 7, settlement(Some(key(10)), true)),
        );
        assert_is_expected("txdata_init", &txdata_init_ix(&pa, &key(1), 7, 900, 500));
        assert_is_expected(
            "txdata_write",
            &txdata_write_ix(&pa, &key(1), 7, 3856, &[1, 2, 3, 4, 5]),
        );
        assert_is_expected("txdata_close", &txdata_close_ix(&pa, &key(1), &key(2), 7));

        let accounts = settlement(None, false);
        let ix = settle_from_txdata_ix(&ID, &key(1), 7, accounts);
        let by_name =
            assert_matches_the_adapters_idl(&ix, "settle_from_txdata", &7u64.to_le_bytes());
        assert_eq!(by_name("tx_data"), tx_data(&ID));
        assert_eq!(by_name("new_root_marker"), ID);
        let ix = txdata_init_ix(&ID, &key(1), 7, 900, 500);
        let args = [
            &7u64.to_le_bytes()[..],
            &900u32.to_le_bytes(),
            &500u64.to_le_bytes(),
        ]
        .concat();
        let by_name = assert_matches_the_adapters_idl(&ix, "txdata_init", &args);
        assert_eq!(by_name("authority"), key(1), "txdata_init");
        let ix = txdata_write_ix(&ID, &key(1), 7, 3856, &[1, 2, 3, 4, 5]);
        let args = [
            &7u64.to_le_bytes()[..],
            &3856u32.to_le_bytes(),
            &5u32.to_le_bytes(),
            &[1, 2, 3, 4, 5],
        ]
        .concat();
        let by_name = assert_matches_the_adapters_idl(&ix, "txdata_write", &args);
        assert_eq!(by_name("authority"), key(1), "txdata_write");
        let ix = txdata_close_ix(&ID, &key(1), &key(2), 7);
        let by_name = assert_matches_the_adapters_idl(&ix, "txdata_close", &7u64.to_le_bytes());
        assert_eq!(by_name("refund"), key(2));
    }

    #[test]
    fn the_owner_instructions_match_the_adapters_idl() {
        let pa = key(9);
        let entries = [
            DeniedLogicRef {
                logic_ref: [7; 32],
                consumed: false,
            },
            DeniedLogicRef {
                logic_ref: [8; 32],
                consumed: true,
            },
        ];
        assert_is_expected("pause", &pause_ix(&pa, &key(4)));
        assert_is_expected("unpause", &unpause_ix(&pa, &key(4)));
        assert_is_expected(
            "deny_logic_refs",
            &deny_logic_refs_ix(&pa, &key(4), &entries),
        );
        assert_is_expected(
            "set_kind_tree_root",
            &set_kind_tree_root_ix(&pa, &key(4), [7; 32]),
        );

        let authority = key(4);
        let bytes = [7; 32];
        for (ix, name, args) in [
            (pause_ix(&ID, &authority), "pause", &[][..]),
            (unpause_ix(&ID, &authority), "unpause", &[][..]),
            (
                deny_logic_refs_ix(&ID, &authority, &entries[..1]),
                "deny_logic_refs",
                &[&[1, 0, 0, 0][..], &bytes, &[0]].concat()[..],
            ),
            (
                set_kind_tree_root_ix(&ID, &authority, bytes),
                "set_kind_tree_root",
                &bytes[..],
            ),
            (
                transfer_ownership_ix(&ID, &authority, &key(5)),
                "transfer_ownership",
                &key(5).to_bytes()[..],
            ),
            (
                renounce_ownership_ix(&ID, &authority),
                "renounce_ownership",
                &[][..],
            ),
            (
                upgrade_ix(&ID, &authority, &key(6), &key(7)),
                "upgrade",
                &[][..],
            ),
        ] {
            let by_name = assert_matches_the_adapters_idl(&ix, name, args);
            assert_eq!(by_name("authority"), authority, "{name}");
            assert_eq!(
                by_name("event_authority"),
                derive_event_authority_pda(&ID).0,
                "{name}"
            );
            assert_eq!(by_name("program"), ID, "{name}");
        }
    }

    #[test]
    fn upgrade_names_the_buffer_and_the_programs_own_accounts() {
        let ix = upgrade_ix(&ID, &key(4), &key(6), &key(7));
        let by_name = assert_matches_the_adapters_idl(&ix, "upgrade", &[]);
        assert_eq!(by_name("buffer"), key(6));
        assert_eq!(by_name("spill"), key(7));
        assert_eq!(by_name("program_data"), derive_program_data_address(&ID));
        assert_eq!(by_name("upgraded_program"), ID);
        // At another address, its own program data and itself.
        let other = upgrade_ix(&key(9), &key(4), &key(6), &key(7));
        let accounts: Vec<Pubkey> = other.accounts.iter().map(|meta| meta.pubkey).collect();
        println!("{accounts:#?}");
        assert_eq!(accounts[2], derive_program_data_address(&key(9)));
        assert_eq!(accounts[3], key(9));
        assert!(!accounts.contains(&ID), "{accounts:?}");
    }

    #[test]
    fn the_remaining_instructions_match_the_adapters_idl() {
        let ix = update_expiry_config_ix(&ID, &key(4), 100, 200);
        let args = [&100u64.to_le_bytes()[..], &200u64.to_le_bytes()].concat();
        let by_name = assert_matches_the_adapters_idl(&ix, "update_expiry_config", &args);
        assert_eq!(by_name("authority"), key(4));

        let entry = derive_verifier_entry_pda(&key(3), crate::MOCK_SELECTOR);
        let ix = risc_zero_verifier_paused_ix(&ID, &entry);
        let by_name = assert_matches_the_adapters_idl(&ix, "risc_zero_verifier_paused", &[]);
        assert_eq!(by_name("verifier_entry"), entry);

        let ix = version_ix(&ID);
        // `version` takes no account, so no name has an address.
        let _ = assert_matches_the_adapters_idl(&ix, "version", &[]);
        assert!(ix.accounts.is_empty());

        let tx_data = derive_tx_data_pda(&ID, &key(1), 7).0;
        let ix = txdata_extend_ix(&ID, &key(1), 7, 900);
        let args = [&7u64.to_le_bytes()[..], &900u64.to_le_bytes()].concat();
        let by_name = assert_matches_the_adapters_idl(&ix, "txdata_extend", &args);
        assert_eq!(by_name("tx_data"), tx_data);
        assert_eq!(by_name("authority"), key(1));

        let ix = txdata_close_expired_ix(&ID, &key(2), &key(3), 7, &key(1));
        let args = [&7u64.to_le_bytes()[..], &key(1).to_bytes()].concat();
        let by_name = assert_matches_the_adapters_idl(&ix, "txdata_close_expired", &args);
        assert_eq!(by_name("tx_data"), tx_data);
        assert_eq!(by_name("payer"), key(2));
        assert_eq!(by_name("refund"), key(3));
    }
}
