//! The vendored IDLs, which `declare_program!` generates the client from, are
//! the ones committed at the adapter tag `PA_TAG.txt` names.

use std::process::Command;

const REPO: &str = "repos/anoma/solana-protocol-adapter";

/// Each vendored IDL in `idls/`, and its path in the adapter repository.
const IDLS: [(&str, &str); 2] = [
    (
        "protocol_adapter.json",
        "solana-pa-prototype/idl/protocol_adapter.json",
    ),
    (
        "verifier_router.json",
        "solana-pa-prototype/idls/verifier_router.json",
    ),
];

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
        &format!("look up tag {tag} in anoma/solana-protocol-adapter"),
    );
    for (file, path) in IDLS {
        let at_tag = gh_api(
            &format!("{REPO}/contents/{path}?ref={tag}"),
            "application/vnd.github.raw",
            &format!("{path} at {tag}"),
        );
        let vendored = std::fs::read(format!("{}/idls/{file}", env!("CARGO_MANIFEST_DIR")))
            .unwrap_or_else(|e| panic!("read idls/{file}: {e}"));
        println!(
            "{path} at {tag}: {} bytes; idls/{file}: {} bytes",
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
            "idls/{file} differs from {path} at {tag} from byte offset {first_difference:?}"
        );
    }
}
