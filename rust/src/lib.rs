//! Client bindings for the Solana Anoma Protocol Adapter.
//!
//! See `REQUIREMENTS.md` at the repository root for the full surface specification.
//! The adapter's id, accounts, events, types, constants, errors and instruction
//! accounts and arguments are generated from its IDL (`generated`); the rest is
//! what the IDL does not describe: settlement planning, the CPI event framing,
//! the PDAs the instructions take as remaining accounts, the Merkle helpers, the
//! settlement input, RPC submission, and the external calls' wire format.

pub mod constants;
pub mod external_call;
pub mod merkle;
#[cfg(feature = "arm")]
pub mod settlement_input;

#[cfg(feature = "solana")]
pub mod accounts;
#[cfg(feature = "solana")]
pub mod events;
#[cfg(feature = "rpc")]
pub mod forwarders;
#[cfg(feature = "solana")]
pub mod generated;
#[cfg(feature = "solana")]
pub mod instructions;
#[cfg(feature = "solana")]
pub mod pda;
#[cfg(feature = "solana")]
pub mod settlement;
#[cfg(feature = "rpc")]
pub mod submission;

pub use constants::*;
pub use external_call::{OutputMode, SolanaExternalCall};
pub use merkle::{
    depth_for_leaves, hash_two, merkle_path, path_root, zero_hashes, CommitmentTreeState,
    MerkleError, PADDING_LEAF,
};

#[cfg(feature = "solana")]
pub use accounts::{decode_pa_state, StateDecodeError};
#[cfg(feature = "solana")]
pub use events::{cpi_event_discriminator, decode_cpi_event, EventDecodeError};
#[cfg(feature = "solana")]
pub use instructions::{
    deny_logic_refs_ix, initialize_ix, pause_ix, set_kind_tree_root_ix, settle_from_txdata_ix,
    settle_ix, simulate_execute_ix, txdata_close_ix, txdata_init_ix, txdata_write_ix, unpause_ix,
    SettlementAccounts,
};
#[cfg(feature = "solana")]
pub use pda::{
    derive_event_authority_pda, derive_nullifier_pda, derive_pa_state_pda,
    derive_program_data_address, derive_root_marker_pda, derive_tx_data_pda,
    derive_upgrade_authority_pda, derive_verifier_entry_pda, derive_verifier_router_pdas,
};
#[cfg(feature = "solana")]
pub use settlement::{
    plan_settlement, plan_simulation, transaction_size, PlanError, PlannedTransaction,
    SettlementPlan, SettlementRequest, Upload,
};
