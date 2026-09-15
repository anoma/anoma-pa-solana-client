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

#[test]
fn every_fixture_event_decodes_to_the_recorded_values() {
    let doc = fixture();
    let events = doc["events"].as_array().unwrap();
    assert_eq!(events.len(), 13);
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
        match (name, &decoded) {
            ("ResourcePayloadEvent", PaEvent::ResourcePayload(ev))
            | ("DiscoveryPayloadEvent", PaEvent::DiscoveryPayload(ev))
            | ("ExternalPayloadEvent", PaEvent::ExternalPayload(ev))
            | ("ApplicationPayloadEvent", PaEvent::ApplicationPayload(ev)) => {
                check_payload(ev, exp, &entry)
            }
            ("ActionExecutedEvent", PaEvent::ActionExecuted(ev)) => {
                assert_eq!(
                    ev.action_tree_root,
                    hex32(exp["action_tree_root"].as_str().unwrap())
                );
                assert_eq!(
                    ev.action_tag_count,
                    exp["action_tag_count"].as_u64().unwrap() as u32
                );
            }
            ("TransactionExecutedEvent", PaEvent::TransactionExecuted(ev)) => {
                let tags: Vec<[u8; 32]> = exp["tags"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|t| hex32(t.as_str().unwrap()))
                    .collect();
                let refs: Vec<[u8; 32]> = exp["logic_refs"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|t| hex32(t.as_str().unwrap()))
                    .collect();
                let consumed: Vec<bool> = exp["is_consumed"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|b| b.as_bool().unwrap())
                    .collect();
                assert_eq!(ev.tags, tags, "{entry}: tags");
                assert_eq!(ev.logic_refs, refs, "{entry}: logic_refs");
                assert_eq!(ev.is_consumed, consumed, "{entry}: is_consumed");
                assert!(
                    consumed.iter().any(|c| *c) && consumed.iter().any(|c| !*c),
                    "{entry}: fixture must exercise both roles"
                );
            }
            ("ForwarderCallExecutedEvent", PaEvent::ForwarderCallExecuted(ev)) => {
                assert_eq!(ev.forwarder, hex32(exp["forwarder"].as_str().unwrap()));
                assert_eq!(ev.input, hex(exp["input"].as_str().unwrap()));
                assert_eq!(ev.output, hex(exp["output"].as_str().unwrap()));
            }
            (name, other) => panic!("{entry}: decoded as {other:?}, fixture says {name}"),
        }
    }
    assert_eq!(seen.len(), 7, "fixture covers every event type: {seen:?}");
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
