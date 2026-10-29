//! The adapter's settlement input: a proven ARM transaction whose aggregation
//! proof is re-encoded as the verifier-router seal the adapter decodes,
//! serialized with bincode.

use std::fmt;

use anoma_rm_risc0::proving_system::encode_seal;
use anoma_rm_risc0::transaction::Transaction;
use anoma_rm_risc0::utils::words_to_bytes;

use crate::constants::MOCK_SELECTOR;
use crate::external_call::SolanaExternalCall;
use risc0_zkvm::sha::{Digest, Digestible};
use risc0_zkvm::InnerReceipt;

/// The verifier parameters of a Groth16 receipt that holds a claim digest
/// instead of a proof, as pa-testkit's local prover produces it.
const MOCK_VERIFIER_PARAMETERS: Digest = Digest::new([u32::MAX; 8]);

/// The bytes of the router seal a proof becomes: the selector, then a Groth16
/// proof (a: 64 bytes, b: 128, c: 64).
const ROUTER_SEAL_LEN: usize = 4 + 64 + 128 + 64;

#[derive(Debug, PartialEq, Eq)]
pub enum SettlementInputError {
    /// The transaction carries no aggregation proof.
    NoAggregation,
    /// The aggregation proof is not a bincode-encoded risc0 receipt.
    Receipt(String),
    /// An external payload blob is not a `SolanaExternalCall`.
    ExternalCall(String),
    /// arm could not encode the receipt as a seal.
    Seal(String),
    /// The transaction did not serialize.
    Serialize(String),
}

impl fmt::Display for SettlementInputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoAggregation => write!(f, "the transaction carries no aggregation proof"),
            Self::Receipt(e) => write!(f, "the aggregation proof is not a risc0 receipt: {e}"),
            Self::ExternalCall(e) => write!(f, "an external payload is not an external call: {e}"),
            Self::Seal(e) => write!(f, "encoding the seal: {e}"),
            Self::Serialize(e) => write!(f, "serializing the transaction: {e}"),
        }
    }
}

impl std::error::Error for SettlementInputError {}

/// `tx` as the adapter settles it: its aggregation proof re-encoded as the
/// router seal (selector ‖ Groth16 a, b, c), the transaction serialized with
/// bincode.
pub fn settlement_input(tx: Transaction) -> Result<Vec<u8>, SettlementInputError> {
    bincode::serialize(&settlement_transaction(tx)?)
        .map_err(|e| SettlementInputError::Serialize(e.to_string()))
}

/// The length of `settlement_input` of `tx` once proven, known before it is:
/// the settlement input carries the router seal, whose length is fixed, in
/// place of `tx`'s aggregation proof (none yet, or the receipt).
pub fn proven_settlement_input_len(tx: &Transaction) -> Result<usize, SettlementInputError> {
    let mut proven = tx.clone();
    proven
        .aggregation
        .as_mut()
        .ok_or(SettlementInputError::NoAggregation)?
        .proof = vec![0; ROUTER_SEAL_LEN];
    bincode::serialized_size(&proven)
        .map(|size| size as usize)
        .map_err(|e| SettlementInputError::Serialize(e.to_string()))
}

/// `tx` as `simulate_execute` takes it: a proven transaction as
/// `settlement_input` encodes it; an unproven one (its aggregation proof
/// empty) with `selector`, the adapter's proof selector, as its proof, which
/// the adapter checks even when it skips verifying the proof, as pa-evm's
/// `simulateExecute` reads an unproven transaction's proof selector.
pub fn simulation_input(
    mut tx: Transaction,
    selector: [u8; 4],
) -> Result<Vec<u8>, SettlementInputError> {
    let aggregation = tx
        .aggregation
        .as_mut()
        .ok_or(SettlementInputError::NoAggregation)?;
    if !aggregation.proof.is_empty() {
        return settlement_input(tx);
    }
    aggregation.proof = selector.to_vec();
    bincode::serialize(&tx).map_err(|e| SettlementInputError::Serialize(e.to_string()))
}

