//! Settle one adapter fixture on a cluster through this crate's builders.
//!
//! This is the pairing check behind `PA_TAG.txt`: the instruction layout,
//! PDA derivations, state decoder, Merkle replay and event decoders of the
//! client crate are exercised against a live adapter, end to end. Usage:
//!
//! ```text
//! cargo run -p settle-fixture -- --url <rpc> --keypair <path> --fixture <path>
//!     --pa <program id> [--call-accounts <b58>,<b58>]...
//! ```
//!
//! `--call-accounts` gives the account segment of one external call, in call
//! order, starting with the forwarder program (the adapter repo's committed
//! fixtures call the block-time forwarder with `[forwarder, clock sysvar]`).
//! Every step is a v1 transaction, the shape every submitter sends: the
//! settlement in one transaction when it fits, otherwise uploaded first.

use std::str::FromStr;

use anchor::AccountDeserialize;
use anoma_pa_solana_client::generated::protocol_adapter::events::{
    ActionExecuted, CommitmentTreeRootAdded, ForwarderCallExecuted, TransactionExecuted,
};
use anoma_pa_solana_client::generated::verifier_router::accounts::VerifierEntry;
use anoma_pa_solana_client::settlement::CallAccounts;
use anoma_pa_solana_client::{
    cpi_event_discriminator, decode_cpi_event, decode_pa_state, derive_pa_state_pda,
    derive_verifier_entry_pda, plan_settlement, transaction_size, EventDecodeError,
    PlannedTransaction, SettlementRequest, TXDATA_EXPIRY_SLOTS_DEFAULT,
};
use base64::Engine;
use solana_client::rpc_client::RpcClient;
use solana_client::rpc_config::RpcTransactionConfig;
use solana_commitment_config::CommitmentConfig;
use solana_sdk::instruction::AccountMeta;
use solana_sdk::message::VersionedMessage;
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
}

struct Args {
    url: String,
    keypair: String,
    fixture: String,
    pa: Pubkey,
    call_accounts: Vec<Vec<Pubkey>>,
}

