//! Cross-package fixture: verifies that the Rust crate serializes the canonical
//! WrapMessage to the bytes documented in `fixtures/wrap_message_fixture.json`.
//!
//! The TS package has a matching test (`ts/src/crossPackageFixture.test.ts`).
//! Both implementations must produce byte-identical output against the same
//! input. Run both sides to validate the package's cross-language consistency.

use anoma_pa_solana_client::wrap_message::{WrapMessage, WRAP_MESSAGE_LEN};

const FIXTURE_FORWARDER_ID: [u8; 32] = [0x01; 32];
const FIXTURE_TOKEN_MINT: [u8; 32] = [0x02; 32];
const FIXTURE_ACTION_TREE_ROOT: [u8; 32] = [0x03; 32];
const FIXTURE_AMOUNT: u64 = 1_000_000;
const FIXTURE_NONCE: u64 = 7;
const FIXTURE_DEADLINE: i64 = 1_700_000_000;

fn fixture_message() -> WrapMessage {
    WrapMessage {
        forwarder_id: FIXTURE_FORWARDER_ID,
        token_mint: FIXTURE_TOKEN_MINT,
        amount: FIXTURE_AMOUNT,
        nonce: FIXTURE_NONCE,
        deadline: FIXTURE_DEADLINE,
        action_tree_root: FIXTURE_ACTION_TREE_ROOT,
    }
}

#[test]
fn serialized_length_is_120_bytes() {
    assert_eq!(fixture_message().serialize().len(), WRAP_MESSAGE_LEN);
}

#[test]
fn serialized_hex_matches_documented_fixture() {
    let bytes = fixture_message().serialize();
    let hex = bytes
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>();
    // Cross-package fixture: this hex is the byte-for-byte canonical form. The
    // TS package must reproduce these exact bytes for the same input. Update
    // both if the WrapMessage layout changes; treat any divergence as a wire
    // incompatibility against the PA.
    let expected = concat!(
        "0101010101010101010101010101010101010101010101010101010101010101", // forwarder
        "0202020202020202020202020202020202020202020202020202020202020202", // mint
        "40420f0000000000", // amount=1_000_000 u64 LE
        "0700000000000000", // nonce=7 u64 LE
        "00f1536500000000", // deadline=1_700_000_000 i64 LE
        "0303030303030303030303030303030303030303030303030303030303030303"  // action_tree_root
    );
    assert_eq!(hex, expected);
}

#[test]
fn sha256_digest_is_deterministic() {
    let a = fixture_message().sha256_digest();
    let b = fixture_message().sha256_digest();
    assert_eq!(a, b);
    // Print so the TS side can hardcode the same expected value.
    let hex = a.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    println!("FIXTURE_SHA256_DIGEST_HEX={}", hex);
}

#[test]
fn base64_digest_is_44_chars() {
    let d = fixture_message().base64_digest();
    assert_eq!(d.len(), 44);
    println!("FIXTURE_BASE64_DIGEST={}", d);
}
