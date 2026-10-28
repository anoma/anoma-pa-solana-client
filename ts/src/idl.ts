// The adapter's IDL: `rust/idls/protocol_adapter.json`, the one the Rust crate
// is generated from, which the package's `copy-data` script copies into
// `src/data/` so the package ships the IDL it decodes with.

import type { Idl } from "@anchor-lang/core";

import idlJson from "./data/protocol_adapter.json" with { type: "json" };

/** The adapter's IDL. */
export const idl = idlJson as Idl;

/** The value of the IDL's constant `name`, which the IDL writes as JSON text. */
export function idlConstant(name: string): unknown {
  const constant = idl.constants?.find((c) => c.name === name);
  if (constant === undefined) throw new Error(`the adapter's IDL declares no constant ${name}`);
  return JSON.parse(constant.value);
}

/** The bytes of the IDL's PDA seed constant `name`. */
export function idlSeed(name: string): Uint8Array {
  return Uint8Array.from(idlConstant(name) as number[]);
}
