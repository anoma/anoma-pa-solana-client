//! Why an instance could not be made or could not answer.

use std::fmt;

use solana_cluster_type::ClusterType;
use solana_pubkey::Pubkey;
use solana_transaction_error::TransactionError;

pub type BindingsResult<T> = Result<T, BindingsError>;

#[derive(Debug)]
pub enum BindingsError {
    /// The genesis hash the RPC transport returned is no known cluster's.
    ChainIdUnknown,
    /// An RPC request failed: retrying may succeed.
    RpcTransportError { what: String, error: String },
    /// The environment records no deployed adapter on the cluster.
    UnsupportedChain(ClusterType),
    /// An account the answer is read from does not exist.
    AccountMissing { address: Pubkey },
    /// What the chain holds does not decode.
    Decode { what: String, error: String },
    /// A simulated instruction failed.
    Simulation {
        error: TransactionError,
        logs: Vec<String>,
    },
}

impl fmt::Display for BindingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ChainIdUnknown => write!(
                f,
                "The genesis hash returned by the RPC transport is not in the list of known \
                 clusters."
            ),
            Self::RpcTransportError { what, error } => {
                write!(
                    f,
                    "The RPC transport returned an error fetching {what}: {error}"
                )
            }
            Self::UnsupportedChain(cluster) => write!(
                f,
                "The current protocol adapter version has not been deployed on the provided \
                 chain '{cluster:?}'."
            ),
            Self::AccountMissing { address } => write!(f, "the account {address} does not exist"),
            Self::Decode { what, error } => write!(f, "{what} does not decode: {error}"),
            Self::Simulation { error, logs } => {
                write!(f, "the simulation failed: {error}; logs: {logs:?}")
            }
        }
    }
}

impl std::error::Error for BindingsError {}

/// The error an RPC request for `what` failed with.
pub(crate) fn rpc_transport_error(
    what: &str,
) -> impl FnOnce(solana_rpc_client_api::client_error::Error) -> BindingsError + '_ {
    move |error| BindingsError::RpcTransportError {
        what: what.to_owned(),
        error: error.to_string(),
    }
}
