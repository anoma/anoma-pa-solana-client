//! Client bindings for the Solana Anoma Protocol Adapter and SPL Token Forwarder.
//!
//! See `REQUIREMENTS.md` at the repository root for the full surface specification.
//! This crate is the canonical home for instruction builders, account decoders, PDA
//! derivation, wire-format types, and PA-owned constants. Integrators should depend
//! on this crate rather than re-implementing PA-binding logic inline.

pub mod constants;
pub mod discriminator;
pub mod pda;
pub mod program_ids;

pub use constants::*;
pub use discriminator::{anchor_event_disc, anchor_instruction_disc};
pub use pda::{
    derive_associated_token_address, derive_forwarder_config_pda, derive_forwarder_escrow_pda,
    derive_nonce_bitmap_pda, derive_nullifier_pda, derive_pa_state_pda, derive_root_marker_pda,
    derive_tx_data_pda, derive_verifier_router_pdas,
};
pub use program_ids::*;
