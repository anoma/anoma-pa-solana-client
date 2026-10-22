//! A settlement as every submitter prepares it, whatever the application: the
//! adapter's state read from the chain, each external call's accounts from
//! the forwarder registered for its program, and the settlement planned or
//! simulated from them. Sending is the submitter's own.

use std::fmt;

use anoma_rm_risc0::transaction::Transaction;
use solana_message::{Hash, VersionedMessage};
use solana_pubkey::Pubkey;
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_rpc_client_api::config::RpcSimulateTransactionConfig;
use solana_signature::Signature;
use solana_transaction::versioned::VersionedTransaction;
use solana_transaction_error::TransactionError;

use crate::accounts::{decode_pa_state, decode_verifier_entry, PAStateAccount};
use crate::errors::PaError;
use crate::forwarders::{AccountsError, Forwarders};
use crate::pda::{derive_pa_state_pda, derive_verifier_entry_pda};
use crate::settlement::{
    plan_settlement, plan_simulation, CallAccounts, PlanError, PlannedTransaction, SettlementPlan,
    SettlementRequest,
};
use crate::settlement_input::{
    external_calls, proven_settlement_input_len, settled_resources, settlement_input,
    simulation_input, SettledResources, SettlementInputError,
};

/// Why a settlement could not be prepared, planned or simulated.
#[derive(Debug)]
pub enum SubmissionError {
    /// An RPC request failed: retrying may succeed.
    Rpc { what: String, error: String },
    /// An account the settlement reads does not exist.
    MissingAccount { what: String },
    /// An account the chain holds does not decode.
    Decode { what: String, error: String },
    /// The transaction's calls have no accounts.
    Accounts(AccountsError),
    /// The transaction is not one the adapter settles.
    Input(SettlementInputError),
    /// The settlement cannot be planned.
    Plan(PlanError),
    /// The runtime truncated the log, so it does not say which program failed:
    /// past its limit the runtime drops lines, the deepest failure's among
    /// them, while a shorter later one can still fit.
    TruncatedLog,
}

impl fmt::Display for SubmissionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rpc { what, error } => write!(f, "failed to fetch {what}: {error}"),
            Self::MissingAccount { what } => write!(f, "{what} does not exist"),
            Self::Decode { what, error } => write!(f, "{what} does not decode: {error}"),
            Self::Accounts(e) => write!(f, "the external calls' accounts: {e}"),
            Self::Input(e) => write!(f, "the transaction: {e}"),
            Self::Plan(e) => write!(f, "the settlement cannot be planned: {e}"),
            Self::TruncatedLog => write!(
                f,
                "the runtime truncated the log, so it does not say which program failed"
            ),
        }
    }
}

impl std::error::Error for SubmissionError {}

impl From<AccountsError> for SubmissionError {
    fn from(e: AccountsError) -> Self {
        Self::Accounts(e)
    }
}

impl From<SettlementInputError> for SubmissionError {
    fn from(e: SettlementInputError) -> Self {
        Self::Input(e)
    }
}

impl From<PlanError> for SubmissionError {
    fn from(e: PlanError) -> Self {
        Self::Plan(e)
    }
}

/// The account at `address`, `what` the settlement reads, as `decode` reads
/// it, at the RPC client's commitment.
async fn read_account<T, E: fmt::Display>(
    rpc: &RpcClient,
    address: &Pubkey,
    what: String,
    decode: impl FnOnce(&[u8]) -> Result<T, E>,
) -> Result<T, SubmissionError> {
    let account = rpc
        .get_account_with_commitment(address, rpc.commitment())
        .await
        .map_err(|error| SubmissionError::Rpc {
            what: what.clone(),
            error: error.to_string(),
        })?
        .value;
    let Some(account) = account else {
        return Err(SubmissionError::MissingAccount { what });
    };
    decode(&account.data).map_err(|e| SubmissionError::Decode {
        what,
        error: e.to_string(),
    })
}

/// The state of the adapter `pa_program`.
pub async fn read_adapter_state(
    rpc: &RpcClient,
    pa_program: &Pubkey,
) -> Result<PAStateAccount, SubmissionError> {
    let (address, _) = derive_pa_state_pda(pa_program);
    let what = format!("the adapter's state {address}");
    read_account(rpc, &address, what, decode_pa_state).await
}

