//! Cross-package fixture: every CPI event instruction in
//! `fixtures/events_fixture.json` must decode, as the generated event types,
//! to the recorded values. The TS package runs the same fixture
//! (`ts/src/events.test.ts`).

use anchor::event::EVENT_IX_TAG_LE;
use anoma_pa_solana_client::generated::protocol_adapter::events::TransactionExecuted;
use anoma_pa_solana_client::generated::protocol_adapter::parsers::Event;
use anoma_pa_solana_client::{cpi_event_discriminator, decode_cpi_event, EventDecodeError};
use base64::Engine;
use serde_json::Value;
use solana_pubkey::Pubkey;

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

fn fixture() -> Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../fixtures/events_fixture.json"
    );
    serde_json::from_str(&std::fs::read_to_string(path).expect("read fixture")).expect("json")
}

fn idl() -> Value {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/idls/protocol_adapter.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("read idl")).expect("json")
}

/// The data of the fixture's entry for `name`.
fn data_of(doc: &Value, name: &str) -> Vec<u8> {
    let entry = doc["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == name)
        .unwrap_or_else(|| panic!("the fixture has no {name}"));
    base64::engine::general_purpose::STANDARD
        .decode(entry["data_b64"].as_str().unwrap())
        .unwrap()
}

/// The IDL type definition `name`.
fn type_def<'a>(idl: &'a Value, name: &str) -> &'a Value {
    idl["types"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == name)
        .unwrap_or_else(|| panic!("the IDL declares no type {name}"))
}

/// How `{:?}` writes `value`, the fixture's form of a value of the IDL type
/// `ty`, as a field of a generated type: byte arrays and vectors as lists of
/// decimal bytes, a `u256` as its `U256LeBytes` of them, keys in base58,
/// integers in decimal.
fn debug_of(idl: &Value, ty: &Value, value: &Value) -> String {
    if let Some(name) = ty["defined"]["name"].as_str() {
        let def = &type_def(idl, name)["type"];
        assert_eq!(def["kind"], "type", "{name} is an alias");
        return debug_of(idl, &def["alias"], value);
    }
    if let Some(inner) = ty.get("vec") {
        let items: Vec<String> = value
            .as_array()
            .unwrap()
            .iter()
            .map(|v| debug_of(idl, inner, v))
            .collect();
        return format!("[{}]", items.join(", "));
    }
    let text = || value.as_str().unwrap();
    match ty.as_str() {
        Some("bool") => value.as_bool().unwrap().to_string(),
        Some("u64") => text().to_string(),
        Some("pubkey") => Pubkey::new_from_array(hex(text()).try_into().unwrap()).to_string(),
        Some("bytes") => format!("{:?}", hex(text())),
        Some("u256") => format!("U256LeBytes({:?})", hex(text())),
        _ if ty.get("array").is_some() => format!("{:?}", hex(text())),
        _ => panic!("no fixture form for the IDL type {ty}"),
    }
}

/// How `{:?}` writes the generated event type `name` holding `expected`.
fn expected_debug(idl: &Value, name: &str, expected: &Value) -> String {
    let fields: Vec<String> = type_def(idl, name)["type"]["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            let field = f["name"].as_str().unwrap();
            format!("{field}: {}", debug_of(idl, &f["type"], &expected[field]))
        })
        .collect();
    format!("{name} {{ {} }}", fields.join(", "))
}

/// `{:?}` of the generated event `event` holds.
fn debug_of_event(event: &Event) -> String {
    match event {
        Event::ActionExecuted(e) => format!("{e:?}"),
        Event::ApplicationPayload(e) => format!("{e:?}"),
        Event::CommitmentTreeRootAdded(e) => format!("{e:?}"),
        Event::DiscoveryPayload(e) => format!("{e:?}"),
        Event::ExternalPayload(e) => format!("{e:?}"),
        Event::ForwarderCallExecuted(e) => format!("{e:?}"),
        Event::Initialized(e) => format!("{e:?}"),
        Event::KindTreeRootUpdated(e) => format!("{e:?}"),
        Event::LogicRefDenied(e) => format!("{e:?}"),
        Event::OwnershipTransferred(e) => format!("{e:?}"),
        Event::Paused(e) => format!("{e:?}"),
        Event::ResourcePayload(e) => format!("{e:?}"),
        Event::TransactionExecuted(e) => format!("{e:?}"),
        Event::Unpaused(e) => format!("{e:?}"),
        Event::Upgraded(e) => format!("{e:?}"),
    }
}

#[test]
fn every_fixture_event_decodes_to_the_recorded_values() {
    let doc = fixture();
    let idl = idl();
    let mut seen = std::collections::BTreeSet::new();
    for (i, e) in doc["events"].as_array().unwrap().iter().enumerate() {
        let name = e["name"].as_str().unwrap();
        let entry = format!("#{i} {name} ({})", e["source"].as_str().unwrap());
        let data = base64::engine::general_purpose::STANDARD
            .decode(e["data_b64"].as_str().unwrap())
            .unwrap();

        let idl_event = idl["events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|ev| ev["name"] == name)
            .unwrap_or_else(|| panic!("{entry}: the IDL declares no event {name}"));
        let discriminator: Vec<u8> =
            serde_json::from_value(idl_event["discriminator"].clone()).unwrap();
        let found = cpi_event_discriminator(&data).unwrap_or_else(|err| panic!("{entry}: {err}"));
        assert_eq!(found.to_vec(), discriminator, "{entry}: discriminator");

        // The generated parser, after the event tag: the discriminator, then
        // exactly one body.
        let event = Event::try_from(&data[EVENT_IX_TAG_LE.len()..])
            .unwrap_or_else(|err| panic!("{entry}: {err}"));
        let decoded = debug_of_event(&event);
        let expected = expected_debug(&idl, name, &e["expected"]);
        println!("{entry}: {decoded}");
        assert_eq!(decoded, expected, "{entry}");
        seen.insert(name.to_string());
    }
    let idl_events: std::collections::BTreeSet<String> = idl["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(seen, idl_events, "the fixture covers every IDL event");
}

#[test]
fn a_fixture_event_decodes_as_its_type_and_no_other() {
    let doc = fixture();
    let data = data_of(&doc, "TransactionExecuted");
    let event = decode_cpi_event::<TransactionExecuted>(&data)
        .unwrap()
        .expect("a TransactionExecuted");
    println!("{event:?}");
    let other = data_of(&doc, "CommitmentTreeRootAdded");
    let result = decode_cpi_event::<TransactionExecuted>(&other).unwrap();
    assert!(
        result.is_none(),
        "CommitmentTreeRootAdded read as {result:?}"
    );
}

#[test]
fn rejects_instruction_data_without_the_event_tag() {
    let data = data_of(&fixture(), "TransactionExecuted");
    // A settle instruction's own data starts with an instruction discriminator, not the event tag.
    let result = cpi_event_discriminator(&data[EVENT_IX_TAG_LE.len()..]);
    assert!(
        matches!(result, Err(EventDecodeError::NotAnEvent)),
        "{result:?}"
    );
}

#[test]
fn rejects_trailing_bytes() {
    let mut data = data_of(&fixture(), "TransactionExecuted");
    data.push(0);
    let result = decode_cpi_event::<TransactionExecuted>(&data);
    assert!(
        matches!(result, Err(EventDecodeError::TrailingBytes)),
        "{result:?}"
    );
}
