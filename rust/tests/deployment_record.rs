//! `DeploymentRecord` reads a record in the `deployments.json` format from a
//! string, through the crate's public API alone, as another program's client
//! reads its own record.

use anoma_pa_solana_client::addresses::{DeploymentRecord, Environment};
use solana_cluster_type::ClusterType;
use solana_pubkey::Pubkey;

const DEVNET: &str = "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1";
const MAINNET: &str = "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp";
const STAGING_PROGRAM: &str = "2gH5DnTwDLLFk3oYxnbrceSFdTMtvaxQz29x5W8Etmpx";

/// The record of one entry in each environment: staging's on devnet with the
/// slot `staging_slot`, production's on mainnet-beta with slot 9.
fn record(staging_slot: &str) -> DeploymentRecord {
    DeploymentRecord::parse(&format!(
        r#"{{"staging": [{{"chainId": "{DEVNET}", "programId": "{STAGING_PROGRAM}", "deploymentSlot": {staging_slot}}}],
            "production": [{{"chainId": "{MAINNET}", "programId": "11111111111111111111111111111111", "deploymentSlot": 9}}]}}"#
    ))
}

#[test]
fn the_entry_on_the_cluster_is_its_address() {
    let record = record("7");
    println!("{record:?}");
    let staging = record.address(Environment::Staging, &ClusterType::Devnet);
    let production = record.address(Environment::Production, &ClusterType::MainnetBeta);
    println!("staging devnet: {staging:?}, production mainnet-beta: {production:?}");
    assert_eq!(staging, Some(STAGING_PROGRAM.parse().unwrap()));
    assert_eq!(production, Some(Pubkey::default()));
    assert_eq!(
        record.address(Environment::Staging, &ClusterType::MainnetBeta),
        None
    );
    assert_eq!(
        record.address(Environment::Production, &ClusterType::Testnet),
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
    println!("{record:?}");
    assert_eq!(
        record.address(Environment::Staging, &ClusterType::Devnet),
        None
    );
    assert!(record.of(Environment::Staging).is_empty());
}

#[test]
#[should_panic(expected = "chain id solana:a is no known cluster's")]
fn a_chain_id_of_no_known_cluster_is_a_panic_naming_the_entry() {
    DeploymentRecord::parse(
        r#"{"staging": [{"chainId": "solana:a", "programId": "11111111111111111111111111111111", "deploymentSlot": 1}], "production": []}"#,
    );
}

#[test]
#[should_panic(expected = "Devnet is recorded twice")]
fn a_cluster_recorded_twice_is_a_panic() {
    DeploymentRecord::parse(&format!(
        r#"{{"staging": [{{"chainId": "{DEVNET}", "programId": "11111111111111111111111111111111", "deploymentSlot": null}},
                         {{"chainId": "{DEVNET}", "programId": "11111111111111111111111111111111", "deploymentSlot": 1}}],
            "production": []}}"#
    ));
}

#[test]
#[should_panic(expected = "program id: ")]
fn a_program_id_that_does_not_parse_is_a_panic() {
    DeploymentRecord::parse(&format!(
        r#"{{"staging": [{{"chainId": "{DEVNET}", "programId": "not a key", "deploymentSlot": null}}], "production": []}}"#
    ));
}

#[test]
#[should_panic(expected = "deployments.json: not a deployment record")]
fn json_of_another_shape_is_a_panic() {
    DeploymentRecord::parse(r#"{"staging": []}"#);
}

/// A client finds the cluster behind an RPC among `clusters()` by its
/// genesis hash: every public cluster is there.
#[test]
fn the_clusters_include_every_public_cluster() {
    let clusters: Vec<ClusterType> = anoma_pa_solana_client::addresses::clusters().collect();
    println!("{clusters:?}");
    for cluster in [
        ClusterType::MainnetBeta,
        ClusterType::Devnet,
        ClusterType::Testnet,
    ] {
        assert!(clusters.contains(&cluster), "{cluster:?}");
    }
}
