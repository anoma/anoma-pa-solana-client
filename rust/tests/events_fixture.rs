//! Cross-package fixture: every CPI event instruction in
//! `fixtures/events_fixture.json` must decode to the recorded values. The TS
//! package runs the same fixture (`ts/src/events.test.ts`).

use anoma_pa_solana_client::events::{decode_event_instruction, PaEvent, PayloadEvent};
use base64::Engine;

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

fn hex32(s: &str) -> [u8; 32] {
    hex(s).try_into().expect("32 bytes")
}

fn fixture() -> serde_json::Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../fixtures/events_fixture.json"
    );
    serde_json::from_str(&std::fs::read_to_string(path).expect("read fixture")).expect("json")
}

fn check_payload(ev: &PayloadEvent, exp: &serde_json::Value, entry: &str) {
    assert_eq!(ev.tag, hex32(exp["tag"].as_str().unwrap()), "{entry}: tag");
    assert_eq!(
        ev.index,
        exp["index"].as_u64().unwrap() as u32,
        "{entry}: index"
    );
    assert_eq!(ev.blob, hex(exp["blob"].as_str().unwrap()), "{entry}: blob");
}

fn hex32s(v: &serde_json::Value) -> Vec<[u8; 32]> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|t| hex32(t.as_str().unwrap()))
        .collect()
}

/// Every event the adapter IDL declares.
fn idl_event_names() -> std::collections::BTreeSet<String> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../idl/protocol_adapter.json");
    let idl: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("read idl")).expect("json");
    idl["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn every_fixture_event_decodes_to_the_recorded_values() {
    let doc = fixture();
    let events = doc["events"].as_array().unwrap();
    let mut seen = std::collections::BTreeSet::new();
    for (i, e) in events.iter().enumerate() {
        let name = e["name"].as_str().unwrap();
        let entry = format!("#{i} {name} ({})", e["source"].as_str().unwrap());
        let data = base64::engine::general_purpose::STANDARD
            .decode(e["data_b64"].as_str().unwrap())
            .unwrap();
        let exp = &e["expected"];
        let decoded =
            decode_event_instruction(&data).unwrap_or_else(|err| panic!("{entry}: {err}"));
        seen.insert(name.to_string());
        let h32 = |field: &str| hex32(exp[field].as_str().unwrap());
        match (name, &decoded) {
            ("ResourcePayloadEvent", PaEvent::ResourcePayload(ev))
            | ("DiscoveryPayloadEvent", PaEvent::DiscoveryPayload(ev))
            | ("ExternalPayloadEvent", PaEvent::ExternalPayload(ev))
            | ("ApplicationPayloadEvent", PaEvent::ApplicationPayload(ev)) => {
                check_payload(ev, exp, &entry)
            }
            ("ActionExecutedEvent", PaEvent::ActionExecuted(ev)) => {
                assert_eq!(ev.action_tree_root, h32("action_tree_root"), "{entry}");
                assert_eq!(ev.nullifiers, hex32s(&exp["nullifiers"]), "{entry}");
                assert_eq!(
                    ev.consumed_logic_refs,
                    hex32s(&exp["consumed_logic_refs"]),
                    "{entry}"
                );
                assert_eq!(ev.commitments, hex32s(&exp["commitments"]), "{entry}");
                assert_eq!(
                    ev.created_logic_refs,
                    hex32s(&exp["created_logic_refs"]),
                    "{entry}"
                );
            }
            ("TransactionExecutedEvent", PaEvent::TransactionExecuted(ev)) => {
                assert_eq!(ev.transaction_id, h32("transaction_id"), "{entry}");
            }
            ("ForwarderCallExecutedEvent", PaEvent::ForwarderCallExecuted(ev)) => {
                assert_eq!(ev.forwarder, h32("forwarder"), "{entry}");
                assert_eq!(ev.input, hex(exp["input"].as_str().unwrap()), "{entry}");
                assert_eq!(ev.output, hex(exp["output"].as_str().unwrap()), "{entry}");
            }
            ("CommitmentTreeRootAddedEvent", PaEvent::CommitmentTreeRootAdded(ev)) => {
                assert_eq!(ev.root, h32("root"), "{entry}");
            }
            ("KindTableCommitmentUpdatedEvent", PaEvent::KindTableCommitmentUpdated(ev)) => {
                assert_eq!(
                    ev.kind_table_commitment,
                    h32("kind_table_commitment"),
                    "{entry}"
                );
            }
            ("LogicRefDeniedEvent", PaEvent::LogicRefDenied(ev)) => {
                assert_eq!(ev.logic_ref, h32("logic_ref"), "{entry}");
            }
            ("PausedEvent", PaEvent::Paused(ev)) | ("UnpausedEvent", PaEvent::Unpaused(ev)) => {
                assert_eq!(ev.account, h32("account"), "{entry}");
            }
            ("OwnershipTransferredEvent", PaEvent::OwnershipTransferred(ev)) => {
                assert_eq!(ev.previous_owner, h32("previous_owner"), "{entry}");
                assert_eq!(ev.new_owner, h32("new_owner"), "{entry}");
            }
            ("UpgradedEvent", PaEvent::Upgraded(ev)) => {
                assert_eq!(ev.executable_hash, h32("executable_hash"), "{entry}");
            }
            (name, other) => panic!("{entry}: decoded as {other:?}, fixture says {name}"),
        }
    }
    assert_eq!(
        seen,
        idl_event_names(),
        "the fixture covers every IDL event"
    );
}

#[test]
fn rejects_instruction_data_without_the_event_tag() {
    let doc = fixture();
    let data = base64::engine::general_purpose::STANDARD
        .decode(doc["events"][0]["data_b64"].as_str().unwrap())
        .unwrap();
    // A settle instruction's own data starts with an instruction discriminator, not the event tag.
    let err = decode_event_instruction(&data[8..]).expect_err("must reject");
    assert_eq!(
        err.to_string(),
        "instruction data does not start with the Anchor event tag"
    );
}

#[test]
fn rejects_trailing_bytes() {
    let doc = fixture();
    let mut data = base64::engine::general_purpose::STANDARD
        .decode(doc["events"][2]["data_b64"].as_str().unwrap())
        .unwrap();
    data.push(0);
    let err = decode_event_instruction(&data).expect_err("must reject");
    assert_eq!(err.to_string(), "1 trailing byte(s) after the event body");
}
