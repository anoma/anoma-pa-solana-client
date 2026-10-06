//! Client bindings for the Solana Anoma Protocol Adapter.
//!
//! See `REQUIREMENTS.md` at the repository root for the full surface specification.
//! This crate is the canonical home for instruction builders, account decoders, PDA
//! derivation, wire-format types, and PA-owned constants. Integrators should depend
//! on this crate rather than re-implementing PA-binding logic inline.

pub mod accounts;
pub mod constants;
pub mod cursor;
pub mod discriminator;
pub mod errors;
pub mod events;
pub mod external_call;
pub mod merkle;
#[cfg(feature = "arm")]
pub mod settlement_input;
pub mod u256;

#[cfg(feature = "solana")]
pub mod instructions;
#[cfg(feature = "solana")]
pub mod pda;
#[cfg(feature = "solana")]
pub mod program_ids;
#[cfg(feature = "solana")]
pub mod settlement;

pub use accounts::{
    decode_pa_state, decode_verifier_entry, encode_pa_state, DecodeError, PAStateAccount,
    VerifierEntryAccount, PA_STATE_SCHEMA_VERSION,
};
pub use constants::*;
pub use discriminator::{anchor_account_disc, anchor_event_disc, anchor_instruction_disc};
pub use errors::{PaError, ANCHOR_ERROR_CODE_OFFSET};
pub use events::{
    decode_cpi_event, decode_event_instruction, decode_ownership_transferred, decode_upgraded,
    ActionExecutedEvent, CommitmentTreeRootAddedEvent, EventDecodeError,
    ForwarderCallExecutedEvent, KindTableCommitmentUpdatedEvent, LogicRefDeniedEvent,
    OwnershipTransferredEvent, PaEvent, PauseEvent, PayloadEvent, TransactionExecutedEvent,
    UpgradedEvent, EVENT_IX_TAG,
};
pub use external_call::{OutputMode, SolanaExternalCall};
pub use merkle::{
    depth_for_leaves, hash_two, merkle_path, path_root, zero_hashes, CommitmentTreeState,
    MerkleError, PADDING_LEAF,
};
pub use u256::{U256OutOfRange, U256};

#[cfg(feature = "solana")]
pub use instructions::{
    deny_logic_ref_ix, initialize_ix, pause_ix, set_kind_table_commitment_ix,
    settle_from_txdata_ix, txdata_close_ix, txdata_init_ix, txdata_write_ix, unpause_ix,
};
#[cfg(feature = "solana")]
pub use pda::{
    derive_event_authority_pda, derive_nullifier_pda, derive_pa_state_pda,
    derive_program_data_address, derive_root_marker_pda, derive_tx_data_pda,
    derive_upgrade_authority_pda, derive_verifier_entry_pda, derive_verifier_router_pdas,
};
#[cfg(feature = "solana")]
pub use program_ids::*;
#[cfg(feature = "solana")]
pub use settlement::{
    adapter_settlement_lookup_keys, plan_settlement, SettlementPlan, SettlementRequest,
};