/// The verifier program the entry of `router` for `selector` names; an
/// adapter's are the router and the selector its state records.
pub async fn read_verifier_program(
    rpc: &RpcClient,
    router: &Pubkey,
    selector: [u8; 4],
) -> Result<Pubkey, SubmissionError> {
    let entry = derive_verifier_entry_pda(router, selector);
    let what = format!("the router's verifier entry {entry}");
    let decoded = read_account(rpc, &entry, what, decode_verifier_entry).await?;
    Ok(Pubkey::new_from_array(decoded.verifier))
}

/// A transaction ready to be planned: the adapter's state and the accounts of
/// its external calls, read from the chain.
pub struct Submission {
    pa_program: Pubkey,
    payer: Pubkey,
    verifier_program: Pubkey,
    state: PAStateAccount,
    transaction: Transaction,
    resources: SettledResources,
    accounts: CallAccounts,
}

impl Submission {
    /// Prepares `transaction`, an aggregated transaction, for the adapter
    /// `pa_program` as `state` records it (`read_adapter_state`): `payer`
    /// pays for and signs the settlement, `verifier_program` is the one the
    /// router names for the adapter's selector (`read_verifier_program`), and
    /// `forwarders` supply the external calls' accounts.
    pub async fn prepare(
        rpc: &RpcClient,
        forwarders: &Forwarders,
        pa_program: Pubkey,
        payer: Pubkey,
        state: PAStateAccount,
        verifier_program: Pubkey,
        transaction: Transaction,
    ) -> Result<Self, SubmissionError> {
        let resources = settled_resources(&transaction)?;
        let calls = external_calls(&transaction)?;
        let accounts = forwarders.accounts(rpc, &calls).await?;
        Ok(Self {
            pa_program,
            payer,
            verifier_program,
            state,
            transaction,
            resources,
            accounts,
        })
    }

    /// The adapter's state the settlement is planned against.
    pub fn state(&self) -> &PAStateAccount {
        &self.state
    }

    /// The resources the settlement consumes and creates.
    pub fn resources(&self) -> &SettledResources {
        &self.resources
    }

    fn request<'a>(&'a self, input: &'a [u8]) -> SettlementRequest<'a> {
        SettlementRequest {
            pa_program: self.pa_program,
            payer: self.payer,
            input,
            state: &self.state,
            verifier_program: self.verifier_program,
            nullifiers: &self.resources.nullifiers,
            consumed_roots: &self.resources.consumed_roots,
            created: &self.resources.created,
            accounts: self.accounts.clone(),
        }
    }

    /// The settlement of the transaction, which must be proven: in one
    /// transaction when it fits, otherwise its data uploaded under
    /// `upload_id`, an upload that expires at `expires_slot`.
    pub fn plan_settlement(
        &self,
        upload_id: u64,
        expires_slot: u64,
    ) -> Result<SettlementPlan, SubmissionError> {
        let input = settlement_input(self.transaction.clone())?;
        Ok(plan_settlement(
            self.request(&input),
            upload_id,
            expires_slot,
        )?)
    }

    /// How many transactions the settlement of the transaction sends once it
    /// is proven, known before it is: `plan_settlement`'s for an input of the
    /// proven input's length (`proven_settlement_input_len`).
    pub fn proven_settlement_transaction_count(&self) -> Result<usize, SubmissionError> {
        // A plan's transactions depend on its input's length alone, and the
        // upload id and expiry slot are fixed-width, so any input of that
        // length and any upload give them, as any blockhash gives a message's
        // size.
        let input = vec![0; proven_settlement_input_len(&self.transaction)?];
        Ok(plan_settlement(self.request(&input), 0, 0)?.transaction_count())
    }

    /// The transaction simulating the settlement (`plan_simulation`), skipping
    /// the RISC Zero proof's verification when
    /// `skip_risc_zero_proof_verification`. An unproven transaction (its
    /// aggregation proof empty) carries the adapter's proof selector instead.
    pub fn plan_simulation(
        &self,
        skip_risc_zero_proof_verification: bool,
    ) -> Result<PlannedTransaction, SubmissionError> {
        let input = simulation_input(self.transaction.clone(), self.state.proof_selector)?;
        Ok(plan_simulation(
            self.request(&input),
            skip_risc_zero_proof_verification,
        )?)
    }

