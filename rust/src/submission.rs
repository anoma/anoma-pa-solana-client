//! A settlement as every submitter prepares it, whatever the application: the
//! adapter's state read from the chain, each external call's accounts from
//! the forwarder registered for its program, and the settlement planned or
//! estimated from them. Sending is the submitter's own.

use std::fmt;

use anoma_rm_risc0::transaction::Transaction;
use solana_pubkey::Pubkey;
use solana_rpc_client::nonblocking::rpc_client::RpcClient;

use anchor::error::ERROR_CODE_OFFSET;
use anchor::AccountDeserialize;

use crate::accounts::decode_pa_state;
use crate::forwarders::{AccountsError, Forwarders};
use crate::generated::protocol_adapter::accounts::PAStateAccount;
use crate::generated::protocol_adapter::error::ProtocolAdapterError;
use crate::generated::verifier_router::accounts::VerifierEntry;
use crate::pda::{derive_pa_state_pda, derive_verifier_entry_pda};
use crate::settlement::{
    plan_settlement, CallAccounts, PlanError, SettlementPlan, SettlementRequest,
};
use crate::settlement_input::{
    external_calls, proven_settlement_input_len, settled_resources, settlement_input,
    SettledResources, SettlementInputError,
};

/// Why a settlement could not be prepared, planned or estimated.
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
    /// The estimate's read names more accounts than one RPC read returns
    /// (`MAX_MULTIPLE_ACCOUNTS`).
    #[cfg(feature = "estimate")]
    TooManyAccounts { accounts: usize },
    /// The estimate has no proof to measure a verification of the adapter's
    /// proof selector with.
    #[cfg(feature = "estimate")]
    UnmeasuredProofSelector { selector: [u8; 4] },
    /// An account the cluster holds cannot be loaded into the estimate's
    /// local SVM (a program its runtime does not load).
    #[cfg(feature = "estimate")]
    LoadAccount { address: Pubkey, error: String },
    /// The verifier program did not verify the proof the estimate measures a
    /// verification's compute on.
    #[cfg(feature = "estimate")]
    VerifierMeasurement {
        verifier_program: Pubkey,
        error: String,
        logs: Vec<String>,
    },
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
            #[cfg(feature = "estimate")]
            Self::TooManyAccounts { accounts } => write!(
                f,
                "the settlement's estimate reads {accounts} accounts; one RPC read returns at most {}",
                crate::estimate::MAX_MULTIPLE_ACCOUNTS
            ),
            #[cfg(feature = "estimate")]
            Self::UnmeasuredProofSelector { selector } => write!(
                f,
                "the estimate has no proof to measure a verification of the selector {selector:02x?} with"
            ),
            #[cfg(feature = "estimate")]
            Self::LoadAccount { address, error } => write!(
                f,
                "the account {address} cannot be loaded into the estimate's local SVM: {error}"
            ),
            #[cfg(feature = "estimate")]
            Self::VerifierMeasurement {
                verifier_program,
                error,
                logs,
            } => write!(
                f,
                "the verifier program {verifier_program} refused the measurement proof: {error}; logs: {logs:?}"
            ),
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

/// The account `what`, `data` its data when it exists, as `decode` reads it.
pub(crate) fn decode_account<T, E: fmt::Display>(
    data: Option<&[u8]>,
    what: String,
    decode: impl FnOnce(&[u8]) -> Result<T, E>,
) -> Result<T, SubmissionError> {
    let Some(data) = data else {
        return Err(SubmissionError::MissingAccount { what });
    };
    decode(data).map_err(|e| SubmissionError::Decode {
        what,
        error: e.to_string(),
    })
}

/// The adapter's state at `address`, `data` its account's data when it
/// exists.
pub(crate) fn decode_adapter_state(
    address: &Pubkey,
    data: Option<&[u8]>,
) -> Result<PAStateAccount, SubmissionError> {
    decode_account(
        data,
        format!("the adapter's state {address}"),
        decode_pa_state,
    )
}

/// The verifier program the router's verifier entry at `entry` names, `data`
/// the entry's data when it exists.
pub(crate) fn decode_verifier_entry(
    entry: &Pubkey,
    data: Option<&[u8]>,
) -> Result<Pubkey, SubmissionError> {
    decode_account(
        data,
        format!("the router's verifier entry {entry}"),
        |mut data| VerifierEntry::try_deserialize(&mut data),
    )
    .map(|entry| entry.verifier)
}

