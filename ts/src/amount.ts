// SPL token amount validation.
//
// SPL Token amounts are u64. JavaScript bigint can silently exceed u64::MAX and
// DataView.setBigUint64 wraps. Validate at every API boundary that takes an amount.

/** Maximum SPL token amount (2^64 - 1). */
export const MAX_SPL_TOKEN_AMOUNT: bigint = (1n << 64n) - 1n;

/** Throw RangeError if `amount` is not a valid u64. */
export function assertSplTokenAmount(amount: bigint, label = "SPL token amount"): void {
  if (amount < 0n || amount > MAX_SPL_TOKEN_AMOUNT) {
    throw new RangeError(
      `${label} must be between 0 and ${MAX_SPL_TOKEN_AMOUNT.toString()}`,
    );
  }
}