/// `tx` with its aggregation proof re-encoded as the router seal: the
/// transaction the settlement input serializes, for a caller that derives
/// more from it (fixture-gen's tampered variants).
pub fn settlement_transaction(mut tx: Transaction) -> Result<Transaction, SettlementInputError> {
    let aggregation = tx
        .aggregation
        .as_mut()
        .ok_or(SettlementInputError::NoAggregation)?;
    aggregation.proof = router_seal(&aggregation.proof)?;
    Ok(tx)
}

/// The resources a settlement of a transaction touches, in the form
/// `plan_settlement` takes them: the consumed resources' nullifiers and the
/// roots their proofs were made against, and the created commitments, each in
/// the aggregation instance's order.
#[derive(Debug, PartialEq, Eq)]
pub struct SettledResources {
    pub nullifiers: Vec<[u8; 32]>,
    pub consumed_roots: Vec<[u8; 32]>,
    pub created: Vec<[u8; 32]>,
}

/// The resources a settlement of `tx` consumes and creates, read from its
/// aggregation instance, the statement the adapter settles.
pub fn settled_resources(tx: &Transaction) -> Result<SettledResources, SettlementInputError> {
    let actions = &tx
        .aggregation
        .as_ref()
        .ok_or(SettlementInputError::NoAggregation)?
        .instance
        .actions;
    let resources = || actions.iter().flat_map(|action| &action.resource_instances);
    let tags = |consumed: bool| {
        resources()
            .filter(move |r| r.is_consumed() == consumed)
            .map(|r| r.tag().into())
            .collect()
    };
    Ok(SettledResources {
        nullifiers: tags(true),
        consumed_roots: resources()
            .filter_map(|r| r.commitment_tree_root())
            .map(Into::into)
            .collect(),
        created: tags(false),
    })
}

/// The external calls a settlement of `tx` runs, in the order the adapter runs
/// them: for each action, its resources' calls in the order the action lists
/// its resources, each resource's in the order its external payload lists them.
/// The proof commits each call; the accounts of its CPI segment are the
/// submitter's to supply.
pub fn external_calls(tx: &Transaction) -> Result<Vec<SolanaExternalCall>, SettlementInputError> {
    tx.aggregation
        .as_ref()
        .ok_or(SettlementInputError::NoAggregation)?
        .instance
        .actions
        .iter()
        .flat_map(|action| &action.resource_instances)
        .flat_map(|resource| &resource.app_data().external_payload)
        .map(|blob| {
            SolanaExternalCall::decode(words_to_bytes(&blob.blob))
                .map_err(|e| SettlementInputError::ExternalCall(e.to_string()))
        })
        .collect()
}

/// A real Groth16 receipt becomes arm's seal; a receipt holding a claim digest
/// (a dev-mode receipt, or pa-testkit's local proof) becomes the mock seal of
/// that digest, as arm's `encode_seal` re-encodes a receipt for the EVM
/// adapter. Whether the seal proves the transaction's claim is the verifier's
/// to decide.
fn router_seal(proof: &[u8]) -> Result<Vec<u8>, SettlementInputError> {
    let receipt: InnerReceipt =
        bincode::deserialize(proof).map_err(|e| SettlementInputError::Receipt(e.to_string()))?;
    let claim = match receipt {
        InnerReceipt::Fake(fake) => fake.claim.digest(),
        InnerReceipt::Groth16(receipt)
            if receipt.verifier_parameters == MOCK_VERIFIER_PARAMETERS =>
        {
            Digest::try_from(receipt.seal.as_slice()).map_err(|_| {
                SettlementInputError::Receipt(format!(
                    "a mock Groth16 receipt holds a {}-byte claim digest, not 32 bytes",
                    receipt.seal.len()
                ))
            })?
        }
        _ => return encode_seal(proof).map_err(|e| SettlementInputError::Seal(format!("{e:?}"))),
    };
    Ok(mock_seal(claim))
}

