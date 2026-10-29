//! Cross-package fixture: `fixtures/instructions_fixture.json` holds every
//! adapter instruction the crate's builders build for fixed inputs (the
//! program `key(9)`, other accounts `key(n)`), with inputs larger than 1000
//! bytes for `settle`, `simulate_execute`, `txdata_write` and
//! `deny_logic_refs`. The TS package
//! builds the same instructions and checks them against the same file
//! (`ts/src/instance.test.ts`).
//!
//! Regenerate it after a builder or IDL change:
//!
//! ```sh
//! cargo test -p anoma-pa-solana-client --test instructions_fixture -- --ignored regenerate
//! ```

use anoma_pa_solana_client::generated::protocol_adapter::types::DeniedLogicRef;
use anoma_pa_solana_client::*;
use serde_json::{json, Value};
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../fixtures/instructions_fixture.json"
);

fn key(n: u8) -> Pubkey {
    Pubkey::new_from_array([n; 32])
}

/// `n` bytes, byte `i` being `i` mod 256.
fn large(n: usize) -> Vec<u8> {
    (0..n).map(|i| i as u8).collect()
}

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

/// Each fixture entry's name and the instruction the crate builds for it.
fn instructions() -> Vec<(&'static str, Instruction)> {
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
    let many: Vec<DeniedLogicRef> = (0..40u8)
        .map(|i| DeniedLogicRef {
            logic_ref: [i; 32],
            consumed: i % 2 == 1,
        })
        .collect();
    vec![
        (
            "initialize",
            initialize_ix(&pa, &key(1), &key(2), &key(3), MOCK_SELECTOR),
        ),
        ("pause", pause_ix(&pa, &key(4))),
        ("unpause", unpause_ix(&pa, &key(4))),
        (
            "deny_logic_refs",
            deny_logic_refs_ix(&pa, &key(4), &entries),
        ),
        (
            "deny_logic_refs_large",
            deny_logic_refs_ix(&pa, &key(4), &many),
        ),
        (
            "set_kind_tree_root",
            set_kind_tree_root_ix(&pa, &key(4), [7; 32]),
        ),
        (
            "transfer_ownership",
            transfer_ownership_ix(&pa, &key(4), &key(5)),
        ),
        ("renounce_ownership", renounce_ownership_ix(&pa, &key(4))),
        (
            "update_expiry_config",
            update_expiry_config_ix(&pa, &key(4), 100, u64::MAX),
        ),
        ("upgrade", upgrade_ix(&pa, &key(4), &key(6), &key(7))),
        (
            "risc_zero_verifier_paused",
            risc_zero_verifier_paused_ix(&pa, &derive_verifier_entry_pda(&key(3), MOCK_SELECTOR)),
        ),
        ("version", version_ix(&pa)),
        ("txdata_init", txdata_init_ix(&pa, &key(1), 7, 900, 500)),
        (
            "txdata_write",
            txdata_write_ix(&pa, &key(1), 7, 3856, &[1, 2, 3, 4, 5]),
        ),
        (
            "txdata_write_large",
            txdata_write_ix(&pa, &key(1), 7, 0, &large(TXDATA_CHUNK_SIZE)),
        ),
        ("txdata_extend", txdata_extend_ix(&pa, &key(1), 7, 900)),
        ("txdata_close", txdata_close_ix(&pa, &key(1), &key(2), 7)),
        (
            "txdata_close_expired",
            txdata_close_expired_ix(&pa, &key(3), &key(2), 7, &key(1)),
        ),
        (
            "settle",
            settle_ix(&pa, &key(1), b"input", settlement(Some(key(10)), true)),
        ),
        (
            "settle_no_marker",
            settle_ix(&pa, &key(1), b"input", settlement(None, false)),
        ),
        (
            "settle_large",
            settle_ix(&pa, &key(1), &large(1500), settlement(Some(key(10)), true)),
        ),
        (
            "simulate_execute_false",
            simulate_execute_ix(
                &pa,
                &key(1),
                b"input",
                false,
                settlement(Some(key(10)), true),
            ),
        ),
        (
            "simulate_execute_true",
            simulate_execute_ix(&pa, &key(1), b"input", true, settlement(None, false)),
        ),
        (
            "simulate_execute_large",
            simulate_execute_ix(
                &pa,
                &key(1),
                &large(1500),
                true,
                settlement(Some(key(10)), true),
            ),
        ),
        (
            "settle_from_txdata",
            settle_from_txdata_ix(&pa, &key(1), 7, settlement(Some(key(10)), true)),
        ),
    ]
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The fixture entry of `ix`, named `name`.
fn entry(name: &str, ix: &Instruction) -> Value {
    json!({
        "name": name,
        "program": ix.program_id.to_string(),
        "data_hex": hex(&ix.data),
        "accounts": ix.accounts.iter().map(|m| json!({
            "address": m.pubkey.to_string(),
            "signer": m.is_signer,
            "writable": m.is_writable,
        })).collect::<Vec<_>>(),
    })
}

const COMMENT: &str = "Adapter instructions built by the Rust crate's instructions::*_ix for the inputs in rust/tests/instructions_fixture.rs (the program key(9), other accounts key(n): 32 bytes n; byte i of a large input is i mod 256). Each entry: the program, the instruction data in lowercase hex, and the accounts in order. Regenerated by `cargo test -p anoma-pa-solana-client --test instructions_fixture -- --ignored regenerate`. The Rust crate and the TS package must build every entry exactly.";

/// The fixture the crate's builders produce, as the file holds it.
fn built() -> String {
    let instructions: Vec<Value> = instructions()
        .iter()
        .map(|(name, ix)| entry(name, ix))
        .collect();
    let doc = json!({ "_comment": COMMENT, "instructions": instructions });
    serde_json::to_string_pretty(&doc).expect("the fixture serializes") + "\n"
}

#[test]
fn the_builders_build_the_fixture() {
    const FIX: &str = "Fix: run the regenerate command in this file's header and review the diff";
    let text = std::fs::read_to_string(FIXTURE).unwrap_or_else(|e| {
        panic!("read {FIXTURE}: {e}. Fix: run the regenerate command in this file's header")
    });
    let fixture: Value = serde_json::from_str(&text).expect("the instructions fixture is JSON");
    assert_eq!(fixture["_comment"], COMMENT, "the fixture's comment. {FIX}");
    let committed: Vec<&Value> = fixture["instructions"]
        .as_array()
        .expect("the fixture lists instructions")
        .iter()
        .collect();
    let instructions = instructions();
    assert_eq!(
        instructions
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>(),
        committed
            .iter()
            .map(|e| e["name"].as_str().expect("each entry has a name"))
            .collect::<Vec<_>>(),
        "the fixture's entries are not the builders'. {FIX}"
    );
    for ((name, ix), committed) in instructions.iter().zip(committed) {
        println!(
            "{name}: {} accounts, {} data bytes",
            ix.accounts.len(),
            ix.data.len()
        );
        assert_eq!(&entry(name, ix), committed, "{name}. {FIX}");
    }
}

// Each of these four reaches past 1000 bytes, which the TS package's coder
// must encode as well.
#[test]
fn the_large_entries_exceed_1000_bytes() {
    for (name, ix) in instructions() {
        if name.ends_with("_large") {
            println!("{name}: {} data bytes", ix.data.len());
            assert!(ix.data.len() > 1000, "{name}: {} data bytes", ix.data.len());
        }
    }
}

#[test]
#[ignore = "writes the fixture; run explicitly to regenerate it"]
fn regenerate() {
    std::fs::write(FIXTURE, built()).unwrap_or_else(|e| panic!("write {FIXTURE}: {e}"));
    println!("wrote {FIXTURE}");
}
