// The form `fixtures/events_fixture.json` records decoded event values in,
// shared by its writer (`tools/capture-events-fixture.mjs`) and its checker
// (`src/events.test.ts`).

import { getAddressEncoder, getU256Encoder } from "@solana/kit";

/** `type` of `idl` with its aliases resolved to the type each stands for. */
export function resolve(idl, type) {
  if (!type.defined) return type;
  const def = idl.types.find((t) => t.name === type.defined.name)?.type;
  if (def?.kind !== "type") throw new Error(`no IDL type alias ${type.defined.name}`);
  return resolve(idl, def.alias);
}

/**
 * A value of `idl`'s type `type` as the coder decodes it, in the fixture's
 * form: keys and byte arrays as lowercase hex, u256s as their 32
 * little-endian bytes in hex, u64s as decimal strings, bools as themselves,
 * vectors element by element.
 */
export function plain(idl, value, type) {
  const t = resolve(idl, type);
  if (t === "bool") return value;
  if (t === "u64") return value.toString();
  if (t === "u256") return Buffer.from(getU256Encoder().encode(value)).toString("hex");
  if (t === "pubkey") return Buffer.from(getAddressEncoder().encode(value)).toString("hex");
  if (t === "bytes" || t.array) return Buffer.from(value).toString("hex");
  if (t.vec) return value.map((v) => plain(idl, v, t.vec));
  throw new Error(`no fixture form for the IDL type ${JSON.stringify(type)}`);
}
