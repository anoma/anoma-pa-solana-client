//! The adapter's deployments, as `deployments.json` records them: per
//! environment, the program on each cluster, under the cluster's CAIP-2 chain
//! id.

use serde::Deserialize;
use solana_cluster_type::ClusterType;
use solana_message::Hash;
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

/// A deployment record, as `deployments.json` holds one.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Record {
    staging: Vec<Entry>,
    production: Vec<Entry>,
}

impl Record {
    /// The record `json` holds, or a panic saying why it is none.
    pub(crate) fn parse(json: &str) -> Self {
        serde_json::from_str(json)
            .unwrap_or_else(|e| panic!("not a deployment record: {e}: {json}"))
    }

    /// The adapter `environment` records on the cluster `chain_id` names, or
    /// `None` when it records none there or records it as not yet deployed
    /// (a `null` deployment slot).
    pub(crate) fn deployment(
        &self,
        environment: Environment,
        chain_id: &str,
    ) -> Option<Deployment> {
        let entries = match environment {
            Environment::Staging => &self.staging,
            Environment::Production => &self.production,
        };
        let entry = entries.iter().find(|entry| entry.chain_id == chain_id)?;
        Some(Deployment {
            chain_id: entry.chain_id.clone(),
            program_id: entry.program_id.parse().unwrap_or_else(|e| {
                panic!(
                    "deployment record: {environment:?} program id {} on {chain_id}: {e}",
                    entry.program_id
                )
            }),
            deployment_slot: entry.deployment_slot?,
        })
    }
}

/// `deployments.json`.
pub(crate) static RECORD: LazyLock<Record> =
    LazyLock::new(|| Record::parse(include_str!("../deployments.json")));

/// The CAIP-2 chain id of the cluster whose genesis hash is `genesis_hash`:
/// `solana:` and the first 32 characters of the hash in base58 (CAIP-30).
pub fn chain_id_of_genesis_hash(genesis_hash: &Hash) -> String {
    format!("solana:{}", &genesis_hash.to_string()[..32])
}

/// The CAIP-2 chain id of `cluster`, or `None` for a cluster without a known
/// genesis hash (`Development`).
pub fn chain_id(cluster: ClusterType) -> Option<String> {
    cluster
        .get_genesis_hash()
        .map(|hash| chain_id_of_genesis_hash(&hash))
}

/// The adapter `environment` records on the cluster `chain_id` names, or
/// `None` when it records none there or records it as not yet deployed (a
/// `null` deployment slot).
pub fn deployment(environment: Environment, chain_id: &str) -> Option<Deployment> {
    RECORD.deployment(environment, chain_id)
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

    /// The record of one entry in each environment: `staging_slot` the
    /// devnet entry's slot, mainnet-beta's 9.
    fn record(staging_slot: &str) -> Record {
        Record::parse(&format!(
            r#"{{"staging": [{{"chainId": "solana:a", "programId": "2gH5DnTwDLLFk3oYxnbrceSFdTMtvaxQz29x5W8Etmpx", "deploymentSlot": {staging_slot}}}],
                "production": [{{"chainId": "solana:b", "programId": "11111111111111111111111111111111", "deploymentSlot": 9}}]}}"#
        ))
    }

    #[test]
    fn the_entry_on_the_chain_is_its_deployment() {
        let record = record("7");
        let found = record.deployment(Environment::Production, "solana:b");
        println!("{found:?}");
        assert_eq!(
            found,
            Some(Deployment {
                chain_id: "solana:b".to_string(),
                program_id: Pubkey::default(),
                deployment_slot: 9,
            })
        );
        assert_eq!(
            record
                .deployment(Environment::Staging, "solana:a")
                .map(|d| d.deployment_slot),
            Some(7)
        );
        assert_eq!(record.deployment(Environment::Staging, "solana:b"), None);
        assert_eq!(record.deployment(Environment::Production, "solana:c"), None);
    }

    /// `deployments.json` is a record as the parser reads it: a misspelled,
    /// missing or extra field fails here.
    #[test]
    fn the_committed_record_parses() {
        let record = &*RECORD;
        println!(
            "deployments.json: {} staging, {} production entries",
            record.staging.len(),
            record.production.len()
        );
    }

    /// An entry whose program is not deployed yet has a `null` slot.
    #[test]
    fn an_entry_without_a_deployment_slot_is_no_deployment() {
        assert_eq!(
            record("null").deployment(Environment::Staging, "solana:a"),
            None
        );
    }
}
