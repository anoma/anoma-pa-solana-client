//! Settle one adapter fixture on a cluster through this crate's builders.
//!
//! This is the pairing check behind `PA_COMMIT.txt`: the instruction layout,
//! PDA derivations, state decoder, Merkle replay and event decoders of the
//! client crate are exercised against a live adapter, end to end. Usage:
//!
//! ```text
//! cargo run -p settle-fixture -- --url <rpc> --keypair <path> --fixture <path>
//!     [--pa <program id>] [--lookup-table <address>] [--call-accounts <b58>,<b58>]...
//! ```
//!
//! `--call-accounts` gives the account segment of one external call, in call
//! order, starting with the forwarder program (the adapter repo's committed
//! fixtures call the block-time forwarder with `[forwarder, clock sysvar]`).
//! `--lookup-table` is the deployment's settlement lookup table (default: the
//! devnet table `SETTLE_LOOKUP_TABLE`); the settle step is a v0 transaction
//! against it, the shape every submitter sends.

use std::str::FromStr;

use anoma_pa_solana_client::{
    decode_event_instruction, decode_pa_state, derive_nullifier_pda, derive_pa_state_pda,
    derive_root_marker_pda, derive_tx_data_pda, derive_verifier_router_pdas, settle_from_txdata_ix,
    txdata_close_ix, txdata_init_ix, txdata_write_ix, CommitmentTreeState, PaEvent, EVENT_IX_TAG,
    PA_PROGRAM_ID, SETTLE_COMPUTE_UNIT_LIMIT, SETTLE_HEAP_FRAME_BYTES, SETTLE_LOOKUP_TABLE,
    TXDATA_CHUNK_SIZE, TXDATA_EXPIRY_SLOTS_DEFAULT,
};
use base64::Engine;
use solana_address_lookup_table_interface::state::AddressLookupTable;
use solana_client::rpc_client::RpcClient;
use solana_client::rpc_config::RpcTransactionConfig;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::compute_budget::ComputeBudgetInstruction;
use solana_sdk::instruction::{AccountMeta, Instruction};
use solana_sdk::message::{v0, AddressLookupTableAccount, VersionedMessage};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{read_keypair_file, Keypair, Signature, Signer};
use solana_sdk::transaction::{Transaction, VersionedTransaction};
use solana_transaction_status::{UiInstruction, UiTransactionEncoding};

#[derive(serde::Deserialize)]
struct Fixture {
    selector: String,
    tx_b64: String,
    consumed_nullifiers_b64: Vec<String>,
    created_commitments_b64: Vec<String>,
}

struct Args {
    url: String,
    keypair: String,
    fixture: String,
    pa: Pubkey,
    lookup_table: Pubkey,
    call_accounts: Vec<Vec<Pubkey>>,
}

fn parse_args() -> Args {
    let mut url = None;
    let mut keypair = None;
    let mut fixture = None;
    let mut pa = PA_PROGRAM_ID;
    let mut lookup_table = SETTLE_LOOKUP_TABLE;
    let mut call_accounts = Vec::new();
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let value = it.next().unwrap_or_else(|| panic!("{flag} takes a value"));
        match flag.as_str() {
            "--url" => url = Some(value),
            "--keypair" => keypair = Some(value),
            "--fixture" => fixture = Some(value),
            "--pa" => pa = Pubkey::from_str(&value).expect("--pa is a base58 pubkey"),
            "--lookup-table" => {
                lookup_table = Pubkey::from_str(&value).expect("--lookup-table is a base58 pubkey")
            }
            "--call-accounts" => call_accounts.push(
                value
                    .split(',')
                    .map(|s| Pubkey::from_str(s).expect("call account is a base58 pubkey"))
                    .collect(),
            ),
            other => panic!("unknown flag {other}"),
        }
    }
    Args {
        url: url.expect("--url is required"),
        keypair: keypair.expect("--keypair is required"),
        fixture: fixture.expect("--fixture is required"),
        pa,
        lookup_table,
        call_accounts,
    }
}

fn b64(s: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .expect("fixture field is base64")
}

fn b64_32(s: &str) -> [u8; 32] {
    b64(s).try_into().expect("32-byte fixture field")
}

fn send(client: &RpcClient, payer: &Keypair, ixs: &[Instruction], label: &str) -> Signature {
    let blockhash = client.get_latest_blockhash().expect("blockhash");
    let tx = Transaction::new_signed_with_payer(ixs, Some(&payer.pubkey()), &[payer], blockhash);
    let sig = client
        .send_and_confirm_transaction(&tx)
        .unwrap_or_else(|e| panic!("{label} failed: {e}"));
    println!("{label}: {sig}");
    sig
}