    /// Simulates the settlement (pa-evm's `simulateExecute`) on the state
    /// behind `rpc` at its commitment, as `plan_simulation` plans it:
    /// unsigned and on the latest blockhash, which the RPC fills in.
    pub async fn simulate(
        &self,
        rpc: &RpcClient,
        skip_risc_zero_proof_verification: bool,
    ) -> Result<SimulationOutcome, SubmissionError> {
        let message = self
            .plan_simulation(skip_risc_zero_proof_verification)?
            .message(&self.payer, Hash::default())
            .map_err(PlanError::from)?;
        let unsigned = VersionedTransaction {
            signatures: vec![
                Signature::default();
                usize::from(message.header.num_required_signatures)
            ],
            message: VersionedMessage::V1(message),
        };
        let simulated = rpc
            .simulate_transaction_with_config(
                &unsigned,
                RpcSimulateTransactionConfig {
                    sig_verify: false,
                    replace_recent_blockhash: true,
                    commitment: Some(rpc.commitment()),
                    ..RpcSimulateTransactionConfig::default()
                },
            )
            .await
            .map_err(|error| SubmissionError::Rpc {
                what: "the simulation".to_owned(),
                error: error.to_string(),
            })?
            .value;
        let logs = simulated.logs.unwrap_or_default();
        Ok(
            match settlement_failure(&logs, &self.pa_program, &self.verifier_program)? {
                Some(Failure::Adapter(PaError::Simulated)) => SimulationOutcome::Executable {
                    units_consumed: simulated.units_consumed,
                },
                Some(failure) => SimulationOutcome::Failed(failure),
                None => SimulationOutcome::Other {
                    error: simulated.err.map(TransactionError::from),
                    logs,
                },
            },
        )
    }
}

/// How a simulation of a settlement ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimulationOutcome {
    /// `simulate_execute` ended with `Simulated`: the settlement would
    /// execute, using these compute units in all.
    Executable { units_consumed: Option<u64> },
    /// The settlement would fail.
    Failed(Failure),
    /// The transaction failed without any program's custom error, or did not
    /// fail at all, which `simulate_execute` never does.
    Other {
        error: Option<TransactionError>,
        logs: Vec<String>,
    },
}

/// The failure that ended a settlement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// The adapter failed with this error of its own.
    Adapter(PaError),
    /// The verifier refused the aggregation proof with its custom error
    /// `code`.
    ProofRefused { code: u32 },
    /// Another program failed with custom error `code`: a forwarder, or the
    /// adapter with a code no `PaError` names.
    Program { program: Pubkey, code: u32 },
}

/// The failure that ended a settlement of the adapter `pa_program`, whose
/// proofs `verifier_program` verifies, from the runtime's log of it (a
/// simulation's, or a send's preflight simulation): `None` when no program
/// failed with a custom error.
pub fn settlement_failure(
    logs: &[String],
    pa_program: &Pubkey,
    verifier_program: &Pubkey,
) -> Result<Option<Failure>, SubmissionError> {
    Ok(first_failure(logs)?.map(|(program, code)| {
        if program == *verifier_program {
            Failure::ProofRefused { code }
        } else if program == *pa_program {
            PaError::from_code(code)
                .map(Failure::Adapter)
                .unwrap_or(Failure::Program { program, code })
        } else {
            Failure::Program { program, code }
        }
    }))
}

