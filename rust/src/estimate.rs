//! A settlement estimated before it is proven, as pa-evm's integrators
//! estimate `execute`: `eth_estimateGas` runs the real `execute` on one
//! block's state, with the verifier's code replaced by a state override, and
//! adds the measured gas of one Groth16 verification. Here the settlement's
//! own transactions run in a local SVM holding the cluster's accounts as of
//! one read, with the verifier program replaced by one that accepts every
//! proof, and the compute of one Groth16 verification, measured on the
//! cluster's verifier program, is held back from the settlement's compute
//! budget.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use agave_feature_set::{FeatureSet, FEATURE_NAMES};
use base64::Engine;
use litesvm::types::FailedTransactionMetadata;
use litesvm::LiteSVM;
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_message::{Message, VersionedMessage};
use solana_program_runtime::declare_process_instruction;
use solana_program_runtime::solana_sbpf::program::BuiltinFunctionDefinition;
use solana_pubkey::Pubkey;
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_signature::Signature;
use solana_transaction::versioned::VersionedTransaction;
use solana_transaction_error::TransactionError;

use anchor::AccountDeserialize;

use crate::accounts::decode_pa_state;
use risc0_zkvm::sha::{Digest, Digestible};
use risc0_zkvm::{MaybePruned, ReceiptClaim};

use crate::constants::{MOCK_SELECTOR, SETTLE_COMPUTE_UNIT_LIMIT, TXDATA_EXPIRY_SLOTS_DEFAULT};
use crate::generated::verifier_router::accounts::VerifierEntry;
use crate::pda::{
    derive_pa_state_pda, derive_root_marker_pda, derive_tx_data_pda, derive_verifier_entry_pda,
};
use crate::settlement::{plan_settlement, PlannedTransaction, SettlementPlan};
use crate::settlement_input::placeholder_settlement_input;
use crate::submission::{settlement_failure, Failure, Submission, SubmissionError};

/// How many accounts one `getMultipleAccounts` returns, the RPC's limit: a
/// read that needs more cannot be one read.
const MAX_MULTIPLE_ACCOUNTS: usize = 100;

/// The upload id the estimate's upload, when it has one, takes. Its account
/// is the estimate's own: the read leaves it out, so the upload starts empty
/// whatever the payer holds on the cluster under that id.
const ESTIMATE_UPLOAD_ID: u64 = 0;

/// The sysvars the runtime gives the settlement's programs, read with the
/// settlement's accounts so they hold the same slot's values.
const SYSVARS: [Pubkey; 7] = [
    solana_sdk_ids::sysvar::clock::ID,
    solana_sdk_ids::sysvar::rent::ID,
    solana_sdk_ids::sysvar::epoch_schedule::ID,
    solana_sdk_ids::sysvar::epoch_rewards::ID,
    solana_sdk_ids::sysvar::last_restart_slot::ID,
    solana_sdk_ids::sysvar::slot_hashes::ID,
    solana_sdk_ids::sysvar::stake_history::ID,
];

/// The selector of a Groth16 proof and the verify instruction of one its
/// verifier program accepted: the router's call to it in a devnet settlement
/// (the file names the transaction). Its compute is the compute of one
/// Groth16 verification, which does not depend on the proof.
static GROTH16_MEASUREMENT: LazyLock<([u8; 4], Vec<u8>)> = LazyLock::new(|| {
    let measurement: serde_json::Value =
        serde_json::from_str(include_str!("groth16_compute_measurement.json"))
            .expect("the Groth16 measurement is JSON");
    let field = |name: &str| {
        measurement[name]
            .as_str()
            .unwrap_or_else(|| panic!("the Groth16 measurement holds no {name}"))
            .to_owned()
    };
    let selector = u32::from_str_radix(&field("selector"), 16)
        .expect("the Groth16 measurement's selector is 4 hex bytes")
        .to_be_bytes();
    let data = base64::engine::general_purpose::STANDARD
        .decode(field("verify_instruction_data_base64"))
        .expect("the Groth16 measurement's instruction data is base64");
    (selector, data)
});