/// The deployment's settlement lookup table, as the message compiler wants it.
fn lookup_table(client: &RpcClient, address: Pubkey) -> AddressLookupTableAccount {
    let data = client
        .get_account_data(&address)
        .unwrap_or_else(|e| panic!("lookup table {address} not found: {e}"));
    let table = AddressLookupTable::deserialize(&data).expect("lookup table state");
    AddressLookupTableAccount {
        key: address,
        addresses: table.addresses.to_vec(),
    }
}

/// Send `ixs` as a v0 transaction compiled against `table`, the shape every
/// settlement submitter sends. Prints the wire size and how many keys the
/// table absorbed.
fn send_v0(
    client: &RpcClient,
    payer: &Keypair,
    ixs: &[Instruction],
    table: &AddressLookupTableAccount,
    label: &str,
) -> Signature {
    let blockhash = client.get_latest_blockhash().expect("blockhash");
    let message =
        v0::Message::try_compile(&payer.pubkey(), ixs, std::slice::from_ref(table), blockhash)
            .expect("compile v0 message");
    let looked_up: usize = message
        .address_table_lookups
        .iter()
        .map(|l| l.writable_indexes.len() + l.readonly_indexes.len())
        .sum();
    let static_keys = message.account_keys.len();
    let tx = VersionedTransaction::try_new(VersionedMessage::V0(message), &[payer]).expect("sign");
    let size = bincode::serialize(&tx).expect("serialize").len();
    let sig = client
        .send_and_confirm_transaction(&tx)
        .unwrap_or_else(|e| panic!("{label} failed: {e}"));
    println!("{label}: {sig} ({size} bytes, {static_keys} static keys, {looked_up} looked up)");
    sig
}

fn main() {
    let args = parse_args();
    let client = RpcClient::new_with_commitment(args.url.clone(), CommitmentConfig::confirmed());
    let payer = read_keypair_file(&args.keypair).expect("keypair file");
    let fixture: Fixture =
        serde_json::from_str(&std::fs::read_to_string(&args.fixture).expect("read fixture"))
            .expect("fixture json");
    let tx_bytes = b64(&fixture.tx_b64);
    println!(
        "payer {} | adapter {} | lookup table {} | fixture {} ({} bytes, selector {})",
        payer.pubkey(),
        args.pa,
        args.lookup_table,
        args.fixture,
        tx_bytes.len(),
        fixture.selector
    );
    let table = lookup_table(&client, args.lookup_table);

    // 1. Read and decode the adapter state with the crate's decoder.
    let (pa_state, _) = derive_pa_state_pda(&args.pa);
    let state_data = client
        .get_account_data(&pa_state)
        .expect("pa_state account");
    let state = decode_pa_state(&state_data).expect("decode pa_state");
    println!(
        "pa_state {pa_state}: schema {} lifecycle {} next_index {} depth {} root {} selector {}",
        state.schema_version,
        state.lifecycle,
        state.next_index,
        state.current_depth,
        hex(&state.root),
        hex(&state.proof_selector)
    );
    let fixture_selector = fixture.selector.trim_start_matches("0x");
    assert_eq!(
        hex(&state.proof_selector),
        fixture_selector,
        "fixture selector must match the deployment's pinned selector"
    );

    // 2. Replay the commitment tree to predict the post-settlement root.
    let mut tree = CommitmentTreeState {
        root: state.root,
        next_index: state.next_index,
        current_depth: state.current_depth,
        frontier: state.frontier.clone(),
    };
    for c in &fixture.created_commitments_b64 {
        tree.append(b64_32(c)).expect("append commitment");
    }
    let (new_root_marker, _) = derive_root_marker_pda(&args.pa, &pa_state, &tree.root);
    println!(
        "predicted root {} marker {new_root_marker}",
        hex(&tree.root)
    );

    // 3. Remaining accounts: nullifier markers, then the external-call segments.
    let mut remaining: Vec<AccountMeta> = fixture
        .consumed_nullifiers_b64
        .iter()
        .map(|n| {
            AccountMeta::new(
                derive_nullifier_pda(&args.pa, &pa_state, &b64_32(n)).0,
                false,
            )
        })
        .collect();
    for segment in &args.call_accounts {
        remaining.extend(segment.iter().map(|k| AccountMeta::new_readonly(*k, false)));
    }

    // 4. Upload the transaction in chunks.
    let upload_id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let (tx_data, _) = derive_tx_data_pda(&args.pa, &payer.pubkey(), upload_id);
    let expires_slot = client.get_slot().expect("slot") + TXDATA_EXPIRY_SLOTS_DEFAULT;
    send(
        &client,
        &payer,
        &[txdata_init_ix(
            &args.pa,
            &pa_state,
            &tx_data,
            &payer.pubkey(),
            upload_id,
            tx_bytes.len() as u32,
            expires_slot,
        )],
        "txdata_init",
    );
    for (i, chunk) in tx_bytes.chunks(TXDATA_CHUNK_SIZE).enumerate() {
        send(
            &client,
            &payer,
            &[txdata_write_ix(
                &args.pa,
                &tx_data,
                &payer.pubkey(),
                upload_id,
                (i * TXDATA_CHUNK_SIZE) as u32,
                chunk,
            )],
            &format!("txdata_write[{i}]"),
        );
    }

    // 5. Settle.
    let (router, verifier_entry) =
        derive_verifier_router_pdas(&Pubkey::from(state.verifier_router));
    let verifier_program = verifier_program_of(&client, &verifier_entry);
    let settle_sig = send_v0(
        &client,
        &payer,
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(SETTLE_COMPUTE_UNIT_LIMIT),
            ComputeBudgetInstruction::request_heap_frame(SETTLE_HEAP_FRAME_BYTES),
            settle_from_txdata_ix(
                &args.pa,
                &pa_state,
                &tx_data,
                &payer.pubkey(),
                upload_id,
                &new_root_marker,
                &Pubkey::from(state.verifier_router),
                &router,
                &verifier_entry,
                &verifier_program,
                remaining,
            ),
        ],
        &table,
        "settle_from_txdata",
    );
    send(
        &client,
        &payer,
        &[txdata_close_ix(
            &args.pa,
            &tx_data,
            &payer.pubkey(),
            &payer.pubkey(),
            upload_id,
        )],
        "txdata_close",
    );

    // 6. Read back: the root moved to the prediction, and the events decode.
    let after = decode_pa_state(&client.get_account_data(&pa_state).expect("pa_state")).unwrap();
    assert_eq!(
        hex(&after.root),
        hex(&tree.root),
        "on-chain root must equal the replayed root"
    );
    println!(
        "root after settlement matches the replay: {}",
        hex(&after.root)
    );
    print_events(&client, &args.pa, &settle_sig);
}

