//! The adapter's errors, as a failed transaction reports them: Anchor's custom
//! program error code, `6000` plus the error's position in the program's
//! `PAError`.

/// The first code Anchor gives a program's own errors.
pub const ANCHOR_ERROR_CODE_OFFSET: u32 = 6000;

/// An error the adapter returns, by its code.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum PaError {
    /// Duplicate nullifier detected.
    PreExistingNullifier = ANCHOR_ERROR_CODE_OFFSET,
    /// Nullifier PDA pubkey mismatch.
    NullifierPdaMismatch,
    /// Commitment tree root does not exist in historical set.
    NonExistingRoot,
    /// Root marker is missing for, does not match, or is passed without a root this settlement produces.
    RootPdaMismatch,
    /// TxData has expired.
    TxDataExpired,
    /// TxData write exceeds payload capacity.
    TxDataBoundsExceeded,
    /// TxData expires_slot is below minimum (too soon).
    TxDataExpiryTooSoon,
    /// TxData expires_slot exceeds maximum (too far in future).
    TxDataExpiryTooLate,
    /// TxData extension must increase expires_slot.
    TxDataExtendMustIncrease,
    /// TxData has not expired yet (permissionless close requires expiration).
    TxDataNotExpired,
    /// Invalid expiry configuration (min must be < max, within reasonable bounds).
    InvalidExpiryConfig,
    /// Invalid transaction data.
    InvalidTransactionData,
    /// The transaction has no actions.
    EmptyTransactionNotAllowed,
    /// Invalid proof.
    InvalidProof,
    /// Verifier router call failed (verifier may be estopped).
    VerifierRouterFailed,
    /// Aggregation required: non-aggregated proofs not enabled.
    AggregationRequired,
    /// Proof selector does not match expected selector.
    RiscZeroVerifierSelectorMismatch,
    /// Invalid external call blob encoding.
    InvalidExternalCallBlob,
    /// Forwarder account does not match the program the external call names.
    UnregisteredForwarder,
    /// External call output verification failed.
    ForwarderCallOutputMismatch,
    /// External call CPI failed.
    ExternalCallCpiFailed,
    /// Delta proof verification failed.
    DeltaProofVerificationFailed,
    /// Delta mismatch: transaction is not balanced.
    DeltaMismatch,
    /// Invalid delta proof format.
    InvalidDeltaProof,
    /// Delta point not on secp256k1 curve.
    PointNotOnCurve,
    /// Expected delta proof, got witness.
    ExpectedDeltaProof,
    /// Aggregation instance compliance key does not match the compliance circuit VK.
    ComplianceKeyMismatch,
    /// The transaction's kind-table commitment is neither the stored one nor the empty table's.
    UnacceptedKindTableCommitment,
    /// Zero kind-table commitment not allowed.
    ZeroKindTableCommitmentNotAllowed,
    /// Unauthorized: the signer does not hold the authority this instruction requires.
    Unauthorized,
    /// The protocol adapter is paused.
    EnforcedPause,
    /// The protocol adapter is not paused.
    ExpectedPause,
    /// The RISC Zero verifier for this deployment's selector is paused.
    RiscZeroVerifierPaused,
    /// Account is not the verifier router's entry for this deployment's selector.
    InvalidVerifierEntry,
    /// Tree has reached maximum depth (32 levels).
    TreeMaxDepthReached,
    /// Account is not owned by this program.
    InvalidMarker,
    /// Marker address is held by an unexpected owner.
    MarkerUnexpectedOwner,
    /// Marker address already contains data.
    MarkerUnexpectedData,
    /// The commitment tree root is already stored.
    PreExistingRoot,
    /// PAState schema version is not the one this program binary reads; migrate the account first.
    UnsupportedStateSchema,
    /// PAState is not a state account in the previous schema version.
    NotPreviousSchema,
    /// Zero logic ref not allowed.
    ZeroLogicRefNotAllowed,
    /// Logic ref is already on that denylist.
    LogicRefAlreadyDenied,
    /// A resource's logic ref is on the denylist for its side.
    ResourceWithDeniedLogicRef,
    /// Zero verifier router not allowed.
    ZeroRiscZeroVerifierRouterNotAllowed,
    /// Zero proof selector not allowed.
    ZeroRiscZeroVerifierSelectorNotAllowed,
    /// The signer is not the adapter's owner.
    OwnableUnauthorizedAccount,
    /// The zero key cannot be the owner.
    OwnableInvalidOwner,
    /// Buffer is not a loader buffer holding a program.
    InvalidUpgradeBuffer,
}

