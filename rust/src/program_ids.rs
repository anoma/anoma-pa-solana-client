//! The adapter's program id and its deployment's settlement lookup table.
//!
//! Devnet defaults are baked in as `pub const` values. For mainnet or alternative
//! cluster deployments, integrators should override at the call site rather than
//! relying on these constants.

use solana_pubkey::{pubkey, Pubkey};

/// Anoma Protocol Adapter program ID (devnet/localnet default).
pub const PA_PROGRAM_ID: Pubkey = pubkey!("5zeqkB3kc9fd1RvaXB2GeMB53Jgf98QJtaFK38e6tTsc");

/// The devnet V2 deployment's settlement lookup table: the accounts every
/// settlement carries that are fixed for the deployment. Settle transactions
/// are v0 messages compiled against it. The adapter repo's `lookup-table`
/// command creates one per deployment; its operations runbook lists the keys
/// and its deployment record names the table.
pub const SETTLE_LOOKUP_TABLE: Pubkey = pubkey!("4UFsq2ks2DcC29ErmEeHxqXWKLRWo26vs4W65S89bpWn");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pa_program_id_is_the_devnet_v2_adapter() {
        assert_eq!(
            PA_PROGRAM_ID.to_string(),
            "5zeqkB3kc9fd1RvaXB2GeMB53Jgf98QJtaFK38e6tTsc"
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
