//! Wire-level types and helpers for the PA's external-call subsystem.
//!
//! The PA carries an opaque `Vec<u8>` per external call in the ARM transaction's
//! external_payload section. Off-chain we serialize/deserialize via `bincode`;
//! on-chain the PA reads the same shape. PA and integrators must agree byte-for-byte.

use serde::{Deserialize, Serialize};

/// Solana-specific external call structure.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SolanaExternalCall {
    /// Forwarder program ID (32 bytes).
    pub program_id: [u8; 32],
    /// The forwarder's instruction data.
    pub instruction_data: Vec<u8>,
    /// Expected return-data bytes (committed in the proof).
    pub expected_output: Vec<u8>,
    /// How the PA reads the forwarder's output.
    pub output_mode: OutputMode,
    /// Number of accounts in this call's CPI segment (including the forwarder
    /// program account at position 0). Committed in the proof so segment
    /// boundaries are unambiguous.
    pub num_accounts: u8,
}

/// How the PA reads an external call's output.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum OutputMode {
    /// Read output from Solana `return_data`.
    ReturnData,
}

impl SolanaExternalCall {
    /// Bincode-serialize this call. Used to embed in the ARM transaction's
    /// external_payload section.
    pub fn encode(&self) -> Vec<u8> {
        bincode::serialize(self).expect("SolanaExternalCall serialization should not fail")
    }

    /// Bincode-deserialize from the ARM transaction's external_payload blob.
    pub fn decode(bytes: &[u8]) -> Result<Self, Box<bincode::ErrorKind>> {
        bincode::deserialize(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_call_roundtrips_via_bincode() {
        let call = SolanaExternalCall {
            program_id: [7u8; 32],
            instruction_data: vec![1, 2, 3, 4, 5],
            expected_output: vec![1],
            output_mode: OutputMode::ReturnData,
            num_accounts: 12,
        };
        let bytes = call.encode();
        let back = SolanaExternalCall::decode(&bytes).expect("decode");
        assert_eq!(call, back);
    }
}