/// The verify instruction the mock verifier accepts, for the mock selector:
/// a proof that is zero but for the receipt claim digest of its image id and
/// journal digest at the start of c, where the mock verifier reads it. The
/// discriminator is the verify instruction's, which every verifier the
/// router dispatches to shares.
fn mock_verification() -> Vec<u8> {
    let (_, groth16) = &*GROTH16_MEASUREMENT;
    let (image_id, journal_digest) = (Digest::ZERO, Digest::ZERO);
    let claim = ReceiptClaim::ok(image_id, MaybePruned::Pruned(journal_digest)).digest();
    let mut data = groth16[..8].to_vec();
    data.extend([0u8; 64 + 128]);
    data.extend(claim.as_bytes());
    data.extend([0u8; 32]);
    data.extend(image_id.as_bytes());
    data.extend(journal_digest.as_bytes());
    data
}

/// The verify instruction the estimate measures the verifier program behind
/// `selector` with.
fn verification_measurement(selector: [u8; 4]) -> Result<Vec<u8>, SubmissionError> {
    let (groth16_selector, groth16) = &*GROTH16_MEASUREMENT;
    if selector == *groth16_selector {
        Ok(groth16.clone())
    } else if selector == MOCK_SELECTOR {
        Ok(mock_verification())
    } else {
        Err(SubmissionError::UnmeasuredProofSelector { selector })
    }
}

declare_process_instruction!(AcceptingVerifier, 0, |_invoke_context| { Ok(()) });

/// How the estimate of a settlement ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Estimate {
    /// The settlement would execute, its settlement transaction using these
    /// compute units with its proof verified.
    Executable { units_consumed: u64 },
    /// The settlement would fail with a program's custom error.
    Failed(Failure),
    /// A transaction of the settlement would fail without any program's
    /// custom error (a fee it cannot pay, a compute budget it exceeds).
    Other {
        error: TransactionError,
        logs: Vec<String>,
    },
}

/// The cluster's accounts as of one read.
struct Snapshot {
    slot: u64,
    accounts: Vec<(Pubkey, Account)>,
}

impl Snapshot {
    fn get(&self, address: &Pubkey) -> Option<&Account> {
        self.accounts
            .iter()
            .find_map(|(key, account)| (key == address).then_some(account))
    }

    /// The account at `address`, which the settlement reads, as `decode`
    /// reads it.
    fn decode<T, E: std::fmt::Display>(
        &self,
        address: &Pubkey,
        what: String,
        decode: impl FnOnce(&[u8]) -> Result<T, E>,
    ) -> Result<T, SubmissionError> {
        let account = self
            .get(address)
            .ok_or_else(|| SubmissionError::MissingAccount { what: what.clone() })?;
        decode(&account.data).map_err(|e| SubmissionError::Decode {
            what,
            error: e.to_string(),
        })
    }
}

/// The accounts the settlement's transactions name that exist before it: all
/// but its upload, which the estimate creates itself, and the marker of the
/// root it produces, which no settlement can have produced before.
fn existing_accounts(
    plan: &SettlementPlan,
    pa_program: &Pubkey,
    upload: &Pubkey,
) -> BTreeSet<Pubkey> {
    let (pa_state, _) = derive_pa_state_pda(pa_program);
    let new_root_marker = plan
        .new_root
        .map(|root| derive_root_marker_pda(pa_program, &pa_state, &root).0);
    let transactions = plan
        .upload
        .iter()
        .flat_map(|upload| std::iter::once(&upload.init).chain(&upload.writes))
        .chain([&plan.settle]);
    transactions
        .flat_map(|planned| &planned.instructions)
        .flat_map(|ix| {
            std::iter::once(ix.program_id).chain(ix.accounts.iter().map(|meta| meta.pubkey))
        })
        .filter(|key| key != upload && Some(*key) != new_root_marker)
        .collect()
}

/// The program data account of each program in `snapshot` that the
/// upgradeable loader owns: the account holding its code.
fn program_data_accounts(snapshot: &Snapshot) -> BTreeSet<Pubkey> {
    snapshot
        .accounts
        .iter()
        .filter(|(_, account)| {
            account.executable && account.owner == solana_sdk_ids::bpf_loader_upgradeable::ID
        })
        .map(|(program, _)| solana_loader_v3_interface::get_program_data_address(program))
        .collect()
}

