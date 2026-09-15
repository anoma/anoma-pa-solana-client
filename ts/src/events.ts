// Decoders for the PA's settlement events.
//
// The PA emits every event as a self-invocation (Anchor `#[event_cpi]`): an
// inner instruction whose program is the PA and whose data is the 8-byte event
// tag, the event's 8-byte discriminator (`sha256("event:<Name>")[..8]`), and
// the Borsh-encoded body. Readers take a settlement transaction's inner
// instructions and pass each PA-addressed one through `decodeEventInstruction`.
// Events never appear in the program log.

import { toHex } from "./codecs.js";
import { ANCHOR_DISCRIMINATOR_LEN } from "./constants.js";
import { Cursor, TruncatedError } from "./cursor.js";
import { anchorEventDisc } from "./discriminator.js";

/** `anchor_lang::event::EVENT_IX_TAG_LE`: the u64 `0x1d9acb512ea545e4` little-endian. */
export const EVENT_IX_TAG = new Uint8Array([0xe4, 0x45, 0xa5, 0x2e, 0x51, 0xcb, 0x9a, 0x1d]);

/**
 * Body shared by the four payload events. `tag` is the resource tag the
 * payload belongs to; `index` is the entry's position within its category's
 * payload list for that resource; `blob` is the payload bytes.
 */
export interface PayloadEvent {
  name: "ResourcePayloadEvent" | "DiscoveryPayloadEvent" | "ExternalPayloadEvent" | "ApplicationPayloadEvent";
  tag: Uint8Array;
  index: number;
  blob: Uint8Array;
}

/** Emitted once per action, after that action's payload events. */
export interface ActionExecutedEvent {
  name: "ActionExecutedEvent";
  actionTreeRoot: Uint8Array;
  actionTagCount: number;
}

/**
 * Emitted once per settlement, last. The three arrays are index-parallel;
 * `isConsumed[i]` is true when `tags[i]` is a nullifier and false when it is a
 * commitment. Tags are grouped per action (consumed, then created), so index
 * parity does not determine the role.
 */
export interface TransactionExecutedEvent {
  name: "TransactionExecutedEvent";
  tags: Uint8Array[];
  logicRefs: Uint8Array[];
  isConsumed: boolean[];
}

/** Emitted once per external call, in call order. */
export interface ForwarderCallExecutedEvent {
  name: "ForwarderCallExecutedEvent";
  forwarder: Uint8Array;
  input: Uint8Array;
  output: Uint8Array;
}

export type PaEvent =
  | PayloadEvent
  | ActionExecutedEvent
  | TransactionExecutedEvent
  | ForwarderCallExecutedEvent;

export class EventDecodeError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "EventDecodeError";
  }
}

const PAYLOAD_EVENT_NAMES: PayloadEvent["name"][] = [
  "ResourcePayloadEvent",
  "DiscoveryPayloadEvent",
  "ExternalPayloadEvent",
  "ApplicationPayloadEvent",
];

const bytesEqual = (a: Uint8Array, b: Uint8Array): boolean =>
  a.length === b.length && a.every((v, i) => v === b[i]);

/** Decode the instruction data of one PA event self-invocation. */
export function decodeEventInstruction(data: Uint8Array): PaEvent {
  try {
    const c = new Cursor(data);
    if (c.remaining() < EVENT_IX_TAG.length || !bytesEqual(c.take(EVENT_IX_TAG.length, "event tag"), EVENT_IX_TAG)) {
      throw new EventDecodeError("instruction data does not start with the Anchor event tag");
    }
    const disc = c.take(ANCHOR_DISCRIMINATOR_LEN, "event discriminator");
    const event = decodeBody(disc, c);
    const trailing = c.remaining();
    if (trailing !== 0) {
      throw new EventDecodeError(`${trailing} trailing byte(s) after the event body`);
    }
    return event;
  } catch (e) {
    if (e instanceof TruncatedError) {
      throw new EventDecodeError(`event ${e.message}`);
    }
    throw e;
  }
}

function decodeBody(disc: Uint8Array, c: Cursor): PaEvent {
  for (const name of PAYLOAD_EVENT_NAMES) {
    if (bytesEqual(disc, anchorEventDisc(name))) {
      return { name, tag: c.array32("tag"), index: c.u32Le("index"), blob: c.vecU8("blob") };
    }
  }
  if (bytesEqual(disc, anchorEventDisc("ActionExecutedEvent"))) {
    return {
      name: "ActionExecutedEvent",
      actionTreeRoot: c.array32("action_tree_root"),
      actionTagCount: c.u32Le("action_tag_count"),
    };
  }
  if (bytesEqual(disc, anchorEventDisc("TransactionExecutedEvent"))) {
    const tags = c.vecArray32("tags");
    const logicRefs = c.vecArray32("logic_refs");
    const len = c.u32Le("is_consumed");
    const isConsumed: boolean[] = [];
    for (let i = 0; i < len; i++) {
      const b = c.u8("is_consumed");
      if (b !== 0 && b !== 1) {
        throw new EventDecodeError(`invalid bool byte ${b}`);
      }
      isConsumed.push(b === 1);
    }
    return { name: "TransactionExecutedEvent", tags, logicRefs, isConsumed };
  }
  if (bytesEqual(disc, anchorEventDisc("ForwarderCallExecutedEvent"))) {
    return {
      name: "ForwarderCallExecutedEvent",
      forwarder: c.array32("forwarder"),
      input: c.vecU8("input"),
      output: c.vecU8("output"),
    };
  }
  throw new EventDecodeError(`unknown event discriminator ${toHex(disc).slice(2)}`);
}
