//! The vendored adapter IDL, which `declare_program!` generates the client
//! from, is the one committed at the adapter tag `PA_TAG.txt` names.

use std::process::Command;

const REPO: &str = "repos/anoma/solana-protocol-adapter";
const IDL_PATH: &str = "solana-pa-prototype/idl/protocol_adapter.json";

/// `gh api` of `endpoint`, with `accept` as the media type: its stdout, or a
/// panic naming `what` with gh's stderr.
fn gh_api(endpoint: &str, accept: &str, what: &str) -> Vec<u8> {
    let output = Command::new("gh")
        .args(["api", "-H", &format!("Accept: {accept}"), endpoint])
        .output()
        .unwrap_or_else(|e| panic!("run gh (the GitHub CLI) for {what}: {e}"));
    assert!(
        output.status.success(),
        "{what}: gh api {endpoint} exited with {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

#[test]
fn the_vendored_idl_is_the_one_at_its_tag() {
    let tag_file = concat!(env!("CARGO_MANIFEST_DIR"), "/../PA_TAG.txt");
    let tag = std::fs::read_to_string(tag_file).expect("read PA_TAG.txt");
    let tag = tag.trim();
    println!("adapter tag: {tag}");

    gh_api(
        &format!("{REPO}/git/ref/tags/{tag}"),
        "application/vnd.github+json",
        &format!("tag {tag} not found in anoma/solana-protocol-adapter"),
    );
    let at_tag = gh_api(
        &format!("{REPO}/contents/{IDL_PATH}?ref={tag}"),
        "application/vnd.github.raw",
        &format!("{IDL_PATH} at {tag}"),
    );
    let vendored = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/idls/protocol_adapter.json"
    ))
    .expect("read idls/protocol_adapter.json");
    println!(
        "{IDL_PATH} at {tag}: {} bytes; idls/protocol_adapter.json: {} bytes",
        at_tag.len(),
        vendored.len()
    );

    let first_difference = at_tag
        .iter()
        .zip(&vendored)
        .position(|(a, b)| a != b)
        .or((at_tag.len() != vendored.len()).then(|| at_tag.len().min(vendored.len())));
    assert_eq!(
        first_difference, None,
        "idls/protocol_adapter.json differs from {IDL_PATH} at {tag} from byte offset {first_difference:?}"
    );
}
