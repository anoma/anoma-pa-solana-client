//! The deployment record as `deployments.json` holds it, entry by entry.

use anoma_pa_solana_client::deployments::chain_id;
pub use anoma_pa_solana_client::deployments::Environment;
use serde_json::Value;
use solana_cluster_type::ClusterType;

pub const ENVIRONMENTS: [Environment; 2] = [Environment::Staging, Environment::Production];

/// `environment`'s entries in `deployments.json`, as written.
pub fn raw_entries(environment: Environment) -> Vec<Value> {
    let record: Value = serde_json::from_str(include_str!("../../deployments.json"))
        .expect("deployments.json is JSON");
    let key = match environment {
        Environment::Staging => "staging",
        Environment::Production => "production",
    };
    record[key]
        .as_array()
        .unwrap_or_else(|| panic!("deployments.json has no {key} list"))
        .clone()
}

/// The cluster `entry`'s chain id names, or a panic naming the entry.
pub fn cluster_of(entry: &Value) -> ClusterType {
    let id = entry["chainId"]
        .as_str()
        .expect("an entry's chainId is a string");
    ClusterType::STRINGS
        .iter()
        .map(|name| name.parse::<ClusterType>().unwrap())
        .find(|&cluster| chain_id(cluster).as_deref() == Some(id))
        .unwrap_or_else(|| panic!("chain id {id} is no known cluster's: {entry}"))
}
