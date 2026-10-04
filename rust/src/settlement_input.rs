//! The adapter's settlement input: a proven ARM transaction whose aggregation
//! proof is re-encoded as the verifier-router seal the adapter decodes,
//! serialized with bincode.

use std::fmt;

use anoma_rm_risc0::proving_system::encode_seal;
use anoma_rm_risc0::transaction::Transaction;
use risc0_zkvm::sha::{Digest, Digestible};
use risc0_zkvm::InnerReceipt;

/// The selector a mock seal carries, risc0's convention for a receipt that
/// holds a claim digest instead of a proof. The local validator registers the
/// mock verifier under it.
pub const MOCK_SELECTOR: [u8; 4] = [0xff; 4];

/// The verifier parameters of a Groth16 receipt that holds a claim digest
/// instead of a proof, as pa-testkit's local prover produces it.
const MOCK_VERIFIER_PARAMETERS: Digest = Digest::new([u32::MAX; 8]);

#[derive(Debug, PartialEq, Eq)]
pub enum SettlementInputError {
    /// The transaction carries no aggregation proof.
    NoAggregation,
    /// The aggregation proof is not a bincode-encoded risc0 receipt.
    Receipt(String),
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
    let consumed = || actions.iter().flat_map(|action| &action.consumed_publics);
    Ok(SettledResources {
        nullifiers: consumed().map(|c| c.resource_nullifier.into()).collect(),
        consumed_roots: consumed().map(|c| c.commitment_tree_root.into()).collect(),
        created: actions
            .iter()
            .flat_map(|action| &action.created_publics)
            .map(|c| c.resource_commitment.into())
            .collect(),
    })
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
    let mut seal = Vec::with_capacity(4 + 256);
    seal.extend(MOCK_SELECTOR);
    seal.extend([0u8; 64 + 128]);
    seal.extend(claim.as_bytes());
    seal.extend([0u8; 32]);
    seal
}

#[cfg(test)]
mod tests {
    use super::*;
    use anoma_pa_testkit::environment::Prover;
    use anoma_pa_testkit::fixtures::trivial;
    use anoma_pa_testkit::prover::LocalProver;
    use anoma_rm_risc0::constants::BATCH_AGGREGATION_VK;
    use anoma_rm_risc0::resource::Resource;
    use risc0_zkvm::sha::{Digest, Digestible, Sha256};
    use risc0_zkvm::{FakeReceipt, Groth16Receipt, InnerReceipt, MaybePruned, ReceiptClaim};

    #[tokio::test]
    async fn the_settled_resources_are_each_actions_nullifiers_and_commitments_in_order() {
        let first = trivial::build(1, trivial::Overrides::default()).expect("trivial action");
        let second = trivial::build(
            2,
            trivial::Overrides {
                consumed_count: Some(2),
                created_count: Some(2),
                ..Default::default()
            },
        )
        .expect("trivial action");
        let created: Vec<[u8; 32]> = first
            .created_ephemerals
            .iter()
            .chain(&second.created_ephemerals)
            .map(|r| r.commitment().into())
            .collect();
        let created_nonces: Vec<Vec<[u8; 32]>> = [&first, &second]
            .iter()
            .map(|a| a.created_ephemerals.iter().map(|r| r.nonce).collect())
            .collect();
        let tx = LocalProver
            .prove(&[first.witnesses, second.witnesses])
            .await
            .expect("local proof")
            .into_arm();

        let resources = settled_resources(&tx).expect("settled resources");
        assert_eq!(resources.created, created);
        assert_eq!(resources.nullifiers.len(), 3);
        assert_eq!(resources.consumed_roots.len(), 3);
        // A created resource's nonce derives from its action's nullifiers, so
        // the nullifiers are each action's, in order.
        for (nullifiers, nonces) in [&resources.nullifiers[..1], &resources.nullifiers[1..]]
            .iter()
            .zip(&created_nonces)
        {
            let digests: Vec<Digest> = nullifiers.iter().map(|n| Digest::from(*n)).collect();
            for (index, nonce) in nonces.iter().enumerate() {
                let derived =
                    Resource::derive_nonce_from_nullifiers(index as u32, &digests).unwrap();
                assert_eq!(&derived, nonce, "created nonce {index}");
            }
        }

        let mut no_aggregation = tx;
        no_aggregation.aggregation = None;
        assert_eq!(
            settled_resources(&no_aggregation),
            Err(SettlementInputError::NoAggregation)
        );
    }

    #[tokio::test]
    async fn a_tampered_local_proof_keeps_its_digest_for_the_verifier_to_refuse() {
        let built = trivial::build(1, trivial::Overrides::default()).expect("trivial action");
        let mut proven = LocalProver
            .prove(&[built.witnesses])
            .await
            .expect("local proof");
        let mut tampered = claim_of(proven.as_arm()).as_bytes().to_vec();
        tampered[0] ^= 0xff;
        proven
            .tamper_aggregation_seal()
            .expect("tamper the aggregation seal");
        let input = settlement_input(proven.into_arm()).expect("settlement input");
        assert_eq!(
            settled_proof(&input),
            mock_seal(Digest::try_from(tampered.as_slice()).unwrap()),
            "the conversion re-encodes the receipt; refusing it is the verifier's job"
        );
    }

    async fn proven_trivial_transaction() -> Transaction {
        let built = trivial::build(1, trivial::Overrides::default()).expect("trivial action");
        LocalProver
            .prove(&[built.witnesses])
            .await
            .expect("local proof")
            .into_arm()
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

    /// selector ‖ Groth16 a (64) ‖ b (128) ‖ c (64), the claim digest at the start of c.
    fn mock_seal(claim: Digest) -> Vec<u8> {
        let mut seal = vec![0xff; 4];
        seal.extend([0u8; 192]);
        seal.extend(claim.as_bytes());
        seal.extend([0u8; 32]);
        seal
    }

    #[tokio::test]
    async fn a_pa_testkit_local_proof_becomes_the_mock_router_seal() {
        let tx = proven_trivial_transaction().await;
        let claim = claim_of(&tx);
        let input = settlement_input(tx.clone()).expect("settlement input");
        assert_eq!(settled_proof(&input), mock_seal(claim));
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
        let mut tx = proven_trivial_transaction().await;
        let journal = tx.aggregation.as_ref().unwrap().instance.to_journal();
        let claim = ReceiptClaim::ok(BATCH_AGGREGATION_VK, journal);
        let digest = claim.digest();
        tx.aggregation.as_mut().unwrap().proof =
            bincode::serialize(&InnerReceipt::Fake(FakeReceipt::new(claim))).unwrap();
        let input = settlement_input(tx).expect("settlement input");
        assert_eq!(settled_proof(&input), mock_seal(digest));
        assert_eq!(digest, claim_of(&bincode::deserialize(&input).unwrap()));
    }

    #[tokio::test]
    async fn a_groth16_receipt_becomes_its_selector_and_seal() {
        let mut tx = proven_trivial_transaction().await;
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
    async fn a_transaction_without_aggregation_has_no_settlement_input() {
        let mut tx = proven_trivial_transaction().await;
        tx.aggregation = None;
        assert_eq!(
            settlement_input(tx),
            Err(SettlementInputError::NoAggregation)
        );
    }
}
