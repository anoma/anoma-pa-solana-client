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
//!
//! A fixture carrying `spl_token_wrap` metadata (the adapter repo's AnomaPay
//! wrap) is settled as a wrap: the segment comes from the forwarder builders,
//! the user's signature rides in an ed25519 instruction at index 0, and the
//! user's nonce bitmap is created in the same transaction when the word has
//! none yet. The user and mint keypairs are seeded from the fixture's labels;
//! the user's token account must hold the amount with the forwarder's escrow
//! authority as its delegate, and the forwarder must be initialized for the
//! mint (`--forwarder`, default `FORWARDER_PROGRAM_ID`).
//!
//! A fixture carrying `spl_token_unwrap` metadata is settled as an unwrap to
//! its seeded recipient: the segment comes from the forwarder builders, and
//! the recipient's token account is created in the same transaction when it
//! does not exist yet.

use std::str::FromStr;

use anoma_pa_solana_client::{
    build_unwrap_forwarder_accounts, build_wrap_forwarder_accounts, create_ata_idempotent_ix,
    decode_event_instruction, decode_forwarder_event_instruction, decode_pa_state,
    derive_nonce_bitmap_pda, derive_pa_state_pda, derive_verifier_entry_pda, init_nonce_bitmap_ix,
    nonce_word_index, plan_settlement, sha256, ForwarderEvent, PaEvent, SettlementRequest,
    EVENT_IX_TAG, FORWARDER_PROGRAM_ID, PA_PROGRAM_ID, SETTLE_LOOKUP_TABLE,
    TXDATA_EXPIRY_SLOTS_DEFAULT,
};
use base64::Engine;
use solana_address_lookup_table_interface::state::AddressLookupTable;
use solana_client::rpc_client::RpcClient;
use solana_client::rpc_config::RpcTransactionConfig;
use solana_commitment_config::CommitmentConfig;
use solana_ed25519_program::new_ed25519_instruction_with_signature;
use solana_sdk::instruction::{AccountMeta, Instruction};
use solana_sdk::message::{v0, AddressLookupTableAccount, VersionedMessage};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{read_keypair_file, Keypair, Signature, Signer};
use solana_sdk::transaction::VersionedTransaction;
use solana_transaction_status_client_types::{
    UiInstruction, UiParsedInstruction, UiTransactionEncoding,
};

#[derive(serde::Deserialize)]
struct Fixture {
    selector: String,
    tx_b64: String,
    consumed_nullifiers_b64: Vec<String>,
    created_commitments_b64: Vec<String>,
    /// The consumed roots other than the initial one; absent when there are none.
    #[serde(default)]
    historical_roots_b64: Vec<String>,
    spl_token_wrap: Option<SplTokenWrap>,
    spl_token_unwrap: Option<SplTokenUnwrap>,
}

/// The adapter repo's AnomaPay unwrap fixture metadata: the seeded mint and
/// recipient the proof releases the tokens to.
#[derive(serde::Deserialize)]
struct SplTokenUnwrap {
    mint_seed_label: String,
    recipient_seed_label: String,
}

/// The adapter repo's AnomaPay wrap fixture metadata: the seeded parties, the
/// wrap terms and the signature the proof is bound to.
#[derive(serde::Deserialize)]
struct SplTokenWrap {
    user_seed_label: String,
    mint_seed_label: String,
    nonce: u64,
    signed_message_b64: String,
    signature_b64: String,
}

/// A keypair seeded with sha256 of a label, as the adapter's fixtures and
/// tests derive their parties.
fn seeded_pubkey(label: &str) -> Pubkey {
    Keypair::new_from_array(sha256(label.as_bytes())).pubkey()
}

struct Args {
    url: String,
    keypair: String,
    fixture: String,
    pa: Pubkey,
    forwarder: Pubkey,
    lookup_table: Pubkey,
    call_accounts: Vec<Vec<Pubkey>>,
}

fn parse_args() -> Args {
    let mut url = None;
    let mut keypair = None;
    let mut fixture = None;
    let mut pa = PA_PROGRAM_ID;
    let mut forwarder = FORWARDER_PROGRAM_ID;
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
            "--forwarder" => {
                forwarder = Pubkey::from_str(&value).expect("--forwarder is a base58 pubkey")
            }
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
        forwarder,
        lookup_table,
        call_accounts,
    }
}

fn b64(s: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .expect("fixture field is base64")
}

