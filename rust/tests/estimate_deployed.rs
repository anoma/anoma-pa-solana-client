//! `Submission::estimate` on devnet, against the staging adapter and its
//! Groth16 verifier: a settlement devnet already executed
//! (`fixtures/devnet_settlement.json`) estimated again fails as settling it
//! again would. Run with `--ignored`, `HELIUS_DEVNET_URL` set.

use std::time::Instant;

use anoma_pa_solana_client::addresses::{protocol_adapter_address, Environment};
use anoma_pa_solana_client::estimate::Estimate;
use anoma_pa_solana_client::forwarders::Forwarders;
use anoma_pa_solana_client::generated::protocol_adapter::error::ProtocolAdapterError;
use anoma_pa_solana_client::submission::{
    read_adapter_state, read_verifier_program, Failure, Submission,
};
use anoma_rm_risc0::transaction::Transaction;
use base64::Engine;
use solana_cluster_type::ClusterType;
use solana_pubkey::Pubkey;
use solana_rpc_client::nonblocking::rpc_client::RpcClient;

#[tokio::test]
#[ignore = "needs a Helius endpoint: run with --ignored, HELIUS_DEVNET_URL set"]
async fn a_settled_devnet_transaction_estimated_again_fails_as_its_settlement_would() {
    let url = std::env::var("HELIUS_DEVNET_URL").expect(
        "HELIUS_DEVNET_URL is unset: set it to the Helius devnet endpoint \
         (scripts/helius.env in the umbrella repository)",
    );
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../fixtures/devnet_settlement.json")).unwrap();
    let input = base64::engine::general_purpose::STANDARD
        .decode(fixture["settlement_input_base64"].as_str().unwrap())
        .unwrap();
    // The settled transaction, its proof the router seal; the estimate
    // replaces the proof with a placeholder.
    let transaction: Transaction = bincode::deserialize(&input).unwrap();
    let rpc = RpcClient::new(url);
    let adapter = protocol_adapter_address(Environment::Staging, &ClusterType::Devnet).unwrap();
    // The account that paid for the settlement.
    let payer: Pubkey = fixture["payer"].as_str().unwrap().parse().unwrap();
    let state = read_adapter_state(&rpc, &adapter).await.unwrap();
    let verifier = read_verifier_program(&rpc, &state.verifier_router, state.proof_selector)
        .await
        .unwrap();
    let submission = Submission::prepare(
        &rpc,
        &Forwarders::default(),
        adapter,
        payer,
        state,
        verifier,
        transaction,
    )
    .await
    .unwrap();

    let started = Instant::now();
    let estimate = submission.estimate(&rpc).await.unwrap();
    println!(
        "estimate on devnet: {estimate:?}, in {:?}",
        started.elapsed()
    );

    assert_eq!(
        estimate,
        Estimate::Failed(Failure::Adapter(ProtocolAdapterError::PreExistingNullifier))
    );
}