/// The mock seal in the router seal's layout: the mock selector, then a
/// Groth16 proof (a: 64 bytes, b: 128, c: 64) that is zero but for the claim
/// digest at the start of c, where the mock verifier reads it (the adapter
/// negates a before it calls the router).
fn mock_seal(claim: Digest) -> Vec<u8> {
    let mut seal = Vec::with_capacity(ROUTER_SEAL_LEN);
    seal.extend(MOCK_SELECTOR);
    seal.extend([0u8; 64 + 128]);
    seal.extend(claim.as_bytes());
    seal.extend([0u8; 32]);
    seal
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use anoma_pa_testkit::environment::Prover;
    use anoma_pa_testkit::fixtures::{action, trivial};
    use anoma_pa_testkit::prover::LocalProver;
    use anoma_pa_testkit::witness::ActionWitnesses;
    use anoma_rm_risc0::aggregation_instance::AggregationResourceInstance;
    use anoma_rm_risc0::constants::BATCH_AGGREGATION_VK;
    use anoma_rm_risc0::logic_instance::{AppData, ExpirableBlob};
    use anoma_rm_risc0::resource::ResourceWitness;
    use risc0_zkvm::sha::Sha256;
    use risc0_zkvm::{FakeReceipt, Groth16Receipt, MaybePruned, ReceiptClaim};

    /// A resource of a trivial action: consumed, with its seed, or created.
    #[derive(Clone, Copy)]
    enum Trivial {
        Consumed(u8),
        Created,
    }

    /// The witnesses of one action of trivial resources, in `resources`'
    /// order.
    fn trivial_action(resources: &[Trivial]) -> ActionWitnesses {
        action::build(resources.iter().map(|resource| match *resource {
            Trivial::Consumed(seed) => trivial::consumed(seed, trivial::Overrides::default()),
            Trivial::Created => trivial::created(),
        }))
        .expect("trivial action")
    }

    async fn proven(actions: &[ActionWitnesses]) -> Transaction {
        LocalProver
            .prove(&actions.iter().collect::<Vec<_>>())
            .await
            .expect("local proof")
            .into_arm()
    }

    #[tokio::test]
    async fn the_settled_resources_are_each_actions_nullifiers_and_commitments_in_list_order() {
        use Trivial::{Consumed, Created};
        let actions = [
            trivial_action(&[Consumed(1), Created]),
            trivial_action(&[Created, Consumed(4), Created, Consumed(6)]),
        ];
        let witnesses = || {
            actions
                .iter()
                .flat_map(|a| &a.compliance_witness.resource_witnesses)
        };
        let nullifiers: Vec<[u8; 32]> = witnesses()
            .filter_map(|w| match w {
                ResourceWitness::Consumed(c) => {
                    Some(c.resource.nullifier(&c.nf_key).unwrap().into())
                }
                ResourceWitness::Created(_) => None,
            })
            .collect();
        let created: Vec<[u8; 32]> = witnesses()
            .filter_map(|w| match w {
                ResourceWitness::Created(r) => Some(r.commitment().into()),
                ResourceWitness::Consumed(_) => None,
            })
            .collect();
        let ephemeral_roots: Vec<[u8; 32]> = actions
            .iter()
            .flat_map(|a| {
                let consumed = a
                    .compliance_witness
                    .resource_witnesses
                    .iter()
                    .filter(|w| matches!(w, ResourceWitness::Consumed(_)));
                consumed.map(|_| a.compliance_witness.ephemeral_root.into())
            })
            .collect();
        let tx = proven(&actions).await;

        let resources = settled_resources(&tx).expect("settled resources");
        assert_eq!(
            resources.nullifiers, nullifiers,
            "the consumed resources' nullifiers in list order"
        );
        assert_eq!(
            resources.consumed_roots, ephemeral_roots,
            "each consumed resource's root"
        );
        assert_eq!(
            resources.created, created,
            "the created resources' commitments in list order"
        );

        let mut no_aggregation = tx;
        no_aggregation.aggregation = None;
        assert_eq!(
            settled_resources(&no_aggregation),
            Err(SettlementInputError::NoAggregation)
        );
        assert_eq!(
            settlement_input(no_aggregation),
            Err(SettlementInputError::NoAggregation)
        );
    }

    #[tokio::test]
    async fn a_tampered_local_proof_keeps_its_digest_for_the_verifier_to_refuse() {
        let built = trivial_action(&[Trivial::Consumed(1), Trivial::Created]);
        let mut proven = LocalProver.prove(&[&built]).await.expect("local proof");
        let mut tampered = claim_of(proven.as_arm()).as_bytes().to_vec();
        tampered[0] ^= 0xff;
        proven
            .tamper_aggregation_seal()
            .expect("tamper the aggregation seal");
        let input = settlement_input(proven.into_arm()).expect("settlement input");
        assert_eq!(
            settled_proof(&input),
            expected_mock_seal(Digest::try_from(tampered.as_slice()).unwrap()),
            "the conversion re-encodes the receipt; refusing it is the verifier's job"
        );
    }

    fn app_data_mut(resource: &mut AggregationResourceInstance) -> &mut AppData {
        match resource {
            AggregationResourceInstance::Consumed { app_data, .. }
            | AggregationResourceInstance::Created { app_data, .. } => app_data,
        }
    }

    #[tokio::test]
    async fn the_external_calls_are_each_actions_resources_calls_in_list_order() {
        use Trivial::{Consumed, Created};
        let call = |n: u8| SolanaExternalCall {
            program_id: [n; 32],
            instruction_data: vec![n; n as usize],
            expected_output: vec![1],
            output_mode: crate::external_call::OutputMode::ReturnData,
            num_accounts: n,
        };
        let blob = |n: u8| ExpirableBlob {
            blob: anoma_rm_risc0::utils::bytes_to_words(&call(n).encode()),
            deletion_criterion: 0,
        };
        let mut tx = proven(&[
            trivial_action(&[Created, Consumed(2), Created]),
            trivial_action(&[Consumed(4), Created]),
        ])
        .await;
        let instance = &mut tx.aggregation.as_mut().unwrap().instance;
        let first = &mut instance.actions[0].resource_instances;
        assert_eq!(
            first.iter().map(|r| r.is_consumed()).collect::<Vec<_>>(),
            [false, true, false],
            "the instance keeps the action's list order"
        );
        // Each resource lists its calls in run order; the created resources
        // around the consumed one run before and after it.
        app_data_mut(&mut first[0]).external_payload = vec![blob(1), blob(2)];
        app_data_mut(&mut first[1]).external_payload = vec![blob(3)];
        app_data_mut(&mut first[2]).external_payload = vec![blob(4)];
        app_data_mut(&mut instance.actions[1].resource_instances[1]).external_payload =
            vec![blob(5)];

        assert_eq!(
            external_calls(&tx).expect("external calls"),
            (1..=5).map(call).collect::<Vec<_>>()
        );

        app_data_mut(
            &mut tx.aggregation.as_mut().unwrap().instance.actions[1].resource_instances[0],
        )
        .external_payload = vec![ExpirableBlob {
            blob: vec![u32::MAX],
            deletion_criterion: 0,
        }];
        assert!(matches!(
            external_calls(&tx),
            Err(SettlementInputError::ExternalCall(_))
        ));
    }

    /// A locally proven transaction of one action consuming and creating
    /// `count` resources each.
    pub(crate) async fn proven_trivial_transaction(count: u8) -> Transaction {
        let consumed = (0..count).map(Trivial::Consumed);
        let created = (0..count).map(|_| Trivial::Created);
        proven(&[trivial_action(&consumed.chain(created).collect::<Vec<_>>())]).await
    }

    fn claim_of(tx: &Transaction) -> Digest {
        let journal = tx.aggregation.as_ref().unwrap().instance.to_journal();
        ReceiptClaim::ok(
            BATCH_AGGREGATION_VK,
            MaybePruned::Pruned(*risc0_zkvm::sha::Impl::hash_bytes(&journal)),
        )
        .digest()
    }

    fn settled_proof(input: &[u8]) -> Vec<u8> {
        let tx: Transaction = bincode::deserialize(input).expect("settlement input decodes");
        tx.aggregation.expect("aggregation").proof
    }

    /// selector ‖ Groth16 a (64) ‖ b (128) ‖ c (64), the claim digest at the
    /// start of c: the layout written out independently of `mock_seal`.
    fn expected_mock_seal(claim: Digest) -> Vec<u8> {
        let mut seal = vec![0xff; 4];
        seal.extend([0u8; 192]);
        seal.extend(claim.as_bytes());
        seal.extend([0u8; 32]);
        seal
    }

    #[tokio::test]
    async fn a_proven_transaction_simulates_as_it_settles() {
        let tx = proven_trivial_transaction(1).await;
        assert_eq!(
            simulation_input(tx.clone(), MOCK_SELECTOR).expect("simulation input"),
            settlement_input(tx).expect("settlement input")
        );
    }

    #[tokio::test]
    async fn an_unproven_transaction_simulates_with_the_selector_as_its_proof() {
        let mut tx = proven_trivial_transaction(1).await;
        tx.aggregation.as_mut().unwrap().proof = vec![];
        let input = simulation_input(tx.clone(), MOCK_SELECTOR).expect("input");
        assert_eq!(settled_proof(&input), MOCK_SELECTOR.to_vec());
        let mut settled: Transaction = bincode::deserialize(&input).unwrap();
        settled.aggregation.as_mut().unwrap().proof = vec![];
        assert_eq!(
            bincode::serialize(&settled).unwrap(),
            bincode::serialize(&tx).unwrap(),
            "only the aggregation proof changes"
        );
    }

    #[tokio::test]
    async fn a_pa_testkit_local_proof_becomes_the_mock_router_seal() {
        let tx = proven_trivial_transaction(1).await;
        let claim = claim_of(&tx);
        let input = settlement_input(tx.clone()).expect("settlement input");
        assert_eq!(settled_proof(&input), expected_mock_seal(claim));
        let mut settled: Transaction = bincode::deserialize(&input).unwrap();
        settled.aggregation.as_mut().unwrap().proof =
            tx.aggregation.as_ref().unwrap().proof.clone();
        assert_eq!(
            bincode::serialize(&settled).unwrap(),
            bincode::serialize(&tx).unwrap(),
            "only the aggregation proof changes"
        );
    }

    #[tokio::test]
    async fn a_dev_mode_receipt_becomes_the_mock_router_seal_of_its_claim() {
        let mut tx = proven_trivial_transaction(1).await;
        let journal = tx.aggregation.as_ref().unwrap().instance.to_journal();
        let claim = ReceiptClaim::ok(BATCH_AGGREGATION_VK, journal);
        let digest = claim.digest();
        tx.aggregation.as_mut().unwrap().proof =
            bincode::serialize(&InnerReceipt::Fake(FakeReceipt::new(claim))).unwrap();
        let input = settlement_input(tx).expect("settlement input");
        assert_eq!(settled_proof(&input), expected_mock_seal(digest));
        assert_eq!(digest, claim_of(&bincode::deserialize(&input).unwrap()));
    }

    #[tokio::test]
    async fn a_groth16_receipt_becomes_its_selector_and_seal() {
        let mut tx = proven_trivial_transaction(1).await;
        let groth16_seal: Vec<u8> = (0..=255).collect();
        let parameters = Digest::new([0x73c4_57ba, 1, 2, 3, 4, 5, 6, 7]);
        tx.aggregation.as_mut().unwrap().proof =
            bincode::serialize(&InnerReceipt::Groth16(Groth16Receipt::new(
                groth16_seal.clone(),
                MaybePruned::Pruned(Digest::default()),
                parameters,
            )))
            .unwrap();
        let mut expected = parameters.as_bytes()[..4].to_vec();
        expected.extend(groth16_seal);
        assert_eq!(settled_proof(&settlement_input(tx).unwrap()), expected);
    }

    #[tokio::test]
    async fn the_proven_settlement_input_len_is_known_before_the_proof() {
        let proven = proven_trivial_transaction(1).await;
        let settled = settlement_input(proven.clone()).unwrap().len();
        assert_eq!(proven_settlement_input_len(&proven), Ok(settled));
        let mut unproven = proven;
        unproven.aggregation.as_mut().unwrap().proof = vec![];
        assert_eq!(
            proven_settlement_input_len(&unproven),
            Ok(settled),
            "an unproven transaction settles in an input as long"
        );

        // A Groth16 receipt's seal is as long as the mock seal.
        let mut groth16 = unproven.clone();
        groth16.aggregation.as_mut().unwrap().proof =
            bincode::serialize(&InnerReceipt::Groth16(Groth16Receipt::new(
                vec![7; 256],
                MaybePruned::Pruned(Digest::default()),
                Digest::new([0x73c4_57ba, 1, 2, 3, 4, 5, 6, 7]),
            )))
            .unwrap();
        assert_eq!(settlement_input(groth16).unwrap().len(), settled);

        unproven.aggregation = None;
        assert_eq!(
            proven_settlement_input_len(&unproven),
            Err(SettlementInputError::NoAggregation)
        );
    }
}
