//! Solana program identifiers used by AnomaPay.
//!
//! Devnet defaults are baked in as `pub const` values. For mainnet or alternative
//! cluster deployments, integrators should override at the call site rather than
//! relying on these constants.

use solana_program::{pubkey, pubkey::Pubkey};

/// Anoma Protocol Adapter program ID (devnet/localnet default).
pub const PA_PROGRAM_ID: Pubkey = pubkey!("De5uxTic9Ed8dRW8TFDKDk6wWtCZa5BDCnLiVhLEoFyJ");

/// SPL Token Forwarder program ID (devnet/localnet default).
pub const FORWARDER_PROGRAM_ID: Pubkey = pubkey!("3cLKSYBijunpCc2F2gzizUkhYtyFrLr4RVdNiaK79b48");

/// Solana's native ed25519 signature-verification program. Used to carry verified
/// wrap-authorization signatures into the settle transaction.
pub const ED25519_PROGRAM_ID: Pubkey = pubkey!("Ed25519SigVerify111111111111111111111111111");

/// SPL Token program ID.
pub const SPL_TOKEN_PROGRAM_ID: Pubkey = pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");

/// Associated Token Account program ID.
pub const ASSOCIATED_TOKEN_PROGRAM_ID: Pubkey =
    pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");

/// Solana's `Instructions` sysvar (used by the forwarder to introspect the
/// ed25519-verify instruction at `ed25519_ix_index`).
pub const INSTRUCTIONS_SYSVAR_ID: Pubkey = pubkey!("Sysvar1nstructions1111111111111111111111111");

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn spl_token_program_id_is_canonical() {
        let expected = Pubkey::from_str("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA").unwrap();
        assert_eq!(SPL_TOKEN_PROGRAM_ID, expected);
    }
}
