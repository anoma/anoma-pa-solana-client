/**
 * The adapter's errors, as a failed transaction reports them: Anchor's custom
 * program error code, `6000` plus the error's position in the program's
 * `PAError`.
 */

/** The first code Anchor gives a program's own errors. */
export const ANCHOR_ERROR_CODE_OFFSET = 6000;

/** Every error the adapter returns, in code order. */
export const PA_ERRORS = [
  "PreExistingNullifier",
  "NullifierPdaMismatch",
  "NonExistingRoot",
  "RootPdaMismatch",
  "TxDataExpired",
  "TxDataBoundsExceeded",
  "TxDataExpiryTooSoon",
  "TxDataExpiryTooLate",
  "TxDataExtendMustIncrease",
  "TxDataNotExpired",
  "InvalidExpiryConfig",
  "InvalidTransactionData",
  "EmptyTransactionNotAllowed",
  "InvalidProof",
  "VerifierRouterFailed",
  "AggregationRequired",
  "RiscZeroVerifierSelectorMismatch",
  "InvalidExternalCallBlob",
  "UnregisteredForwarder",
  "ForwarderCallOutputMismatch",
  "ExternalCallCpiFailed",
  "DeltaProofVerificationFailed",
  "DeltaMismatch",
  "InvalidDeltaProof",
  "PointNotOnCurve",
  "ExpectedDeltaProof",
  "ComplianceKeyMismatch",
  "UnacceptedKindTableCommitment",
  "ZeroKindTableCommitmentNotAllowed",
  "Unauthorized",
  "EnforcedPause",
  "ExpectedPause",
  "RiscZeroVerifierPaused",
  "InvalidVerifierEntry",
  "TreeMaxDepthReached",
  "InvalidMarker",
  "MarkerUnexpectedOwner",
  "MarkerUnexpectedData",
  "PreExistingRoot",
  "UnsupportedStateSchema",
  "NotPreviousSchema",
  "ZeroLogicRefNotAllowed",
  "LogicRefAlreadyDenied",
  "ResourceWithDeniedLogicRef",
  "ZeroRiscZeroVerifierRouterNotAllowed",
  "ZeroRiscZeroVerifierSelectorNotAllowed",
  "OwnableUnauthorizedAccount",
  "OwnableInvalidOwner",
  "InvalidUpgradeBuffer",
] as const;

/** An error the adapter returns. */
export type PaError = (typeof PA_ERRORS)[number];

/** The error the adapter returns as custom program error `code`, if any. */
export function paErrorFromCode(code: number): PaError | undefined {
  return Number.isInteger(code) && code >= ANCHOR_ERROR_CODE_OFFSET
    ? PA_ERRORS[code - ANCHOR_ERROR_CODE_OFFSET]
    : undefined;
}

/** The custom program error code the adapter returns for `error`. */
export function paErrorCode(error: PaError): number {
  return ANCHOR_ERROR_CODE_OFFSET + PA_ERRORS.indexOf(error);
}
