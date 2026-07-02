//! Client bindings for the Solana Anoma Protocol Adapter and SPL Token Forwarder.
//!
//! See `REQUIREMENTS.md` at the repository root for the full surface specification.
//! This crate is the canonical home for instruction builders, account decoders, PDA
//! derivation, wire-format types, and PA-owned constants. Integrators should depend
//! on this crate rather than re-implementing PA-binding logic inline.

pub mod accounts;
pub mod constants;
pub mod discriminator;
pub mod external_call;
pub mod merkle;
pub mod wallet_keyring;
pub mod wrap_message;

#[cfg(feature = "solana")]
pub mod ata;
#[cfg(feature = "solana")]
pub mod forwarder;
#[cfg(feature = "solana")]
pub mod instructions;
#[cfg(feature = "solana")]
pub mod pda;
#[cfg(feature = "solana")]
pub mod program_ids;

pub use accounts::{decode_pa_state, DecodeError, PAStateAccount};
pub use constants::*;
pub use discriminator::{anchor_event_disc, anchor_instruction_disc};
pub use external_call::{
    encode_migrate_forwarder_input, encode_unwrap_forwarder_input, encode_wrap_forwarder_input,
    OutputMode, SolanaExternalCall, OP_MIGRATE, OP_UNWRAP, OP_WRAP,
};
pub use merkle::{hash_two, zero_hashes, CommitmentTreeState, MerkleError, PADDING_LEAF};

#[cfg(feature = "solana")]
pub use ata::create_ata_idempotent_ix;
#[cfg(feature = "solana")]
pub use forwarder::{build_unwrap_forwarder_accounts, build_wrap_forwarder_accounts};
#[cfg(feature = "solana")]
pub use instructions::{settle_from_txdata_ix, txdata_close_ix, txdata_init_ix, txdata_write_ix};
#[cfg(feature = "solana")]
pub use pda::{
    derive_associated_token_address, derive_forwarder_config_pda, derive_forwarder_escrow_pda,
    derive_nonce_bitmap_pda, derive_nullifier_pda, derive_pa_state_pda, derive_root_marker_pda,
    derive_tx_data_pda, derive_verifier_router_pdas,
};
#[cfg(feature = "solana")]
pub use program_ids::*;
pub use wallet_keyring::{
    derive_keyring_secrets, nullifier_commitment, sign_message, KeyringSecrets, AUTHORITY_DOMAIN,
    DISCOVERY_DOMAIN, ENCRYPTION_DOMAIN, KEYRING_SALT, NULLIFIER_DOMAIN,
};
pub use wrap_message::{sha256, WrapMessage, WRAP_MESSAGE_LEN};
