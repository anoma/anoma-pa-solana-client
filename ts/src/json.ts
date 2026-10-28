// JSON text of the values kit's RPC returns, whose integers are bigints
// (`JSON.stringify` refuses a bigint): each bigint as its decimal digits.

export const toJson = (value: unknown): string =>
  JSON.stringify(value, (_, v: unknown) => (typeof v === "bigint" ? v.toString() : v));
