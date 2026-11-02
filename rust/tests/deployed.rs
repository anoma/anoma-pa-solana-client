//! The recorded programs on their clusters. Run with `--ignored` at each
//! deployment and upgrade, with the Helius endpoint of each recorded cluster.

use anoma_pa_solana_client::addresses::{protocol_adapter_deployments_map, Environment};
use anoma_pa_solana_client::contract::protocol_adapter;
use solana_cluster_type::ClusterType;
use solana_rpc_client::nonblocking::rpc_client::RpcClient;

/// The variable that holds the Helius endpoint of `cluster`.
fn endpoint_variable(cluster: ClusterType) -> &'static str {
    match cluster {
        ClusterType::Devnet => "HELIUS_DEVNET_URL",
        other => panic!("no Helius endpoint variable is named for {other:?}; name one here"),
    }
}

/// Each recorded program is executable on its cluster, and the environment's
/// instance over that cluster's RPC is at it.
#[tokio::test]
#[ignore = "needs a Helius endpoint: run with --ignored at each deployment and upgrade"]
async fn the_recorded_programs_exist_and_are_executable() {
    for environment in [Environment::Staging, Environment::Production] {
        for (cluster, program) in protocol_adapter_deployments_map(environment) {
            let variable = endpoint_variable(cluster);
            let url = std::env::var(variable).unwrap_or_else(|_| {
                panic!(
                    "{variable} is unset: set it to the Helius endpoint of {cluster:?} \
                     (scripts/helius.env in the umbrella repository) and run with --ignored"
                )
            });
            let rpc = RpcClient::new(url);
            let account = rpc
                .get_account(&program)
                .await
                .unwrap_or_else(|e| panic!("{environment:?}: {program} on {cluster:?}: {e}"));
            println!(
                "{environment:?}: {program} on {cluster:?}, owner {}, executable {}",
                account.owner, account.executable
            );
            assert!(
                account.executable,
                "{environment:?}: {program} on {cluster:?} is not executable"
            );
            let instance = protocol_adapter(&rpc, environment)
                .await
                .unwrap_or_else(|e| panic!("{environment:?} on {cluster:?}: {e}"));
            assert_eq!(
                instance.address(),
                program,
                "{environment:?}: the instance on {cluster:?} is not at the recorded program"
            );
        }
    }
}