async fn read(rpc: &RpcClient, keys: &BTreeSet<Pubkey>) -> Result<Snapshot, SubmissionError> {
    if keys.len() > MAX_MULTIPLE_ACCOUNTS {
        return Err(SubmissionError::TooManyAccounts {
            accounts: keys.len(),
        });
    }
    let keys: Vec<Pubkey> = keys.iter().copied().collect();
    let read = rpc
        .get_multiple_accounts_with_commitment(&keys, rpc.commitment())
        .await
        .map_err(|error| SubmissionError::Rpc {
            what: "the settlement's accounts".to_owned(),
            error: error.to_string(),
        })?;
    Ok(Snapshot {
        slot: read.context.slot,
        accounts: keys
            .into_iter()
            .zip(read.value)
            .filter_map(|(key, account)| Some((key, account?)))
            .collect(),
    })
}

/// The features active on the cluster behind `rpc` at `slot`: each of the
/// runtime's features whose account records an activation at or before it.
async fn read_feature_set(rpc: &RpcClient, slot: u64) -> Result<FeatureSet, SubmissionError> {
    let ids: Vec<Pubkey> = FEATURE_NAMES.keys().copied().collect();
    let mut features = FeatureSet::default();
    for chunk in ids.chunks(MAX_MULTIPLE_ACCOUNTS) {
        let accounts = rpc
            .get_multiple_accounts_with_commitment(chunk, rpc.commitment())
            .await
            .map_err(|error| SubmissionError::Rpc {
                what: "the cluster's feature accounts".to_owned(),
                error: error.to_string(),
            })?
            .value;
        for (id, account) in chunk.iter().zip(accounts) {
            let Some(account) = account else { continue };
            let feature: solana_feature_gate_interface::Feature =
                bincode::deserialize(&account.data).map_err(|e| SubmissionError::Decode {
                    what: format!("the feature account {id}"),
                    error: e.to_string(),
                })?;
            if let Some(activated) = feature.activated_at.filter(|activated| *activated <= slot) {
                features.activate(id, activated);
            }
        }
    }
    Ok(features)
}

/// The unsigned v1 transaction of `planned`, paid for by `payer`.
fn unsigned(
    planned: &PlannedTransaction,
    payer: &Pubkey,
    svm: &LiteSVM,
) -> Result<VersionedTransaction, SubmissionError> {
    let message = planned
        .message(payer, svm.latest_blockhash())
        .map_err(crate::settlement::PlanError::from)?;
    Ok(VersionedTransaction {
        signatures: vec![Signature::default(); usize::from(message.header.num_required_signatures)],
        message: VersionedMessage::V1(message),
    })
}

/// The compute units the verifier program of `svm` uses to verify one proof
/// of `selector`'s proof system, paid for by `payer`.
fn verification_units(
    svm: &LiteSVM,
    verifier_program: &Pubkey,
    selector: [u8; 4],
    payer: &Pubkey,
) -> Result<u64, SubmissionError> {
    let verify = Instruction {
        program_id: *verifier_program,
        accounts: vec![AccountMeta::new_readonly(
            solana_sdk_ids::system_program::ID,
            false,
        )],
        data: verification_measurement(selector)?,
    };
    let message = Message::new_with_blockhash(&[verify], Some(payer), &svm.latest_blockhash());
    let transaction = VersionedTransaction {
        signatures: vec![Signature::default()],
        message: VersionedMessage::Legacy(message),
    };
    svm.simulate_transaction(transaction)
        .map(|simulated| simulated.meta.compute_units_consumed)
        .map_err(|failed| SubmissionError::VerifierMeasurement {
            verifier_program: *verifier_program,
            error: failed.err.to_string(),
            logs: failed.meta.logs,
        })
}