/// The data of the account at `address`, at the RPC client's commitment, or
/// `None` when it does not exist.
async fn read_account_data(
    rpc: &RpcClient,
    address: &Pubkey,
) -> Result<Option<Vec<u8>>, SubmissionError> {
    Ok(rpc
        .get_account_with_commitment(address, rpc.commitment())
        .await
        .map_err(|error| SubmissionError::Rpc {
            what: format!("the account {address}"),
            error: error.to_string(),
        })?
        .value
        .map(|account| account.data))
}

/// The state of the adapter `pa_program`.
pub async fn read_adapter_state(
    rpc: &RpcClient,
    pa_program: &Pubkey,
) -> Result<PAStateAccount, SubmissionError> {
    let (address, _) = derive_pa_state_pda(pa_program);
    decode_adapter_state(&address, read_account_data(rpc, &address).await?.as_deref())
}

/// The verifier program the entry of `router` for `selector` names; an
/// adapter's are the router and the selector its state records.
pub async fn read_verifier_program(
    rpc: &RpcClient,
    router: &Pubkey,
    selector: [u8; 4],
) -> Result<Pubkey, SubmissionError> {
    let entry = derive_verifier_entry_pda(router, selector);
    decode_verifier_entry(&entry, read_account_data(rpc, &entry).await?.as_deref())
}

/// A transaction ready to be planned: the adapter's state and the accounts of
/// its external calls, read from the chain.
pub struct Submission {
    pub(crate) pa_program: Pubkey,
    pub(crate) payer: Pubkey,
    pub(crate) verifier_program: Pubkey,
    pub(crate) state: PAStateAccount,
    pub(crate) transaction: Transaction,
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
        self.request_with(input, &self.state, self.verifier_program)
    }

    /// The request settling `input` against `state`, whose proofs
    /// `verifier_program` verifies.
    pub(crate) fn request_with<'a>(
        &'a self,
        input: &'a [u8],
        state: &'a PAStateAccount,
        verifier_program: Pubkey,
    ) -> SettlementRequest<'a> {
        SettlementRequest {
            pa_program: self.pa_program,
            payer: self.payer,
            input,
            state,
            verifier_program,
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
}

/// The failure that ended a settlement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// The adapter failed with this error of its own.
    Adapter(ProtocolAdapterError),
    /// The adapter failed with one of Anchor's own errors, custom error
    /// `code` below `ERROR_CODE_OFFSET`, where a program's own errors start
    /// (an account constraint the instruction's accounts break).
    Anchor { code: u32 },
    /// The verifier refused the aggregation proof with its custom error
    /// `code`.
    ProofRefused { code: u32 },
    /// Another program failed with custom error `code`: a forwarder, or the
    /// adapter with a code its IDL names no error for.
    Program { program: Pubkey, code: u32 },
}

/// The failure that ended a settlement of the adapter `pa_program`, whose
/// proofs `verifier_program` verifies, from the runtime's log of it (an
/// estimate's, or a send's preflight simulation): `None` when no program
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
            match ProtocolAdapterError::try_from(code) {
                Ok(error) => Failure::Adapter(error),
                Err(code) if code < ERROR_CODE_OFFSET => Failure::Anchor { code },
                Err(code) => Failure::Program { program, code },
            }
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
        let code = u32::from(ProtocolAdapterError::NonExistingRoot);
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
        let code = u32::from(ProtocolAdapterError::NonExistingRoot);
        assert_eq!(
            failure_of(ADAPTER, code),
            Some(Failure::Adapter(ProtocolAdapterError::NonExistingRoot))
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
        // 2006: Anchor's ConstraintSeeds, which the adapter returns for a
        // PDA account at the wrong address.
        assert_eq!(
            failure_of(ADAPTER, 2006),
            Some(Failure::Anchor { code: 2006 })
        );
        // A code of the adapter's past every error its IDL names.
        assert_eq!(
            failure_of(ADAPTER, 7000),
            Some(Failure::Program {
                program: ADAPTER,
                code: 7000
            })
        );
    }

    #[test]
    fn a_programs_log_line_reading_like_a_failure_is_not_the_failure() {
        let code = u32::from(ProtocolAdapterError::ForwarderCallOutputMismatch);
        let logs = [
            invoke(ADAPTER, 1),
            invoke(FORWARDER, 2),
            // A program's own log lines start "Program log: ", whatever follows.
            format!(
                "Program log: {}",
                failure(ADAPTER, u32::from(ProtocolAdapterError::NonExistingRoot))
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
            failure(ADAPTER, u32::from(ProtocolAdapterError::NonExistingRoot)),
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
