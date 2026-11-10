//! A settlement estimated before it is proven, as pa-evm's integrators
//! estimate `execute`: `eth_estimateGas` runs the real `execute` on one
//! block's state, with the verifier's code replaced by a state override, and
//! adds the measured gas of one Groth16 verification. Here the settlement's
//! own transactions run in a local SVM holding the cluster's accounts as of
//! one read, with the verifier program replaced by one that accepts every
//! proof, and the compute of one Groth16 verification, measured on the
//! cluster's verifier program, is held back from the settlement's compute
//! budget.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use agave_feature_set::{FeatureSet, FEATURE_NAMES};
use base64::Engine;
use litesvm::types::FailedTransactionMetadata;
use litesvm::LiteSVM;
use risc0_zkvm::sha::{Digest, Digestible};
use risc0_zkvm::{MaybePruned, ReceiptClaim};
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_message::v1::TransactionConfig;
use solana_message::{Message, VersionedMessage};
use solana_program_runtime::declare_process_instruction;
use solana_program_runtime::solana_sbpf::program::BuiltinFunctionDefinition;
use solana_pubkey::Pubkey;
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_signature::Signature;
use solana_transaction::versioned::VersionedTransaction;
use solana_transaction_error::TransactionError;

use crate::constants::{MOCK_SELECTOR, SETTLE_COMPUTE_UNIT_LIMIT, TXDATA_EXPIRY_SLOTS_DEFAULT};
use crate::pda::{
    derive_pa_state_pda, derive_program_data_address, derive_root_marker_pda, derive_tx_data_pda,
    derive_verifier_entry_pda,
};
use crate::settlement::{plan_settlement, PlanError, PlannedTransaction, SettlementPlan};
use crate::settlement_input::{mock_seal, placeholder_settlement_input};
use crate::submission::{
    decode_account, decode_adapter_state, decode_verifier_entry, settlement_failure, Failure,
    Submission, SubmissionError,
};

/// How many accounts one `getMultipleAccounts` returns, the RPC's limit: a
/// read that needs more cannot be one read.
pub const MAX_MULTIPLE_ACCOUNTS: usize = 100;

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

/// A Groth16 proof's verify instruction that a verifier program accepted:
/// the router's call to it in a devnet settlement, which the file names with
/// its verifier program. Its compute is the compute of one
/// Groth16 verification, which does not depend on the proof.
struct Groth16Measurement {
    selector: [u8; 4],
    verify_instruction_data: Vec<u8>,
    #[cfg(test)]
    verifier_program: Pubkey,
}

static GROTH16_MEASUREMENT: LazyLock<Groth16Measurement> = LazyLock::new(|| {
    let measurement: serde_json::Value =
        serde_json::from_str(include_str!("groth16_compute_measurement.json"))
            .expect("the Groth16 measurement is JSON");
    let field = |name: &str| {
        measurement[name]
            .as_str()
            .unwrap_or_else(|| panic!("the Groth16 measurement holds no {name}"))
            .to_owned()
    };
    #[cfg(test)]
    let key = |name: &str| {
        field(name)
            .parse()
            .unwrap_or_else(|_| panic!("the Groth16 measurement's {name} is no address"))
    };
    Groth16Measurement {
        selector: u32::from_str_radix(&field("selector"), 16)
            .expect("the Groth16 measurement's selector is 4 hex bytes")
            .to_be_bytes(),
        verify_instruction_data: base64::engine::general_purpose::STANDARD
            .decode(field("verify_instruction_data_base64"))
            .expect("the Groth16 measurement's instruction data is base64"),
        #[cfg(test)]
        verifier_program: key("verifier_program"),
    }
});

/// The verify instruction the mock verifier accepts, for the mock selector:
/// the mock seal's proof of the receipt claim of a zero image id and journal
/// digest. The discriminator is the verify instruction's, which every
/// verifier the router dispatches to shares.
fn mock_verification() -> Vec<u8> {
    let (image_id, journal_digest) = (Digest::ZERO, Digest::ZERO);
    let claim = ReceiptClaim::ok(image_id, MaybePruned::Pruned(journal_digest)).digest();
    [
        &GROTH16_MEASUREMENT.verify_instruction_data[..8],
        // The seal without its selector: the proof.
        &mock_seal(claim)[4..],
        image_id.as_bytes(),
        journal_digest.as_bytes(),
    ]
    .concat()
}

