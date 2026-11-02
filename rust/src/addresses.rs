//! The adapter's deployments, as `deployments.json` records them: per
//! environment, the program on each cluster, under the cluster's CAIP-2 chain
//! id (`solana:` and the first 32 characters of its genesis hash in base58,
//! CAIP-30).

use serde::Deserialize;
use solana_cluster_type::ClusterType;
use solana_pubkey::Pubkey;
use std::sync::LazyLock;

/// The deployment environment of a recorded protocol adapter program.
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

/// The programs recorded for each environment, by cluster. `ClusterType`
/// implements neither `Hash` nor `Ord`, so a map of it is a list of pairs,
/// one per cluster.
struct DeploymentsMaps {
    staging: Vec<(ClusterType, Pubkey)>,
    production: Vec<(ClusterType, Pubkey)>,
}

impl DeploymentsMaps {
    /// The record `json` holds, or a panic naming the entry that is none.
    fn parse(json: &str) -> Self {
        let deployments: Deployments = serde_json::from_str(json)
            .unwrap_or_else(|e| panic!("deployments.json: not a deployment record: {e}: {json}"));
        Self {
            staging: to_map(Environment::Staging, deployments.staging),
            production: to_map(Environment::Production, deployments.production),
        }
    }

    fn of(&self, environment: Environment) -> &[(ClusterType, Pubkey)] {
        match environment {
            Environment::Staging => &self.staging,
            Environment::Production => &self.production,
        }
    }
}

/// `environment`'s entries by cluster: those with a deployment slot (an entry
/// whose slot is `null` is no deployment yet). An entry whose chain id is no
/// known cluster's, whose program id does not parse, or whose cluster another
/// entry records too is a panic naming it.
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

/// Every cluster `solana-cluster-type` names.
pub(crate) fn clusters() -> impl Iterator<Item = ClusterType> {
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

static DEPLOYMENTS: LazyLock<DeploymentsMaps> =
    LazyLock::new(|| DeploymentsMaps::parse(include_str!("../deployments.json")));

/// Returns a map of the protocol adapter programs recorded for the
/// environment.
pub fn protocol_adapter_deployments_map(environment: Environment) -> Vec<(ClusterType, Pubkey)> {
    DEPLOYMENTS.of(environment).to_vec()
}

/// Returns the protocol adapter program recorded for the environment on the
/// provided cluster, if any.
pub fn protocol_adapter_address(environment: Environment, cluster: &ClusterType) -> Option<Pubkey> {
    address_in(&DEPLOYMENTS, environment, cluster)
}

fn address_in(
    deployments: &DeploymentsMaps,
    environment: Environment,
    cluster: &ClusterType,
) -> Option<Pubkey> {
    deployments
        .of(environment)
        .iter()
        .find(|(recorded, _)| recorded == cluster)
        .map(|&(_, program)| program)
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

    const DEVNET: &str = "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1";
    const MAINNET: &str = "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp";

    /// The record of one entry in each environment: staging's on devnet with
    /// the slot `staging_slot`, production's on mainnet-beta with slot 9.
    fn record(staging_slot: &str) -> DeploymentsMaps {
        DeploymentsMaps::parse(&format!(
            r#"{{"staging": [{{"chainId": "{DEVNET}", "programId": "2gH5DnTwDLLFk3oYxnbrceSFdTMtvaxQz29x5W8Etmpx", "deploymentSlot": {staging_slot}}}],
                "production": [{{"chainId": "{MAINNET}", "programId": "11111111111111111111111111111111", "deploymentSlot": 9}}]}}"#
        ))
    }

    #[test]
    fn the_entry_on_the_cluster_is_its_address() {
        let record = record("7");
        let staging = address_in(&record, Environment::Staging, &ClusterType::Devnet);
        let production = address_in(&record, Environment::Production, &ClusterType::MainnetBeta);
        println!("staging devnet: {staging:?}, production mainnet-beta: {production:?}");
        assert_eq!(
            staging,
            Some(
                "2gH5DnTwDLLFk3oYxnbrceSFdTMtvaxQz29x5W8Etmpx"
                    .parse()
                    .unwrap()
            )
        );
        assert_eq!(production, Some(Pubkey::default()));
        assert_eq!(
            address_in(&record, Environment::Staging, &ClusterType::MainnetBeta),
            None
        );
        assert_eq!(
            address_in(&record, Environment::Production, &ClusterType::Testnet),
            None
        );
        assert_eq!(
            record.of(Environment::Production),
            [(ClusterType::MainnetBeta, Pubkey::default())]
        );
    }

    /// An entry whose program is not deployed yet has a `null` slot.
    #[test]
    fn an_entry_without_a_deployment_slot_is_no_deployment() {
        let record = record("null");
        assert_eq!(
            address_in(&record, Environment::Staging, &ClusterType::Devnet),
            None
        );
        assert!(record.of(Environment::Staging).is_empty());
    }

    #[test]
    #[should_panic(expected = "chain id solana:a is no known cluster's")]
    fn a_chain_id_of_no_known_cluster_is_a_panic_naming_the_entry() {
        DeploymentsMaps::parse(
            r#"{"staging": [{"chainId": "solana:a", "programId": "11111111111111111111111111111111", "deploymentSlot": 1}], "production": []}"#,
        );
    }

    #[test]
    #[should_panic(expected = "Devnet is recorded twice")]
    fn a_cluster_recorded_twice_is_a_panic() {
        DeploymentsMaps::parse(&format!(
            r#"{{"staging": [{{"chainId": "{DEVNET}", "programId": "11111111111111111111111111111111", "deploymentSlot": null}},
                             {{"chainId": "{DEVNET}", "programId": "11111111111111111111111111111111", "deploymentSlot": 1}}],
                "production": []}}"#
        ));
    }

    #[test]
    #[should_panic(expected = "program id: ")]
    fn a_program_id_that_does_not_parse_is_a_panic() {
        DeploymentsMaps::parse(&format!(
            r#"{{"staging": [{{"chainId": "{DEVNET}", "programId": "not a key", "deploymentSlot": null}}], "production": []}}"#
        ));
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
        let record = &*DEPLOYMENTS;
        println!(
            "deployments.json: staging {:?}, production {:?}",
            record.staging, record.production
        );
    }
}