/// The program and custom error code of the failure that ended a transaction:
/// the deepest failure, which the runtime logs first as the program that
/// failed and the custom error it returned (a line no program can write: a
/// program's own lines start `Program log: `). A log the runtime truncated is
/// an error, since the first failure line left may then carry another
/// program's code.
fn first_failure(logs: &[String]) -> Result<Option<(Pubkey, u32)>, SubmissionError> {
    if logs.iter().any(|line| line == "Log truncated") {
        return Err(SubmissionError::TruncatedLog);
    }
    Ok(logs
        .iter()
        .find_map(|line| {
            let (failed, error) = line.strip_prefix("Program ")?.split_once(" failed: ")?;
            Some((failed.parse::<Pubkey>().ok()?, error))
        })
        .and_then(|(failed, error)| {
            let code =
                u32::from_str_radix(error.strip_prefix("custom program error: 0x")?, 16).ok()?;
            Some((failed, code))
        }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADAPTER: Pubkey = Pubkey::new_from_array([1; 32]);
    const FORWARDER: Pubkey = Pubkey::new_from_array([3; 32]);

    fn invoke(program: Pubkey, depth: u8) -> String {
        format!("Program {program} invoke [{depth}]")
    }

    /// The runtime's line for `program` failing with custom error `code`.
    fn failure(program: Pubkey, code: u32) -> String {
        format!("Program {program} failed: custom program error: {code:#x}")
    }

    /// The submission of `tx`, which makes no external call, so preparing it
    /// reads nothing through the RPC.
    async fn submission_of(tx: Transaction) -> Submission {
        Submission::prepare(
            &RpcClient::new("http://127.0.0.1:1".to_owned()),
            &Forwarders::default(),
            ADAPTER,
            Pubkey::new_from_array([8; 32]),
            crate::settlement::tests::state_with(&[]),
            Pubkey::new_from_array([6; 32]),
            tx,
        )
        .await
        .expect("a transaction without external calls prepares offline")
    }

    #[tokio::test]
    async fn an_unproven_transaction_settles_in_as_many_transactions_as_its_proven_one() {
        // One resource each way settles inline; twenty each way upload.
        for (count, uploads) in [(1, false), (20, true)] {
            let proven = crate::settlement_input::tests::proven_trivial_transaction(count).await;
            let mut unproven = proven.clone();
            unproven.aggregation.as_mut().unwrap().proof = vec![];

            let plan = submission_of(proven)
                .await
                .plan_settlement(7, 500)
                .expect("the proven settlement plans");
            assert_eq!(
                plan.upload.is_some(),
                uploads,
                "{count} resources each way upload: {uploads}"
            );
            assert_eq!(
                submission_of(unproven)
                    .await
                    .proven_settlement_transaction_count()
                    .expect("the unproven settlement plans"),
                plan.transaction_count(),
                "{count} resources each way"
            );
        }
    }

    #[test]
    fn the_first_failure_is_the_deepest() {
        let code = PaError::NonExistingRoot.code();
        let logs = [
            invoke(ADAPTER, 1),
            invoke(FORWARDER, 2),
            failure(FORWARDER, 7),
            failure(ADAPTER, code),
        ];
        assert_eq!(first_failure(&logs).unwrap(), Some((FORWARDER, 7)));
    }

    #[test]
    fn a_settlements_failure_is_the_failing_programs() {
        const VERIFIER: Pubkey = Pubkey::new_from_array([2; 32]);
        let failure_of = |program: Pubkey, code: u32| {
            let logs = [
                invoke(ADAPTER, 1),
                invoke(program, 2),
                failure(program, code),
            ];
            settlement_failure(&logs, &ADAPTER, &VERIFIER).unwrap()
        };
        let code = PaError::NonExistingRoot.code();
        assert_eq!(
            failure_of(ADAPTER, code),
            Some(Failure::Adapter(PaError::NonExistingRoot))
        );
        // 6600: the mock verifier's ClaimDigestMismatch.
        assert_eq!(
            failure_of(VERIFIER, 6600),
            Some(Failure::ProofRefused { code: 6600 })
        );
        assert_eq!(
            failure_of(FORWARDER, code),
            Some(Failure::Program {
                program: FORWARDER,
                code
            })
        );
        // A code of the adapter's that no `PaError` names.
        assert_eq!(
            failure_of(ADAPTER, 1),
            Some(Failure::Program {
                program: ADAPTER,
                code: 1
            })
        );
    }

    #[test]
    fn a_programs_log_line_reading_like_a_failure_is_not_the_failure() {
        let code = PaError::ForwarderCallOutputMismatch.code();
        let logs = [
            invoke(ADAPTER, 1),
            invoke(FORWARDER, 2),
            // A program's own log lines start "Program log: ", whatever follows.
            format!(
                "Program log: {}",
                failure(ADAPTER, PaError::NonExistingRoot.code())
            ),
            format!("Program {FORWARDER} success"),
            failure(ADAPTER, code),
        ];
        assert_eq!(first_failure(&logs).unwrap(), Some((ADAPTER, code)));
    }

    #[test]
    fn a_truncated_log_names_no_failure() {
        let logs = [
            invoke(ADAPTER, 1),
            invoke(FORWARDER, 2),
            "Log truncated".to_owned(),
            failure(ADAPTER, PaError::NonExistingRoot.code()),
        ];
        assert!(matches!(
            first_failure(&logs),
            Err(SubmissionError::TruncatedLog)
        ));
    }

    #[test]
    fn a_failure_without_a_custom_error_names_no_failure() {
        let logs = [
            invoke(ADAPTER, 1),
            format!("Program {ADAPTER} failed: insufficient funds"),
        ];
        assert_eq!(first_failure(&logs).unwrap(), None);
    }
}