impl PaError {
    /// Every error, in code order.
    pub const ALL: [PaError; 49] = [
        PaError::PreExistingNullifier,
        PaError::NullifierPdaMismatch,
        PaError::NonExistingRoot,
        PaError::RootPdaMismatch,
        PaError::TxDataExpired,
        PaError::TxDataBoundsExceeded,
        PaError::TxDataExpiryTooSoon,
        PaError::TxDataExpiryTooLate,
        PaError::TxDataExtendMustIncrease,
        PaError::TxDataNotExpired,
        PaError::InvalidExpiryConfig,
        PaError::InvalidTransactionData,
        PaError::EmptyTransactionNotAllowed,
        PaError::InvalidProof,
        PaError::VerifierRouterFailed,
        PaError::AggregationRequired,
        PaError::RiscZeroVerifierSelectorMismatch,
        PaError::InvalidExternalCallBlob,
        PaError::UnregisteredForwarder,
        PaError::ForwarderCallOutputMismatch,
        PaError::ExternalCallCpiFailed,
        PaError::DeltaProofVerificationFailed,
        PaError::DeltaMismatch,
        PaError::InvalidDeltaProof,
        PaError::PointNotOnCurve,
        PaError::ExpectedDeltaProof,
        PaError::ComplianceKeyMismatch,
        PaError::UnacceptedKindTableCommitment,
        PaError::ZeroKindTableCommitmentNotAllowed,
        PaError::Unauthorized,
        PaError::EnforcedPause,
        PaError::ExpectedPause,
        PaError::RiscZeroVerifierPaused,
        PaError::InvalidVerifierEntry,
        PaError::TreeMaxDepthReached,
        PaError::InvalidMarker,
        PaError::MarkerUnexpectedOwner,
        PaError::MarkerUnexpectedData,
        PaError::PreExistingRoot,
        PaError::UnsupportedStateSchema,
        PaError::NotPreviousSchema,
        PaError::ZeroLogicRefNotAllowed,
        PaError::LogicRefAlreadyDenied,
        PaError::ResourceWithDeniedLogicRef,
        PaError::ZeroRiscZeroVerifierRouterNotAllowed,
        PaError::ZeroRiscZeroVerifierSelectorNotAllowed,
        PaError::OwnableUnauthorizedAccount,
        PaError::OwnableInvalidOwner,
        PaError::InvalidUpgradeBuffer,
    ];

    /// The error the adapter returns as custom program error `code`, if any.
    pub fn from_code(code: u32) -> Option<PaError> {
        let index = code.checked_sub(ANCHOR_ERROR_CODE_OFFSET)?;
        Self::ALL.get(usize::try_from(index).ok()?).copied()
    }

    /// The custom program error code the adapter returns for this error.
    pub fn code(self) -> u32 {
        self as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_error_has_the_adapters_idl_name_and_code() {
        let idl: serde_json::Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../idl/protocol_adapter.json"
        )))
        .unwrap();
        let errors = idl["errors"].as_array().unwrap();
        assert_eq!(errors.len(), PaError::ALL.len(), "the IDL's error count");
        for error in errors {
            let code = u32::try_from(error["code"].as_u64().unwrap()).unwrap();
            let decoded = PaError::from_code(code).expect("an IDL code decodes");
            assert_eq!(
                format!("{decoded:?}"),
                error["name"].as_str().unwrap(),
                "{code}"
            );
            assert_eq!(decoded.code(), code);
        }
    }

    #[test]
    fn a_code_outside_the_adapters_errors_is_none() {
        assert_eq!(PaError::from_code(ANCHOR_ERROR_CODE_OFFSET - 1), None);
        let past = ANCHOR_ERROR_CODE_OFFSET + u32::try_from(PaError::ALL.len()).unwrap();
        assert_eq!(PaError::from_code(past), None);
        assert_eq!(PaError::from_code(0), None);
    }
}
