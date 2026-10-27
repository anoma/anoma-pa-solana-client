//! Checks of the deployment record itself. They need no cluster and hold
//! everywhere, so they run on every push.

mod common;

use anoma_pa_solana_client::generated::protocol_adapter;
use common::{cluster_of, raw_entries, ENVIRONMENTS};
use solana_cluster_type::ClusterType;
use solana_pubkey::Pubkey;
use std::collections::HashSet;

#[test]
fn every_chain_id_is_a_known_clusters() {
    for environment in ENVIRONMENTS {
        for entry in raw_entries(environment) {
            let cluster = cluster_of(&entry);
            println!("{environment:?}: {} is {cluster:?}", entry["chainId"]);
        }
    }
}

#[test]
fn no_chain_id_repeats_within_an_environment() {
    for environment in ENVIRONMENTS {
        let mut seen = HashSet::new();
        for entry in raw_entries(environment) {
            let id = entry["chainId"].to_string();
            assert!(
                seen.insert(id.clone()),
                "{environment:?} records {id} twice"
            );
        }
    }
}

#[test]
fn every_program_id_parses() {
    for environment in ENVIRONMENTS {
        for entry in raw_entries(environment) {
            let id = entry["programId"]
                .as_str()
                .unwrap_or_else(|| panic!("{environment:?}: programId is no string: {entry}"));
            let program: Pubkey = id
                .parse()
                .unwrap_or_else(|e| panic!("{environment:?}: program id {id}: {e}"));
            println!("{environment:?}: {program}");
        }
    }
}

/// The client is generated from the IDL of the program staging runs.
#[test]
fn the_staging_devnet_program_is_the_vendored_idls_address() {
    let entries = raw_entries(common::Environment::Staging);
    let devnet: Vec<_> = entries
        .iter()
        .filter(|entry| cluster_of(entry) == ClusterType::Devnet)
        .collect();
    assert_eq!(devnet.len(), 1, "staging's devnet entries: {devnet:?}");
    let idl: serde_json::Value =
        serde_json::from_str(include_str!("../idls/protocol_adapter.json")).unwrap();
    println!(
        "staging devnet: {}, IDL: {}",
        devnet[0]["programId"], idl["address"]
    );
    assert_eq!(devnet[0]["programId"], idl["address"]);
    assert_eq!(
        devnet[0]["programId"].as_str().unwrap(),
        protocol_adapter::ID.to_string()
    );
}
