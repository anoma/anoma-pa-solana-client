//! Why an instance could not be made or could not answer.

use solana_cluster_type::ClusterType;
use solana_pubkey::Pubkey;
use solana_rpc_client_api::client_error::Error as ClientError;
use solana_transaction_error::TransactionError;
use thiserror::Error;

pub type BindingsResult<T> = Result<T, BindingsError>;

#[derive(Error, Debug)]
pub enum BindingsError {
    /// The genesis hash the RPC transport returned is no known cluster's.
    #[error(
        "The genesis hash returned by the RPC transport is not in the list of known clusters."
    )]
    ChainIdUnknown,
    /// An RPC request failed: retrying may succeed.
    #[error("The RPC transport returned an error fetching {what}: {error}")]
    RpcTransportError {
        what: String,
        #[source]
        error: ClientError,
    },
    /// The environment records no deployed adapter on the cluster.
    #[error(
        "The current protocol adapter version has not been deployed on the provided chain \
         '{0:?}'."
    )]
    UnsupportedChain(ClusterType),
    /// An account the answer is read from does not exist.
    #[error("the account {address} does not exist")]
    AccountMissing { address: Pubkey },
    /// What the chain holds does not decode.
    #[error("{what} does not decode: {error}")]
    Decode { what: String, error: String },
    /// A simulated instruction failed.
    #[error("the simulation failed: {error}; logs: {logs:?}")]
    Simulation {
        error: TransactionError,
        logs: Vec<String>,
    },
}

/// The error an RPC request for `what` failed with.
pub(crate) fn rpc_transport_error(what: &str) -> impl FnOnce(ClientError) -> BindingsError + '_ {
    move |error| BindingsError::RpcTransportError {
        what: what.to_owned(),
        error,
    }
}