/// The verify instruction the estimate measures the verifier program behind
/// `selector` with.
fn verification_measurement(selector: [u8; 4]) -> Result<Vec<u8>, SubmissionError> {
    if selector == GROTH16_MEASUREMENT.selector {
        Ok(GROTH16_MEASUREMENT.verify_instruction_data.clone())
    } else if selector == MOCK_SELECTOR {
        Ok(mock_verification())
    } else {
        Err(SubmissionError::UnmeasuredProofSelector { selector })
    }
}

/// The compute units the verifier that accepts every proof consumes: the
/// least the runtime lets a builtin consume.
const ACCEPTING_VERIFIER_UNITS: u64 = 1;

declare_process_instruction!(
    AcceptingVerifier,
    ACCEPTING_VERIFIER_UNITS,
    |_invoke_context| { Ok(()) }
);

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
    accounts: BTreeMap<Pubkey, Account>,
}

impl Snapshot {
    /// The data of the account at `address`, when it exists.
    fn data(&self, address: &Pubkey) -> Option<&[u8]> {
        self.accounts.get(address).map(|account| &account.data[..])
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
    plan.upload
        .iter()
        .flat_map(|upload| upload.filling())
        .chain([&plan.settle])
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
        .map(|(program, _)| derive_program_data_address(program))
        .collect()
}

/// The accounts at `keys`, each `None` when it does not exist, at the RPC
/// client's commitment, in one read.
async fn read_accounts(
    rpc: &RpcClient,
    keys: &[Pubkey],
    what: &str,
) -> Result<Vec<Option<Account>>, SubmissionError> {
    Ok(rpc
        .get_multiple_accounts_with_commitment(keys, rpc.commitment())
        .await
        .map_err(|error| SubmissionError::Rpc {
            what: what.to_owned(),
            error: error.to_string(),
        })?
        .value)
}

/// The accounts at `keys`, in one read.
async fn read(rpc: &RpcClient, keys: &BTreeSet<Pubkey>) -> Result<Snapshot, SubmissionError> {
    if keys.len() > MAX_MULTIPLE_ACCOUNTS {
        return Err(SubmissionError::TooManyAccounts {
            accounts: keys.len(),
        });
    }
    let keys: Vec<Pubkey> = keys.iter().copied().collect();
    let accounts = read_accounts(rpc, &keys, "the settlement's accounts").await?;
    Ok(Snapshot {
        accounts: keys
            .into_iter()
            .zip(accounts)
            .filter_map(|(key, account)| Some((key, account?)))
            .collect(),
    })
}

/// The features active on the cluster behind `rpc`: each of the runtime's
/// features whose account records its activation. A feature's account
/// records the slot it activated at only once it has, so every recorded one
/// is active; a local runtime may record another cluster's slots.
async fn read_feature_set(rpc: &RpcClient) -> Result<FeatureSet, SubmissionError> {
    let ids: Vec<Pubkey> = FEATURE_NAMES.keys().copied().collect();
    let mut features = FeatureSet::default();
    for chunk in ids.chunks(MAX_MULTIPLE_ACCOUNTS) {
        let accounts = read_accounts(rpc, chunk, "the cluster's feature accounts").await?;
        for (id, account) in chunk.iter().zip(accounts) {
            let Some(account) = account else { continue };
            let feature: solana_feature_gate_interface::Feature = decode_account(
                Some(&account.data),
                format!("the feature account {id}"),
                |data| bincode::deserialize(data),
            )?;
            if let Some(activated) = feature.activated_at {
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
        .map_err(PlanError::from)?;
    Ok(VersionedTransaction {
        signatures: vec![Signature::default(); usize::from(message.header.num_required_signatures)],
        message: VersionedMessage::V1(message),
    })
}

/// The account that pays for the verification measurement in the local SVM,
/// which funds it: the measurement is the verifier's, whatever the
/// settlement's payer holds.
const MEASUREMENT_PAYER: Pubkey = Pubkey::new_from_array([0x4d; 32]);

/// The compute units the verifier program of `svm` uses to verify one proof
/// of `selector`'s proof system, as one instruction of a transaction with the
/// default heap. A settlement's router invokes the verifier within the
/// settlement transaction, and that invocation is charged no heap of its
/// own: the harness's estimates equal its settlements' units measured this
/// way, and exceed them by the 56 units of the settlement's 256 KiB heap
/// measured under the settlement transaction's settings.
fn verification_units(
    svm: &mut LiteSVM,
    verifier_program: &Pubkey,
    selector: [u8; 4],
) -> Result<u64, SubmissionError> {
    svm.set_account(
        MEASUREMENT_PAYER,
        Account {
            // One SOL: more than the fee of any transaction.
            lamports: 1_000_000_000,
            owner: solana_sdk_ids::system_program::ID,
            ..Account::default()
        },
    )
    .map_err(|e| SubmissionError::LoadAccount {
        address: MEASUREMENT_PAYER,
        error: e.to_string(),
    })?;
    let verify = Instruction {
        program_id: *verifier_program,
        accounts: vec![AccountMeta::new_readonly(
            solana_sdk_ids::system_program::ID,
            false,
        )],
        data: verification_measurement(selector)?,
    };
    let message =
        Message::new_with_blockhash(&[verify], Some(&MEASUREMENT_PAYER), &svm.latest_blockhash());
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

/// A local SVM holding `snapshot`'s accounts, under `features`. The
/// builtins stay the runtime's own.
fn svm_of(snapshot: &Snapshot, features: FeatureSet) -> Result<LiteSVM, SubmissionError> {
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
            .map_err(|e| SubmissionError::LoadAccount {
                address: *address,
                error: e.to_string(),
            })?;
    }
    Ok(svm)
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
        let upload = derive_tx_data_pda(&pa_program, &payer, ESTIMATE_UPLOAD_ID).0;
        let (state_address, _) = derive_pa_state_pda(&pa_program);
        let input = |selector| placeholder_settlement_input(self.transaction.clone(), selector);

        // The read's keys depend on what it reads: the adapter's state names
        // the verifier entry, the entry names the verifier program, and each
        // program names its program data. Read until one read holds every
        // account that the settlement, planned from that read, names.
        let first = plan_settlement(
            self.request_with(
                &input(self.state.proof_selector)?,
                &self.state,
                self.verifier_program,
            ),
            ESTIMATE_UPLOAD_ID,
            0,
        )?;
        let mut keys: BTreeSet<Pubkey> = SYSVARS
            .into_iter()
            .chain(existing_accounts(&first, &pa_program, &upload))
            .collect();
        let (snapshot, state, verifier_program, plan) = loop {
            let snapshot = read(rpc, &keys).await?;
            let state = decode_adapter_state(&state_address, snapshot.data(&state_address))?;
            let entry = derive_verifier_entry_pda(&state.verifier_router, state.proof_selector);
            if keys.insert(entry) {
                // The state names another verifier entry than the read held.
                continue;
            }
            let verifier_program = decode_verifier_entry(&entry, snapshot.data(&entry))?;
            let clock: solana_clock::Clock = decode_account(
                snapshot.data(&solana_sdk_ids::sysvar::clock::ID),
                "the Clock sysvar".to_owned(),
                |data| bincode::deserialize(data),
            )?;
            let plan = plan_settlement(
                self.request_with(&input(state.proof_selector)?, &state, verifier_program),
                ESTIMATE_UPLOAD_ID,
                clock.slot + TXDATA_EXPIRY_SLOTS_DEFAULT,
            )?;
            let needed: BTreeSet<Pubkey> = existing_accounts(&plan, &pa_program, &upload)
                .into_iter()
                .chain(program_data_accounts(&snapshot))
                .collect();
            if needed.is_subset(&keys) {
                break (snapshot, state, verifier_program, plan);
            }
            keys.extend(needed);
        };
        let features = read_feature_set(rpc).await?;

        // Loading the programs and running the settlement is compute, not
        // I/O: it runs off the async runtime's workers.
        tokio::task::spawn_blocking(move || {
            run(
                &snapshot,
                features,
                &plan,
                &pa_program,
                &payer,
                &verifier_program,
                state.proof_selector,
            )
        })
        .await
        .unwrap_or_else(|join| std::panic::resume_unwind(join.into_panic()))
    }
}

/// Runs `plan`, paid for by `payer`, in a local SVM holding `snapshot` under
/// `features`, with `verifier_program`, whose proofs carry `selector`,
/// replaced by one that accepts every proof.
fn run(
    snapshot: &Snapshot,
    features: FeatureSet,
    plan: &SettlementPlan,
    pa_program: &Pubkey,
    payer: &Pubkey,
    verifier_program: &Pubkey,
    selector: [u8; 4],
) -> Result<Estimate, SubmissionError> {
    let mut svm = svm_of(snapshot, features)?;
    let verification = verification_units(&mut svm, verifier_program, selector)?;
    svm.add_builtin(*verifier_program, AcceptingVerifier::register);

    let failed = |failed: FailedTransactionMetadata| -> Result<Estimate, SubmissionError> {
        Ok(
            match settlement_failure(&failed.meta.logs, pa_program, verifier_program)? {
                Some(failure) => Estimate::Failed(failure),
                None => Estimate::Other {
                    error: failed.err,
                    logs: failed.meta.logs,
                },
            },
        )
    };
    for planned in plan.upload.iter().flat_map(|upload| upload.filling()) {
        if let Err(failure) = svm.send_transaction(unsigned(planned, payer, &svm)?) {
            return failed(failure);
        }
    }
    // The verification's compute stands in for the accepting verifier's. One
    // using more than the settlement's whole budget leaves it none.
    let held_back = verification.saturating_sub(ACCEPTING_VERIFIER_UNITS);
    let settle = PlannedTransaction {
        instructions: plan.settle.instructions.clone(),
        config: TransactionConfig {
            compute_unit_limit: Some(
                u32::try_from(held_back)
                    .map_or(0, |units| SETTLE_COMPUTE_UNIT_LIMIT.saturating_sub(units)),
            ),
            ..plan.settle.config
        },
    };
    match svm.send_transaction(unsigned(&settle, payer, &svm)?) {
        Ok(meta) => Ok(Estimate::Executable {
            units_consumed: meta.compute_units_consumed + held_back,
        }),
        Err(failure) => failed(failure),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The devnet verifier program verifies the Groth16 measurement proof
    /// with the compute its verifications use in devnet settlements: 99,541
    /// units in both of the staging adapter's settlements at slots 518006892
    /// and 518008799.
    #[tokio::test]
    #[ignore = "needs a Helius endpoint: run with --ignored, HELIUS_DEVNET_URL set"]
    async fn the_groth16_measurement_uses_a_devnet_verifications_compute() {
        let url = std::env::var("HELIUS_DEVNET_URL").expect(
            "HELIUS_DEVNET_URL is unset: set it to the Helius devnet endpoint \
             (scripts/helius.env in the umbrella repository)",
        );
        let rpc = RpcClient::new(url);
        let measurement = &*GROTH16_MEASUREMENT;
        let verifier = measurement.verifier_program;
        let keys: BTreeSet<Pubkey> = SYSVARS
            .into_iter()
            .chain([verifier, derive_program_data_address(&verifier)])
            .collect();
        let snapshot = read(&rpc, &keys).await.unwrap();
        let features = read_feature_set(&rpc).await.unwrap();
        let mut svm = svm_of(&snapshot, features).unwrap();

        let units = verification_units(&mut svm, &verifier, measurement.selector).unwrap();

        println!("a Groth16 verification on devnet: {units} compute units");
        assert_eq!(units, 99_541);
    }
}