impl Submission {
    /// Estimates the settlement against the cluster behind `rpc`, at its
    /// commitment, before the transaction is proven: its transactions, as
    /// the payer would send them with the adapter's state the read holds,
    /// run in a local SVM on the cluster's accounts as of one read, with the
    /// verifier program replaced by one that accepts every proof. The
    /// settlement's compute budget is reduced by the compute of one Groth16
    /// verification, measured on the cluster's verifier program, so a
    /// settlement whose verified proof would exhaust its budget fails here
    /// too.
    pub async fn estimate(&self, rpc: &RpcClient) -> Result<Estimate, SubmissionError> {
        let pa_program = self.pa_program;
        let payer = self.payer;
        let input =
            placeholder_settlement_input(self.transaction.clone(), self.state.proof_selector)?;
        let upload = derive_tx_data_pda(&pa_program, &payer, ESTIMATE_UPLOAD_ID).0;
        let (state_address, _) = derive_pa_state_pda(&pa_program);

        // The read's keys depend on what it reads: the adapter's state names
        // the verifier entry, the entry names the verifier program, and each
        // program names its program data. Read until one read holds every
        // account that the settlement, planned from that read, names.
        let mut state = self.state.clone();
        let mut verifier_program = self.verifier_program;
        let mut keys: BTreeSet<Pubkey> = SYSVARS.into_iter().collect();
        let snapshot = loop {
            let plan = plan_settlement(
                self.request_with(&input, &state, verifier_program),
                ESTIMATE_UPLOAD_ID,
                0,
            )?;
            keys.extend(existing_accounts(&plan, &pa_program, &upload));
            let snapshot = read(rpc, &keys).await?;
            state = snapshot.decode(
                &state_address,
                format!("the adapter's state {state_address}"),
                decode_pa_state,
            )?;
            let entry = derive_verifier_entry_pda(&state.verifier_router, state.proof_selector);
            verifier_program = snapshot
                .decode(
                    &entry,
                    format!("the router's verifier entry {entry}"),
                    |mut data| VerifierEntry::try_deserialize(&mut data),
                )?
                .verifier;
            let replanned = plan_settlement(
                self.request_with(&input, &state, verifier_program),
                ESTIMATE_UPLOAD_ID,
                0,
            )?;
            let needed: BTreeSet<Pubkey> = existing_accounts(&replanned, &pa_program, &upload)
                .into_iter()
                .chain(program_data_accounts(&snapshot))
                .collect();
            if needed.is_subset(&keys) {
                break snapshot;
            }
            keys.extend(needed);
        };

        let clock: solana_clock::Clock = snapshot.decode(
            &solana_sdk_ids::sysvar::clock::ID,
            "the Clock sysvar".to_owned(),
            |data| bincode::deserialize(data),
        )?;
        let plan = plan_settlement(
            self.request_with(&input, &state, verifier_program),
            ESTIMATE_UPLOAD_ID,
            clock.slot + TXDATA_EXPIRY_SLOTS_DEFAULT,
        )?;

        let features = read_feature_set(rpc, snapshot.slot).await?;
        let mut svm = LiteSVM::default()
            .with_feature_set(features)
            .with_builtins()
            .with_precompiles()
            .with_sysvars()
            .with_transaction_history(0);
        // A program's code must be in place before the program is.
        let (programs, data): (Vec<_>, Vec<_>) = snapshot
            .accounts
            .iter()
            .filter(|(_, account)| account.owner != solana_sdk_ids::native_loader::ID)
            .partition(|(_, account)| account.executable);
        for (address, account) in data.into_iter().chain(programs) {
            svm.set_account(*address, account.clone())
                .map_err(|e| SubmissionError::Decode {
                    what: format!("the account {address}"),
                    error: e.to_string(),
                })?;
        }

        let verification =
            verification_units(&svm, &verifier_program, state.proof_selector, &payer)?;
        svm.add_builtin(verifier_program, AcceptingVerifier::register);

        let failed = |failed: FailedTransactionMetadata| -> Result<Estimate, SubmissionError> {
            Ok(
                match settlement_failure(&failed.meta.logs, &pa_program, &verifier_program)? {
                    Some(failure) => Estimate::Failed(failure),
                    None => Estimate::Other {
                        error: failed.err,
                        logs: failed.meta.logs,
                    },
                },
            )
        };
        let uploading = plan
            .upload
            .into_iter()
            .flat_map(|upload| std::iter::once(upload.init).chain(upload.writes));
        for planned in uploading {
            if let Err(failure) = svm.send_transaction(unsigned(&planned, &payer, &svm)?) {
                return failed(failure);
            }
        }
        let mut settle = plan.settle;
        // A verification using more than the settlement's whole budget leaves
        // it none.
        settle.config.compute_unit_limit = Some(
            u32::try_from(verification)
                .map_or(0, |units| SETTLE_COMPUTE_UNIT_LIMIT.saturating_sub(units)),
        );
        match svm.send_transaction(unsigned(&settle, &payer, &svm)?) {
            Ok(meta) => Ok(Estimate::Executable {
                units_consumed: meta.compute_units_consumed + verification,
            }),
            Err(failure) => failed(failure),
        }
    }
}
