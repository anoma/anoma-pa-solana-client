//! Solana program identifiers used by AnomaPay.
//!
//! Devnet defaults are baked in as `pub const` values. For mainnet or alternative
//! cluster deployments, integrators should override at the call site rather than
//! relying on these constants.

use solana_program::{pubkey, pubkey::Pubkey};

/// Anoma Protocol Adapter program ID (devnet/localnet default).
pub const PA_PROGRAM_ID: Pubkey = pubkey!("28Hvr1YFv2ouGN2fS99aF3ZzYXzkncJVVaHcZNhquLFT");

/// SPL Token Forwarder program ID (devnet/localnet default).
pub const FORWARDER_PROGRAM_ID: Pubkey = pubkey!("3cLKSYBijunpCc2F2gzizUkhYtyFrLr4RVdNiaK79b48");

/// The devnet V2 deployment's settlement lookup table: the accounts every
/// settlement carries that are fixed for the deployment. Settle transactions
/// are v0 messages compiled against it. The adapter repo's `lookup-table`
/// command creates one per deployment; its operations runbook lists the keys
/// and its deployment record names the table.
pub const SETTLE_LOOKUP_TABLE: Pubkey = pubkey!("CKAMrsJSf1SDgsmaM7hsKEwmi2efQsoCAuMNf4msGRSW");

/// Solana's native ed25519 signature-verification program. Used to carry verified
/// wrap-authorization signatures into the settle transaction.
pub const ED25519_PROGRAM_ID: Pubkey = pubkey!("Ed25519SigVerify111111111111111111111111111");

/// Solana's `Instructions` sysvar (used by the forwarder to introspect the
/// ed25519-verify instruction at `ed25519_ix_index`).
pub const INSTRUCTIONS_SYSVAR_ID: Pubkey = pubkey!("Sysvar1nstructions1111111111111111111111111");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pa_program_id_is_the_devnet_v2_adapter() {
        assert_eq!(
            PA_PROGRAM_ID.to_string(),
            "28Hvr1YFv2ouGN2fS99aF3ZzYXzkncJVVaHcZNhquLFT"
        );
    }

    #[test]
    fn pa_program_id_matches_the_vendored_idl() {
        // The IDL is regenerated from the paired adapter commit; the constant
        // and the IDL's `address` must name the same deployment.
        let idl: serde_json::Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../idl/protocol_adapter.json"
        )))
        .unwrap();
        assert_eq!(idl["address"].as_str().unwrap(), PA_PROGRAM_ID.to_string());
    }
}