/// The verifier program the router entry points at (first 32 bytes after the
/// entry's discriminator and selector are the entry's `verifier` field).
fn verifier_program_of(client: &RpcClient, verifier_entry: &Pubkey) -> Pubkey {
    let data = client
        .get_account_data(verifier_entry)
        .expect("verifier entry account");
    // VerifierEntry { selector: [u8; 4], verifier: Pubkey } after the 8-byte discriminator.
    Pubkey::try_from(&data[12..44]).expect("verifier pubkey")
}

fn print_events(client: &RpcClient, pa: &Pubkey, sig: &Signature) {
    let tx = client
        .get_transaction_with_config(
            sig,
            RpcTransactionConfig {
                encoding: Some(UiTransactionEncoding::Json),
                commitment: Some(CommitmentConfig::confirmed()),
                max_supported_transaction_version: Some(0),
            },
        )
        .expect("fetch settle transaction");
    let meta = tx.transaction.meta.expect("meta");
    let keys: Vec<String> = match tx.transaction.transaction {
        solana_transaction_status::EncodedTransaction::Json(ui) => match ui.message {
            solana_transaction_status::UiMessage::Raw(raw) => raw.account_keys,
            other => panic!("unexpected message encoding {other:?}"),
        },
        other => panic!("unexpected transaction encoding {other:?}"),
    };
    let inner: Vec<_> = Option::from(meta.inner_instructions).unwrap_or_default();
    let mut count = 0;
    for group in inner {
        for ix in group.instructions {
            let UiInstruction::Compiled(c) = ix else {
                continue;
            };
            if keys[c.program_id_index as usize] != pa.to_string() {
                continue;
            }
            let data = bs58::decode(&c.data).into_vec().expect("base58 ix data");
            if !data.starts_with(&EVENT_IX_TAG) {
                continue;
            }
            count += 1;
            match decode_event_instruction(&data).expect("decode event") {
                PaEvent::TransactionExecuted(e) => println!(
                    "event TransactionExecuted: {} tags, is_consumed {:?}",
                    e.tags.len(),
                    e.is_consumed
                ),
                PaEvent::ActionExecuted(e) => println!(
                    "event ActionExecuted: root {} tag_count {}",
                    hex(&e.action_tree_root),
                    e.action_tag_count
                ),
                PaEvent::ForwarderCallExecuted(e) => println!(
                    "event ForwarderCallExecuted: forwarder {} input {} B output {} B",
                    Pubkey::from(e.forwarder),
                    e.input.len(),
                    e.output.len()
                ),
                other => println!("event {other:?}"),
            }
        }
    }
    println!("{count} events decoded from the settlement");
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
