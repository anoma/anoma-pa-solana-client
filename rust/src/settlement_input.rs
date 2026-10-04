//! The adapter's settlement input: a proven ARM transaction whose aggregation
//! proof is re-encoded as the verifier-router seal the adapter decodes,
//! serialized with bincode.

use std::fmt;

use anoma_rm_risc0::constants::BATCH_AGGREGATION_VK;
use anoma_rm_risc0::proving_system::encode_seal;
use anoma_rm_risc0::transaction::Transaction;
use risc0_zkvm::sha::{Digest, Digestible, Sha256};
use risc0_zkvm::{InnerReceipt, MaybePruned, ReceiptClaim};

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
    /// A mock receipt claims something other than the transaction's own
    /// aggregation claim.
    ClaimMismatch {
        receipt: Digest,
        transaction: Digest,
    },
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
            Self::ClaimMismatch {
                receipt,
                transaction,
            } => write!(
                f,
                "the mock receipt claims {receipt}, the transaction's aggregation claim is {transaction}"
            ),
            Self::Seal(e) => write!(f, "encoding the seal: {e}"),
            Self::Serialize(e) => write!(f, "serializing the transaction: {e}"),
        }
    }
}

impl std::error::Error for SettlementInputError {}

/// `tx` as the adapter settles it: its aggregation proof re-encoded as the
/// router seal (selector ‖ Groth16 a, b, c), the transaction serialized with
/// bincode.
pub fn settlement_input(mut tx: Transaction) -> Result<Vec<u8>, SettlementInputError> {
    let aggregation = tx
        .aggregation
        .as_mut()
        .ok_or(SettlementInputError::NoAggregation)?;
    let claim = aggregation_claim(&aggregation.instance.to_journal());
    aggregation.proof = router_seal(&aggregation.proof, claim)?;
    bincode::serialize(&tx).map_err(|e| SettlementInputError::Serialize(e.to_string()))
}

/// The claim a batch aggregation proof over `journal` proves.
fn aggregation_claim(journal: &[u8]) -> Digest {
    ReceiptClaim::ok(
        BATCH_AGGREGATION_VK,
        MaybePruned::Pruned(*risc0_zkvm::sha::Impl::hash_bytes(journal)),
    )
    .digest()
}

/// A real Groth16 receipt becomes arm's seal; a receipt holding a claim digest
/// (a dev-mode receipt, or pa-testkit's local proof) becomes the mock seal the
/// mock verifier accepts, once its claim is the transaction's.
fn router_seal(proof: &[u8], claim: Digest) -> Result<Vec<u8>, SettlementInputError> {
    let receipt: InnerReceipt =
        bincode::deserialize(proof).map_err(|e| SettlementInputError::Receipt(e.to_string()))?;
    let mock_claim = match receipt {
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
    if mock_claim != claim {
        return Err(SettlementInputError::ClaimMismatch {
            receipt: mock_claim,
            transaction: claim,
        });
    }
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
    use risc0_zkvm::sha::{Digest, Digestible, Sha256};
    use risc0_zkvm::{FakeReceipt, Groth16Receipt, InnerReceipt, MaybePruned, ReceiptClaim};

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
    async fn a_dev_mode_receipt_becomes_the_mock_router_seal_when_its_claim_matches() {
        let mut tx = proven_trivial_transaction().await;
        let journal = tx.aggregation.as_ref().unwrap().instance.to_journal();
        let claim = ReceiptClaim::ok(BATCH_AGGREGATION_VK, journal);
        let digest = claim.digest();
        tx.aggregation.as_mut().unwrap().proof =
            bincode::serialize(&InnerReceipt::Fake(FakeReceipt::new(claim))).unwrap();
        let input = settlement_input(tx.clone()).expect("settlement input");
        assert_eq!(settled_proof(&input), mock_seal(digest));

        let other = ReceiptClaim::ok(Digest::default(), Vec::<u8>::new());
        tx.aggregation.as_mut().unwrap().proof =
            bincode::serialize(&InnerReceipt::Fake(FakeReceipt::new(other.clone()))).unwrap();
        assert_eq!(
            settlement_input(tx),
            Err(SettlementInputError::ClaimMismatch {
                receipt: other.digest(),
                transaction: digest,
            })
        );
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