fn parse_args() -> Args {
    let mut url = None;
    let mut keypair = None;
    let mut fixture = None;
    let mut pa = None;
    let mut call_accounts = Vec::new();
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let value = it.next().unwrap_or_else(|| panic!("{flag} takes a value"));
        match flag.as_str() {
            "--url" => url = Some(value),
            "--keypair" => keypair = Some(value),
            "--fixture" => fixture = Some(value),
            "--pa" => pa = Some(Pubkey::from_str(&value).expect("--pa is a base58 pubkey")),
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
        pa: pa.expect("--pa is required"),
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

/// Send `transaction` as one v1 transaction, the shape every submitter
/// sends. Prints the wire size and the accounts it names.
fn send(
    client: &RpcClient,
    payer: &Keypair,
    transaction: &PlannedTransaction,
    label: &str,
) -> Signature {
    try_send(client, payer, transaction, label).unwrap_or_else(|e| panic!("{e}"))
}

/// `send`, returning the failure instead of panicking on it.
fn try_send(
    client: &RpcClient,
    payer: &Keypair,
    transaction: &PlannedTransaction,
    label: &str,
) -> Result<Signature, String> {
    let blockhash = client.get_latest_blockhash().expect("blockhash");
    let message = transaction
        .message(&payer.pubkey(), blockhash)
        .expect("compile v1 message");
    let size = transaction_size(&message);
    let keys = message.account_keys.len();
    let tx = VersionedTransaction::try_new(VersionedMessage::V1(message), &[payer]).expect("sign");
    let sig = client
        .send_and_confirm_transaction(&tx)
        .map_err(|e| format!("{label} failed: {e}"))?;
    println!("{label}: {sig} ({size} bytes, {keys} accounts)");
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
        "payer {} | adapter {} | fixture {} ({} bytes, selector {})",
        payer.pubkey(),
        args.pa,
        args.fixture,
        tx_bytes.len(),
        fixture.selector
    );

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

    // 2. The external-call segments, as given on the command line.
    let call_segments: Vec<Vec<AccountMeta>> = args
        .call_accounts
        .iter()
        .map(|segment| {
            segment
                .iter()
                .map(|k| AccountMeta::new_readonly(*k, false))
                .collect()
        })
        .collect();

    // 3. Plan the settlement with the crate: one transaction, or the upload,
    //    the settle and the close (the remaining accounts, the predicted root
    //    and its marker either way).
    let upload_id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let verifier_entry = derive_verifier_entry_pda(&state.verifier_router, state.proof_selector);
    let plan = plan_settlement(
        SettlementRequest {
            pa_program: args.pa,
            payer: payer.pubkey(),
            input: &tx_bytes,
            state: &state,
            verifier_program: verifier_program_of(&client, &verifier_entry),
            nullifiers: &b64_32s(&fixture.consumed_nullifiers_b64),
            consumed_roots: &b64_32s(&fixture.historical_roots_b64),
            created: &b64_32s(&fixture.created_commitments_b64),
            accounts: CallAccounts {
                segments: call_segments,
                preceding: vec![],
            },
        },
        upload_id,
        client.get_slot().expect("slot") + TXDATA_EXPIRY_SLOTS_DEFAULT,
    )
    .expect("plan the settlement");
    println!("predicted root {:?}", plan.new_root.map(|root| hex(&root)));

    // 4. Settle: in one transaction, or upload, settle, then close the upload
    //    whatever the outcome, so a refused settlement does not strand its rent.
    let settle_sig = match &plan.upload {
        None => send(&client, &payer, &plan.settle, "settle"),
        Some(upload) => {
            send(&client, &payer, &upload.init, "txdata_init");
            let settled = (|| {
                for (i, write) in upload.writes.iter().enumerate() {
                    try_send(&client, &payer, write, &format!("txdata_write[{i}]"))?;
                }
                try_send(&client, &payer, &plan.settle, "settle_from_txdata")
            })();
            send(&client, &payer, &upload.close, "txdata_close");
            settled.unwrap_or_else(|e| panic!("{e}"))
        }
    };

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
    print_events(&client, &args.pa, &settle_sig);
}

/// The verifier program the router entry points at.
fn verifier_program_of(client: &RpcClient, verifier_entry: &Pubkey) -> Pubkey {
    let data = client
        .get_account_data(verifier_entry)
        .expect("verifier entry account");
    VerifierEntry::try_deserialize(&mut &*data)
        .expect("decode the verifier entry")
        .verifier
}

fn print_events(client: &RpcClient, pa: &Pubkey, sig: &Signature) {
    // The parsed encoding names each inner instruction's program; the adapter
    // has no RPC parser, so its event self-invocations arrive partially
    // decoded with base58 data.
    let tx = client
        .get_transaction_with_config(
            sig,
            RpcTransactionConfig {
                encoding: Some(UiTransactionEncoding::JsonParsed),
                commitment: Some(CommitmentConfig::confirmed()),
                max_supported_transaction_version: Some(1),
            },
        )
        .expect("fetch settle transaction");
    let meta = tx.transaction.meta.expect("meta");
    println!(
        "settlement consumed {:?} compute units",
        Option::<u64>::from(meta.compute_units_consumed)
    );
    let inner: Vec<_> = Option::from(meta.inner_instructions).unwrap_or_default();
    let pa = pa.to_string();
    let mut count = 0;
    for group in inner {
        for ix in group.instructions {
            let UiInstruction::Parsed(UiParsedInstruction::PartiallyDecoded(ix)) = ix else {
                continue;
            };
            if ix.program_id != pa {
                continue;
            }
            let data = bs58::decode(&ix.data).into_vec().expect("base58 ix data");
            let discriminator = match cpi_event_discriminator(&data) {
                Ok(discriminator) => discriminator,
                Err(EventDecodeError::NotAnEvent) => continue,
                Err(e) => panic!("an adapter event self-invocation: {e}"),
            };
            count += 1;
            if let Some(e) = decode_cpi_event::<TransactionExecuted>(&data).expect("decode event") {
                println!(
                    "event TransactionExecuted: transaction id {}",
                    hex(&e.transaction_id)
                );
            } else if let Some(e) = decode_cpi_event::<ActionExecuted>(&data).expect("decode event")
            {
                println!(
                    "event ActionExecuted: root {} nullifiers {} commitments {}",
                    hex(&e.action_tree_root),
                    e.nullifiers.len(),
                    e.commitments.len()
                );
            } else if let Some(e) =
                decode_cpi_event::<CommitmentTreeRootAdded>(&data).expect("decode event")
            {
                println!("event CommitmentTreeRootAdded: root {}", hex(&e.root));
            } else if let Some(e) =
                decode_cpi_event::<ForwarderCallExecuted>(&data).expect("decode event")
            {
                println!(
                    "event ForwarderCallExecuted: forwarder {} input {} B output {} B",
                    e.untrusted_forwarder,
                    e.input.len(),
                    e.output.len()
                );
            } else {
                println!("event with discriminator {}", hex(&discriminator));
            }
        }
    }
    println!("{count} events decoded from the settlement");
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