fn b64_32s(fields: &[String]) -> Vec<[u8; 32]> {
    fields
        .iter()
        .map(|s| b64(s).try_into().expect("32-byte fixture field"))
        .collect()
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

/// Send `ixs` as a v0 transaction compiled against `tables`, the shape every
/// submitter sends (none for the upload steps, the settlement table for the
/// settle). Prints the wire size and how many keys the tables absorbed.
fn send(
    client: &RpcClient,
    payer: &Keypair,
    ixs: &[Instruction],
    tables: &[AddressLookupTableAccount],
    label: &str,
) -> Signature {
    try_send(client, payer, ixs, tables, label).unwrap_or_else(|e| panic!("{e}"))
}

/// `send`, returning the failure instead of panicking on it.
fn try_send(
    client: &RpcClient,
    payer: &Keypair,
    ixs: &[Instruction],
    tables: &[AddressLookupTableAccount],
    label: &str,
) -> Result<Signature, String> {
    let blockhash = client.get_latest_blockhash().expect("blockhash");
    let message = v0::Message::try_compile(&payer.pubkey(), ixs, tables, blockhash)
        .expect("compile v0 message");
    let looked_up: usize = message
        .address_table_lookups
        .iter()
        .map(|l| l.writable_indexes.len() + l.readonly_indexes.len())
        .sum();
    let static_keys = message.account_keys.len();
    let tx = VersionedTransaction::try_new(VersionedMessage::V0(message), &[payer]).expect("sign");
    let size = bincode::serialized_size(&tx).expect("serialize");
    let sig = client
        .send_and_confirm_transaction(&tx)
        .map_err(|e| format!("{label} failed: {e}"))?;
    println!("{label}: {sig} ({size} bytes, {static_keys} static keys, {looked_up} looked up)");
    Ok(sig)
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
        "pa_state {pa_state}: schema {} paused {} next_index {} depth {} root {} selector {}",
        state.schema_version,
        state.paused,
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

    // 2. The external-call segments. A wrap's segment comes from the
    //    forwarder builders, and its settlement starts with the ed25519
    //    instruction the wrap input names (index 0) and the bitmap creation
    //    when the nonce's word has none.
    let mut call_segments: Vec<Vec<AccountMeta>> = Vec::new();
    let mut pre_instructions: Vec<Instruction> = Vec::new();
    if let Some(wrap) = &fixture.spl_token_wrap {
        let user = seeded_pubkey(&wrap.user_seed_label);
        let mint = seeded_pubkey(&wrap.mint_seed_label);
        let signature: [u8; 64] = b64(&wrap.signature_b64)
            .try_into()
            .expect("64-byte ed25519 signature");
        pre_instructions.push(new_ed25519_instruction_with_signature(
            &b64(&wrap.signed_message_b64),
            &signature,
            &user.to_bytes(),
        ));
        let word = nonce_word_index(wrap.nonce);
        let (bitmap, _) = derive_nonce_bitmap_pda(&args.forwarder, &user, word);
        if client.get_account(&bitmap).is_err() {
            println!(
                "nonce bitmap {bitmap} for word {word} is missing: creating it in the settlement"
            );
            pre_instructions.push(init_nonce_bitmap_ix(
                &args.forwarder,
                &payer.pubkey(),
                &user,
                word,
            ));
        }
        call_segments.push(build_wrap_forwarder_accounts(
            &args.forwarder,
            &user,
            &mint,
            wrap.nonce,
        ));
        println!(
            "wrap: user {user} mint {mint} nonce {} forwarder {}",
            wrap.nonce, args.forwarder
        );
    }
    if let Some(unwrap) = &fixture.spl_token_unwrap {
        let recipient = seeded_pubkey(&unwrap.recipient_seed_label);
        let mint = seeded_pubkey(&unwrap.mint_seed_label);
        pre_instructions.push(create_ata_idempotent_ix(&payer.pubkey(), &recipient, &mint));
        call_segments.push(build_unwrap_forwarder_accounts(
            &args.forwarder,
            &recipient,
            &mint,
        ));
        println!(
            "unwrap: recipient {recipient} mint {mint} forwarder {}",
            args.forwarder
        );
    }
    call_segments.extend(args.call_accounts.iter().map(|segment| {
        segment
            .iter()
            .map(|k| AccountMeta::new_readonly(*k, false))
            .collect()
    }));

    // 3. Plan the settlement with the crate: the upload, the settle (the
    //    remaining accounts, the predicted root and its marker) and the close.
    let upload_id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let verifier_entry =
        derive_verifier_entry_pda(&Pubkey::from(state.verifier_router), state.proof_selector);
    let plan = plan_settlement(SettlementRequest {
        pa_program: args.pa,
        payer: payer.pubkey(),
        upload_id,
        expires_slot: client.get_slot().expect("slot") + TXDATA_EXPIRY_SLOTS_DEFAULT,
        input: &tx_bytes,
        state: &state,
        verifier_program: verifier_program_of(&client, &verifier_entry),
        nullifiers: &b64_32s(&fixture.consumed_nullifiers_b64),
        consumed_roots: &b64_32s(&fixture.historical_roots_b64),
        created: &b64_32s(&fixture.created_commitments_b64),
        call_segments,
    })
    .expect("plan the settlement");
    println!("predicted root {:?}", plan.new_root.map(|root| hex(&root)));
    let mut settle_ixs = pre_instructions;
    settle_ixs.extend(plan.settle);

    // 4. Upload, settle, then close the upload whatever the outcome, so a
    //    refused settlement does not strand its rent.
    send(&client, &payer, &[plan.init], &[], "txdata_init");
    let settled = (|| {
        for (i, write) in plan.writes.into_iter().enumerate() {
            try_send(
                &client,
                &payer,
                &[write],
                &[],
                &format!("txdata_write[{i}]"),
            )?;
        }
        try_send(
            &client,
            &payer,
            &settle_ixs,
            std::slice::from_ref(&table),
            "settle_from_txdata",
        )
    })();
    send(&client, &payer, &[plan.close], &[], "txdata_close");
    let settle_sig = settled.unwrap_or_else(|e| panic!("{e}"));

    // 5. Read back: the root moved to the prediction, and the events decode.
    let after = decode_pa_state(&client.get_account_data(&pa_state).expect("pa_state")).unwrap();
    assert_eq!(
        hex(&after.root),
        hex(&plan.new_root.unwrap_or(state.root)),
        "on-chain root must equal the replayed root"
    );
    println!(
        "root after settlement matches the replay: {}",
        hex(&after.root)
    );
    print_events(&client, &args.pa, &args.forwarder, &settle_sig);
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

fn print_events(client: &RpcClient, pa: &Pubkey, forwarder: &Pubkey, sig: &Signature) {
    // The parsed encoding resolves lookup-table addresses and names each
    // inner instruction's program; neither the adapter nor the forwarder has
    // an RPC parser, so their event self-invocations arrive partially decoded
    // with base58 data.
    let tx = client
        .get_transaction_with_config(
            sig,
            RpcTransactionConfig {
                encoding: Some(UiTransactionEncoding::JsonParsed),
                commitment: Some(CommitmentConfig::confirmed()),
                max_supported_transaction_version: Some(0),
            },
        )
        .expect("fetch settle transaction");
    let meta = tx.transaction.meta.expect("meta");
    println!(
        "settlement consumed {:?} compute units",
        Option::<u64>::from(meta.compute_units_consumed)
    );
    let inner: Vec<_> = Option::from(meta.inner_instructions).unwrap_or_default();
    let (pa, forwarder) = (pa.to_string(), forwarder.to_string());
    let mut count = 0;
    for group in inner {
        for ix in group.instructions {
            let UiInstruction::Parsed(UiParsedInstruction::PartiallyDecoded(ix)) = ix else {
                continue;
            };
            if ix.program_id != pa && ix.program_id != forwarder {
                continue;
            }
            let data = bs58::decode(&ix.data).into_vec().expect("base58 ix data");
            if !data.starts_with(&EVENT_IX_TAG) {
                continue;
            }
            count += 1;
            if ix.program_id == forwarder {
                match decode_forwarder_event_instruction(&data).expect("decode forwarder event") {
                    ForwarderEvent::Wrapped(e) => println!(
                        "forwarder event Wrapped: mint {} from {} amount {} nonce {}",
                        Pubkey::from(e.token_mint),
                        Pubkey::from(e.from),
                        e.amount,
                        e.nonce
                    ),
                    ForwarderEvent::Unwrapped(e) => println!(
                        "forwarder event Unwrapped: mint {} to {} amount {}",
                        Pubkey::from(e.token_mint),
                        Pubkey::from(e.to),
                        e.amount
                    ),
                    other => println!("forwarder event {other:?}"),
                }
                continue;
            }
            match decode_event_instruction(&data).expect("decode event") {
                PaEvent::TransactionExecuted(e) => println!(
                    "event TransactionExecuted: transaction id {}",
                    hex(&e.transaction_id)
                ),
                PaEvent::ActionExecuted(e) => println!(
                    "event ActionExecuted: root {} nullifiers {} commitments {}",
                    hex(&e.action_tree_root),
                    e.nullifiers.len(),
                    e.commitments.len()
                ),
                PaEvent::CommitmentTreeRootAdded(e) => {
                    println!("event CommitmentTreeRootAdded: root {}", hex(&e.root))
                }
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
