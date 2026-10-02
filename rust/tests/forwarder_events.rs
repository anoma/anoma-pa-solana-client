//! The SPL Token Forwarder's CPI events: each one decodes from instruction
//! data built by hand (event tag, discriminator, Borsh body), malformed data is
//! rejected with the specific error, and the decoder's discriminators are the
//! ones `idl/spl_token_forwarder.json` declares. The TS package runs the same
//! checks (`ts/src/events.test.ts`).

use anoma_pa_solana_client::events::{
    decode_forwarder_event_instruction, EmergencyCallerSetEvent, EmergencyWithdrawEvent,
    EventDecodeError, ForwarderEvent, InitializedEvent, UnwrappedEvent, WrappedEvent, EVENT_IX_TAG,
};
use anoma_pa_solana_client::{anchor_event_disc, ANCHOR_DISCRIMINATOR_LEN};

const MINT: [u8; 32] = [1; 32];
const USER: [u8; 32] = [2; 32];
const OTHER: [u8; 32] = [3; 32];
const ROOT: [u8; 32] = [4; 32];

/// Event tag, discriminator of `name`, then the body.
fn event_ix(name: &str, body: &[&[u8]]) -> Vec<u8> {
    let mut data = EVENT_IX_TAG.to_vec();
    data.extend_from_slice(&anchor_event_disc(name));
    for part in body {
        data.extend_from_slice(part);
    }
    data
}

/// Every event with its hand-built instruction data and the value it must decode to.
fn cases() -> Vec<(&'static str, Vec<u8>, ForwarderEvent)> {
    vec![
        (
            "Wrapped",
            event_ix(
                "Wrapped",
                &[
                    &MINT,
                    &USER,
                    &1_000_000u64.to_le_bytes(),
                    &77u64.to_le_bytes(),
                    &ROOT,
                ],
            ),
            ForwarderEvent::Wrapped(WrappedEvent {
                token_mint: MINT,
                from: USER,
                amount: 1_000_000,
                nonce: 77,
                action_tree_root: ROOT,
            }),
        ),
        (
            "Unwrapped",
            event_ix("Unwrapped", &[&MINT, &USER, &u64::MAX.to_le_bytes()]),
            ForwarderEvent::Unwrapped(UnwrappedEvent {
                token_mint: MINT,
                to: USER,
                amount: u64::MAX,
            }),
        ),
        (
            "EmergencyCallerSet",
            event_ix("EmergencyCallerSet", &[&USER, &OTHER]),
            ForwarderEvent::EmergencyCallerSet(EmergencyCallerSetEvent {
                emergency_caller: USER,
                set_by: OTHER,
            }),
        ),
        (
            "EmergencyWithdraw",
            event_ix(
                "EmergencyWithdraw",
                &[&MINT, &USER, &42u64.to_le_bytes(), &OTHER],
            ),
            ForwarderEvent::EmergencyWithdraw(EmergencyWithdrawEvent {
                token_mint: MINT,
                to: USER,
                amount: 42,
                caller: OTHER,
            }),
        ),
        (
            "Initialized",
            event_ix("Initialized", &[&2u64.to_le_bytes()]),
            ForwarderEvent::Initialized(InitializedEvent { version: 2 }),
        ),
    ]
}

/// Every event the forwarder IDL declares, with its discriminator.
fn idl_events() -> Vec<(String, [u8; ANCHOR_DISCRIMINATOR_LEN])> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../idl/spl_token_forwarder.json"
    );
    let idl: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("read idl")).expect("json");
    idl["events"]
        .as_array()
        .expect("events array")
        .iter()
        .map(|e| {
            let disc: Vec<u8> = e["discriminator"]
                .as_array()
                .expect("discriminator array")
                .iter()
                .map(|b| b.as_u64().expect("byte") as u8)
                .collect();
            (
                e["name"].as_str().expect("name").to_string(),
                disc.try_into().expect("8-byte discriminator"),
            )
        })
        .collect()
}

#[test]
fn every_forwarder_event_decodes_from_hand_built_bytes() {
    for (name, data, expected) in cases() {
        let decoded =
            decode_forwarder_event_instruction(&data).unwrap_or_else(|err| panic!("{name}: {err}"));
        assert_eq!(decoded, expected, "{name}");
    }
}

#[test]
fn decoder_covers_every_idl_event_with_the_idl_discriminator() {
    let idl = idl_events();
    let mut idl_names: Vec<&str> = idl.iter().map(|(n, _)| n.as_str()).collect();
    let mut decoded_names: Vec<&str> = cases().iter().map(|(n, _, _)| *n).collect();
    idl_names.sort();
    decoded_names.sort();
    assert_eq!(
        decoded_names, idl_names,
        "the decoder covers every IDL event"
    );
    for (name, disc) in &idl {
        assert_eq!(&anchor_event_disc(name), disc, "{name}: IDL discriminator");
    }
}

#[test]
fn rejects_instruction_data_without_the_event_tag() {
    let (_, data, _) = &cases()[0];
    let err = decode_forwarder_event_instruction(&data[8..]).expect_err("must reject");
    assert_eq!(err, EventDecodeError::NotAnEvent);
}

#[test]
fn rejects_an_unknown_discriminator() {
    // A PA event is not a forwarder event.
    let data = event_ix("TransactionExecutedEvent", &[&ROOT]);
    let err = decode_forwarder_event_instruction(&data).expect_err("must reject");
    assert_eq!(
        err,
        EventDecodeError::UnknownDiscriminator(anchor_event_disc("TransactionExecutedEvent"))
    );
}

#[test]
fn rejects_a_truncated_body() {
    for (name, data, _) in cases() {
        let err =
            decode_forwarder_event_instruction(&data[..data.len() - 1]).expect_err("must reject");
        assert!(
            matches!(err, EventDecodeError::Truncated { .. }),
            "{name}: got {err:?}"
        );
    }
    let (_, wrapped, _) = &cases()[0];
    let err =
        decode_forwarder_event_instruction(&wrapped[..wrapped.len() - 1]).expect_err("must reject");
    assert_eq!(
        err,
        EventDecodeError::Truncated {
            field: "action_tree_root"
        }
    );
}

#[test]
fn rejects_trailing_bytes() {
    for (name, mut data, _) in cases() {
        data.push(0);
        let err = decode_forwarder_event_instruction(&data).expect_err("must reject");
        assert_eq!(err, EventDecodeError::TrailingBytes(1), "{name}");
    }
}
