//! Deployment records, as a `deployments.json` holds one: per environment
//! (`staging`, `production`), a list of entries, each a program (`programId`)
//! on a cluster under the cluster's CAIP-2 chain id (`chainId`: `solana:` and
//! the first 32 characters of its genesis hash in base58, CAIP-30), with the
//! slot of its first deployment (`deploymentSlot`, `null` until the program is
//! deployed). [`DeploymentRecord`] reads any record in this format; this
//! crate's `deployments.json` records the adapter.

use serde::Deserialize;
use solana_cluster_type::ClusterType;
use solana_pubkey::Pubkey;
use std::sync::LazyLock;

/// The deployment environment of a recorded program.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Environment {
    /// Devnet.
    Staging,
    /// Mainnet-beta.
    Production,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DeploymentEntry {
    chain_id: String,
    program_id: String,
    /// The slot of the program's first deployment, where its history starts;
    /// `null` until the program is deployed.
    deployment_slot: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Deployments {
    staging: Vec<DeploymentEntry>,
    production: Vec<DeploymentEntry>,
}

/// The deployed programs a deployment record holds for each environment, by
/// cluster. `ClusterType` implements neither `Hash` nor `Ord`, so a map of it
/// is a list of pairs, one per cluster.
#[derive(Debug)]
pub struct DeploymentRecord {
    staging: Vec<(ClusterType, Pubkey)>,
    production: Vec<(ClusterType, Pubkey)>,
}

impl DeploymentRecord {
    /// The record `json` holds. An entry whose slot is `null` is no
    /// deployment. JSON that is not a record in this format, or an entry whose
    /// chain id is no known cluster's, whose program id does not parse, or
    /// whose cluster another entry of its environment records too, is a panic
    /// naming it.
    pub fn parse(json: &str) -> Self {
        let deployments: Deployments = serde_json::from_str(json)
            .unwrap_or_else(|e| panic!("deployments.json: not a deployment record: {e}: {json}"));
        Self {
            staging: to_map(Environment::Staging, deployments.staging),
            production: to_map(Environment::Production, deployments.production),
        }
    }

    /// The deployed programs `environment` records, by cluster.
    pub fn of(&self, environment: Environment) -> &[(ClusterType, Pubkey)] {
        match environment {
            Environment::Staging => &self.staging,
            Environment::Production => &self.production,
        }
    }

    /// The deployed program `environment` records on `cluster`, if any.
    pub fn address(&self, environment: Environment, cluster: &ClusterType) -> Option<Pubkey> {
        self.of(environment)
            .iter()
            .find(|(recorded, _)| recorded == cluster)
            .map(|&(_, program)| program)
    }
}

/// `environment`'s deployed programs by cluster, as
/// [`DeploymentRecord::parse`] reads them.
fn to_map(environment: Environment, entries: Vec<DeploymentEntry>) -> Vec<(ClusterType, Pubkey)> {
    let mut seen = Vec::new();
    let mut map = Vec::new();
    for entry in entries {
        let cluster = cluster_of_chain_id(&entry.chain_id).unwrap_or_else(|| {
            panic!(
                "deployments.json: {environment:?} entry {entry:?}: chain id {} is no known \
                 cluster's",
                entry.chain_id
            )
        });
        assert!(
            !seen.contains(&cluster),
            "deployments.json: {environment:?} entry {entry:?}: {cluster:?} is recorded twice"
        );
        seen.push(cluster);
        let program: Pubkey = entry.program_id.parse().unwrap_or_else(|e| {
            panic!("deployments.json: {environment:?} entry {entry:?}: program id: {e}")
        });
        if entry.deployment_slot.is_some() {
            map.push((cluster, program));
        }
    }
    map
}

/// Every cluster `solana-cluster-type` names; a client finds the cluster behind
/// an RPC among them by its genesis hash.
pub fn clusters() -> impl Iterator<Item = ClusterType> {
    ClusterType::STRINGS
        .iter()
        .map(|name| name.parse().expect("a ClusterType name parses"))
}

/// The CAIP-2 chain id of `cluster`, or `None` for a cluster without a known
/// genesis hash (`Development`).
fn chain_id(cluster: ClusterType) -> Option<String> {
    cluster
        .get_genesis_hash()
        .map(|hash| format!("solana:{}", &hash.to_string()[..32]))
}

/// The cluster whose CAIP-2 chain id is `id`, if any.
fn cluster_of_chain_id(id: &str) -> Option<ClusterType> {
    clusters().find(|&cluster| chain_id(cluster).as_deref() == Some(id))
}

static DEPLOYMENTS: LazyLock<DeploymentRecord> =
    LazyLock::new(|| DeploymentRecord::parse(include_str!("../deployments.json")));

/// Returns a map of the protocol adapter programs recorded for the
/// environment.
pub fn protocol_adapter_deployments_map(environment: Environment) -> Vec<(ClusterType, Pubkey)> {
    DEPLOYMENTS.of(environment).to_vec()
}

/// Returns the protocol adapter program recorded for the environment on the
/// provided cluster, if any.
pub fn protocol_adapter_address(environment: Environment, cluster: &ClusterType) -> Option<Pubkey> {
    DEPLOYMENTS.address(environment, cluster)
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
            assert_eq!(cluster_of_chain_id(expected), Some(cluster), "{expected}");
        }
        assert_eq!(chain_id(ClusterType::Development), None);
        assert_eq!(
            cluster_of_chain_id("solana:11111111111111111111111111111111"),
            None
        );
    }

    /// The client is generated from the IDL of the program staging runs.
    #[test]
    fn the_staging_devnet_program_is_the_vendored_idls_address() {
        let devnet = protocol_adapter_address(Environment::Staging, &ClusterType::Devnet);
        println!(
            "staging devnet: {devnet:?}, IDL: {}",
            crate::generated::protocol_adapter::ID
        );
        assert_eq!(devnet, Some(crate::generated::protocol_adapter::ID));
    }

    /// `deployments.json` is a record as the parser reads it: a misspelled,
    /// missing or extra field, or an entry the loader refuses, fails here.
    #[test]
    fn the_committed_record_parses() {
        println!("deployments.json: {:?}", *DEPLOYMENTS);
    }
}
