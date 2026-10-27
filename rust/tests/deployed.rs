//! The recorded programs on their clusters. Run with `--ignored` at each
//! deployment and upgrade, with the Helius endpoint of each recorded cluster.

mod common;

use anoma_pa_solana_client::deployments::deployment;
use common::{cluster_of, raw_entries, ENVIRONMENTS};
use solana_cluster_type::ClusterType;
use solana_rpc_client::nonblocking::rpc_client::RpcClient;

/// The variable that holds the Helius endpoint of `cluster`.
fn endpoint_variable(cluster: ClusterType) -> &'static str {
    match cluster {
        ClusterType::Devnet => "HELIUS_DEVNET_URL",
        other => panic!("no Helius endpoint variable is named for {other:?}; name one here"),
    }
}

#[tokio::test]
#[ignore = "needs a Helius endpoint: run with --ignored at each deployment and upgrade"]
async fn the_recorded_programs_exist_and_are_executable() {
    for environment in ENVIRONMENTS {
        for entry in raw_entries(environment) {
            let chain_id = entry["chainId"].as_str().unwrap();
            let variable = endpoint_variable(cluster_of(&entry));
            let url = std::env::var(variable).unwrap_or_else(|_| {
                panic!(
                    "{variable} is unset: set it to the Helius endpoint of {chain_id} \
                     (scripts/helius.env in the umbrella repository) and run with --ignored"
                )
            });
            let recorded = deployment(environment, chain_id).unwrap_or_else(|| {
                panic!("{environment:?} records no deployment slot on {chain_id}: {entry}")
            });
            let account = RpcClient::new(url)
                .get_account(&recorded.program_id)
                .await
                .unwrap_or_else(|e| {
                    panic!(
                        "{environment:?}: {} on {chain_id}: {e}",
                        recorded.program_id
                    )
                });
            println!(
                "{environment:?}: {} on {chain_id}, owner {}, executable {}",
                recorded.program_id, account.owner, account.executable
            );
            assert!(
                account.executable,
                "{environment:?}: {} on {chain_id} is not executable",
                recorded.program_id
            );
        }
    }
}
