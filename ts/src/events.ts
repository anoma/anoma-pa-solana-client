// Anchor's framing of CPI events.
//
// The PA emits every event as a self-invocation (Anchor `#[event_cpi]`): an
// inner instruction whose program is the PA and whose data is the 8-byte event
// tag, the event's discriminator, and its Borsh body. Readers take a
// settlement transaction's inner instructions and pass each PA-addressed one
// through `decodeCpiEvent`, which reads the event with the IDL's coder.
// Events never appear in the program log.

import type { IdlType } from "@anchor-lang/core";

import { toHex } from "./codecs.js";
import { coder } from "./coder.js";
import { idl } from "./idl.js";

/** `anchor_lang::event::EVENT_IX_TAG_LE`: the u64 `0x1d9acb512ea545e4` little-endian. */
export const EVENT_IX_TAG = new Uint8Array([0xe4, 0x45, 0xa5, 0x2e, 0x51, 0xcb, 0x9a, 0x1d]);

export class EventDecodeError extends Error {
  constructor(message: string, options?: ErrorOptions) {
    super(message, options);
    this.name = "EventDecodeError";
  }
}

/** The byte length of each fixed-size Borsh type. */
const FIXED_LENGTHS: Record<string, number> = {
  bool: 1,
  u8: 1,
  i8: 1,
  u16: 2,
  i16: 2,
  u32: 4,
  i32: 4,
  f32: 4,
  u64: 8,
  i64: 8,
  f64: 8,
  u128: 16,
  i128: 16,
  u256: 32,
  i256: 32,
  pubkey: 32,
};

/**
 * The length of the Borsh encoding of `value`, as the coder decoded it for the
 * IDL type `type`: the bytes the coder read. The coder neither reports it nor
 * refuses bytes after a body.
 */
function borshLength(value: any, type: IdlType): number {
  if (typeof type === "string") {
    if (type === "bytes") return 4 + value.length;
    const fixed = FIXED_LENGTHS[type];
    if (fixed !== undefined) return fixed;
  } else if ("array" in type) {
    return value.reduce((n: number, v: unknown) => n + borshLength(v, type.array[0]), 0);
  } else if ("vec" in type) {
    return value.reduce((n: number, v: unknown) => n + borshLength(v, type.vec), 4);
  } else if ("defined" in type) {
    const def = idl.types?.find((t) => t.name === type.defined.name)?.type;
    if (def?.kind === "type") return borshLength(value, def.alias);
    if (def?.kind === "struct" && def.fields !== undefined) {
      return def.fields.reduce((n: number, f) => {
        if (typeof f !== "object" || !("name" in f)) throw new EventDecodeError(`${type.defined.name} has unnamed fields`);
        return n + borshLength(value[f.name], f.type);
      }, 0);
    }
  }
  throw new EventDecodeError(`no Borsh length for the IDL type ${JSON.stringify(type)}`);
}

const base64 = (bytes: Uint8Array): string => btoa(Array.from(bytes, (b) => String.fromCharCode(b)).join(""));

/**
 * The event in `data`, the instruction data of one PA event self-invocation:
 * its IDL name and its fields under their IDL names, as the coder decodes them.
 * Refuses data without the event tag, a discriminator no IDL event has, a body
 * that does not decode, and bytes after the body.
 */
export function decodeCpiEvent(data: Uint8Array): { name: string; data: unknown } {
  const tag = data.subarray(0, EVENT_IX_TAG.length);
  if (tag.length !== EVENT_IX_TAG.length || !tag.every((b, i) => b === EVENT_IX_TAG[i])) {
    throw new EventDecodeError("the instruction data does not start with the event tag");
  }
  const rest = data.subarray(EVENT_IX_TAG.length);
  let event: { name: string; data: unknown } | null;
  try {
    event = coder.events.decode(base64(rest));
  } catch (e) {
    throw new EventDecodeError(`the event body does not decode: ${e}`, { cause: e });
  }
  if (event === null) {
    throw new EventDecodeError(`no IDL event has the discriminator ${toHex(rest.subarray(0, 8)).slice(2)}`);
  }
  const name = event.name;
  const discriminator = idl.events!.find((e) => e.name === name)!.discriminator;
  const body = rest.length - discriminator.length;
  const read = borshLength(event.data, { defined: { name } });
  if (read < body) throw new EventDecodeError(`${body - read} trailing byte(s) after the ${name} body`);
  if (read > body) throw new EventDecodeError(`the ${name} body ends ${read - body} byte(s) short`);
  return event;
}
