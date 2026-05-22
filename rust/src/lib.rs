//! Client bindings for the Solana Anoma Protocol Adapter and SPL Token Forwarder.
//!
//! See `REQUIREMENTS.md` at the repository root for the full surface specification.
//! This crate is the canonical home for instruction builders, account decoders, PDA
//! derivation, wire-format types, and PA-owned constants. Integrators should depend
//! on this crate rather than re-implementing PA-binding logic inline.

pub mod constants;
pub mod discriminator;
pub mod program_ids;

pub use constants::*;
pub use discriminator::{anchor_event_disc, anchor_instruction_disc};
pub use program_ids::*;
