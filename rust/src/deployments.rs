//! The adapter's deployments, as `deployments.json` records them: per
//! environment, the program on each cluster, under the cluster's CAIP-2 chain
//! id.

use serde::Deserialize;
use solana_cluster_type::ClusterType;
use solana_pubkey::Pubkey;
use std::sync::LazyLock;

/// The deployment environment of a recorded adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Environment {
    /// Devnet.
    Staging,
    /// Mainnet-beta.
    Production,
}

/// A recorded adapter deployment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Deployment {
    /// The cluster's CAIP-2 chain id (`chain_id`).
    pub chain_id: String,
    /// The adapter program.
    pub program_id: Pubkey,
    /// The slot of the program's first deployment, where its history starts.
    pub deployment_slot: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Entry {
    chain_id: String,
    program_id: String,
    deployment_slot: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    staging: Vec<Entry>,
    production: Vec<Entry>,
}

static RECORD: LazyLock<Record> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../deployments.json"))
        .unwrap_or_else(|e| panic!("deployments.json is not a deployment record: {e}"))
});

/// The CAIP-2 chain id of `cluster`: `solana:` and the first 32 characters of
/// its genesis hash in base58 (CAIP-30), or `None` for a cluster without a
/// known genesis hash (`Development`).
pub fn chain_id(cluster: ClusterType) -> Option<String> {
    cluster
        .get_genesis_hash()
        .map(|hash| format!("solana:{}", &hash.to_string()[..32]))
}

/// The adapter `environment` records on the cluster `chain_id` names, or
/// `None` when it records none there or records it as not yet deployed (a
/// `null` deployment slot).
pub fn deployment(environment: Environment, chain_id: &str) -> Option<Deployment> {
    match environment {
        Environment::Staging => find(&RECORD.staging, chain_id),
        Environment::Production => find(&RECORD.production, chain_id),
    }
}

fn find(entries: &[Entry], chain_id: &str) -> Option<Deployment> {
    let entry = entries.iter().find(|entry| entry.chain_id == chain_id)?;
    Some(Deployment {
        chain_id: entry.chain_id.clone(),
        program_id: entry.program_id.parse().unwrap_or_else(|e| {
            panic!(
                "deployments.json: program id {} on {chain_id}: {e}",
                entry.program_id
            )
        }),
        deployment_slot: entry.deployment_slot?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The chain ids CAIP-30 gives the three public clusters.
    #[test]
    fn the_chain_ids_are_caip30s() {
        for (cluster, expected) in [
            (
                ClusterType::MainnetBeta,
                "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp",
            ),
            (
                ClusterType::Devnet,
                "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1",
            ),
            (
                ClusterType::Testnet,
                "solana:4uhcVJyU9pJkvQyS88uRDiswHXSCkY3z",
            ),
        ] {
            let id = chain_id(cluster);
            println!("{cluster:?}: {id:?}");
            assert_eq!(id.as_deref(), Some(expected), "{cluster:?}");
        }
        assert_eq!(chain_id(ClusterType::Development), None);
    }

    fn entries(json: &str) -> Vec<Entry> {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn the_entry_on_the_chain_is_its_deployment() {
        let entries = entries(
            r#"[{"chainId": "solana:a", "programId": "2gH5DnTwDLLFk3oYxnbrceSFdTMtvaxQz29x5W8Etmpx", "deploymentSlot": 7},
                {"chainId": "solana:b", "programId": "11111111111111111111111111111111", "deploymentSlot": 9}]"#,
        );
        let found = find(&entries, "solana:b");
        println!("{found:?}");
        assert_eq!(
            found,
            Some(Deployment {
                chain_id: "solana:b".to_string(),
                program_id: Pubkey::default(),
                deployment_slot: 9,
            })
        );
        assert_eq!(find(&entries, "solana:c"), None);
    }

    /// An entry whose program is not deployed yet has a `null` slot.
    #[test]
    fn an_entry_without_a_deployment_slot_is_no_deployment() {
        let entries = entries(
            r#"[{"chainId": "solana:a", "programId": "11111111111111111111111111111111", "deploymentSlot": null}]"#,
        );
        assert_eq!(find(&entries, "solana:a"), None);
    }
}
