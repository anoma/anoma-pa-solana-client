// Decoders for the PA's events.
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
 * payload list for that resource, a `u256` (pa-evm's `uint256`); `blob` is
 * the payload bytes.
 */
export interface PayloadEvent {
  name: "ResourcePayloadEvent" | "DiscoveryPayloadEvent" | "ExternalPayloadEvent" | "ApplicationPayloadEvent";
  tag: Uint8Array;
  index: bigint;
  blob: Uint8Array;
}

/**
 * Emitted once per action, after that action's resources are processed, as
 * pa-evm's `ActionExecuted`: the nullifiers of its consumed resources and the
 * commitments of its created ones, each with its resource's logic ref, in
 * instance order.
 */
export interface ActionExecutedEvent {
  name: "ActionExecutedEvent";
  actionTreeRoot: Uint8Array;
  nullifiers: Uint8Array[];
  consumedLogicRefs: Uint8Array[];
  commitments: Uint8Array[];
  createdLogicRefs: Uint8Array[];
}

/**
 * Emitted once per settlement, last, as pa-evm's `TransactionExecuted`: the
 * transaction id is the Keccak-256 hash of the concatenated action tree roots.
 */
export interface TransactionExecutedEvent {
  name: "TransactionExecutedEvent";
  transactionId: Uint8Array;
}

/**
 * A root the commitment tree took on: the empty tree's at initialization, then
 * the root of each settlement that appends commitments.
 */
export interface CommitmentTreeRootAddedEvent {
  name: "CommitmentTreeRootAddedEvent";
  root: Uint8Array;
}

/**
 * The kind-table commitment the adapter now requires: the empty table's at
 * initialization, then each `set_kind_table_commitment`.
 */
export interface KindTableCommitmentUpdatedEvent {
  name: "KindTableCommitmentUpdatedEvent";
  kindTableCommitment: Uint8Array;
}

/** A logic ref the owner added to the denylist for consumed resources (`consumed`) or to the one for created resources, for good. */
export interface LogicRefDeniedEvent {
  name: "LogicRefDeniedEvent";
  logicRef: Uint8Array;
  consumed: boolean;
}

/**
 * The owner (`account`) paused or unpaused settlement, as OpenZeppelin
 * Pausable's `Paused(account)` / `Unpaused(account)`.
 */
export interface PauseEvent {
  name: "PausedEvent" | "UnpausedEvent";
  account: Uint8Array;
}

/**
 * The ownership moved from `previousOwner` to `newOwner`, as OpenZeppelin
 * Ownable's `OwnershipTransferred`; the zero key stands for no owner (the
 * previous owner at `initialize`, the new owner once renounced).
 */
export interface OwnershipTransferredEvent {
  name: "OwnershipTransferredEvent";
  previousOwner: Uint8Array;
  newOwner: Uint8Array;
}

/**
 * The owner upgraded the program to the code whose executable hash is
 * `executableHash` (sha256 of the code without trailing zero bytes, what
 * `solana-verify get-program-hash` reports), as ERC1967's `Upgraded`.
 */
export interface UpgradedEvent {
  name: "UpgradedEvent";
  executableHash: Uint8Array;
}

/**
 * The adapter's state was initialized at schema version `version`: by
 * `initialize`, or by `migrate_state` bringing it to that version, as
 * OpenZeppelin Initializable's `Initialized(version)`.
 */
export interface InitializedEvent {
  name: "InitializedEvent";
  version: bigint;
}

/** Emitted once per external call, in call order. */
export interface ForwarderCallExecutedEvent {
  name: "ForwarderCallExecutedEvent";
  untrustedForwarder: Uint8Array;
  input: Uint8Array;
  output: Uint8Array;
}

export type PaEvent =
  | PayloadEvent
  | ActionExecutedEvent
  | TransactionExecutedEvent
  | ForwarderCallExecutedEvent
  | CommitmentTreeRootAddedEvent
  | KindTableCommitmentUpdatedEvent
  | LogicRefDeniedEvent
  | PauseEvent
  | OwnershipTransferredEvent
  | InitializedEvent
  | UpgradedEvent;

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

/** A Borsh `bool`: one byte, 0 or 1. */
function boolField(c: Cursor, field: string): boolean {
  const byte = c.u8(field);
  if (byte !== 0 && byte !== 1) throw new EventDecodeError(`invalid bool byte ${byte} for event field ${field}`);
  return byte === 1;
}

const bytesEqual = (a: Uint8Array, b: Uint8Array): boolean =>
  a.length === b.length && a.every((v, i) => v === b[i]);

/**
 * Decode the instruction data of one PA event self-invocation: check the tag,
 * read the discriminator, decode the body it names, and refuse trailing bytes.
 */
export function decodeEventInstruction(data: Uint8Array): PaEvent {
  try {
    const c = new Cursor(data);
    if (c.remaining() < EVENT_IX_TAG.length || !bytesEqual(c.take(EVENT_IX_TAG.length, "event tag"), EVENT_IX_TAG)) {
      throw new EventDecodeError("instruction data does not start with the Anchor event tag");
    }
    const disc = c.take(ANCHOR_DISCRIMINATOR_LEN, "event discriminator");
    const event = paEventBody(disc, c);
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

function paEventBody(disc: Uint8Array, c: Cursor): PaEvent {
  for (const name of PAYLOAD_EVENT_NAMES) {
    if (bytesEqual(disc, anchorEventDisc(name))) {
      return { name, tag: c.array32("tag"), index: c.u256Le("index"), blob: c.vecU8("blob") };
    }
  }
  if (bytesEqual(disc, anchorEventDisc("ActionExecutedEvent"))) {
    return {
      name: "ActionExecutedEvent",
      actionTreeRoot: c.array32("action_tree_root"),
      nullifiers: c.vecArray32("nullifiers"),
      consumedLogicRefs: c.vecArray32("consumed_logic_refs"),
      commitments: c.vecArray32("commitments"),
      createdLogicRefs: c.vecArray32("created_logic_refs"),
    };
  }
  if (bytesEqual(disc, anchorEventDisc("TransactionExecutedEvent"))) {
    return { name: "TransactionExecutedEvent", transactionId: c.array32("transaction_id") };
  }
  if (bytesEqual(disc, anchorEventDisc("CommitmentTreeRootAddedEvent"))) {
    return { name: "CommitmentTreeRootAddedEvent", root: c.array32("root") };
  }
  if (bytesEqual(disc, anchorEventDisc("KindTableCommitmentUpdatedEvent"))) {
    return { name: "KindTableCommitmentUpdatedEvent", kindTableCommitment: c.array32("kind_table_commitment") };
  }
  if (bytesEqual(disc, anchorEventDisc("LogicRefDeniedEvent"))) {
    return { name: "LogicRefDeniedEvent", logicRef: c.array32("logic_ref"), consumed: boolField(c, "consumed") };
  }
  for (const name of ["PausedEvent", "UnpausedEvent"] as const) {
    if (bytesEqual(disc, anchorEventDisc(name))) {
      return { name, account: c.array32("account") };
    }
  }
  if (bytesEqual(disc, anchorEventDisc("OwnershipTransferredEvent"))) {
    return { name: "OwnershipTransferredEvent", previousOwner: c.array32("previous_owner"), newOwner: c.array32("new_owner") };
  }
  if (bytesEqual(disc, anchorEventDisc("InitializedEvent"))) {
    return { name: "InitializedEvent", version: c.u64Le("version") };
  }
  if (bytesEqual(disc, anchorEventDisc("UpgradedEvent"))) {
    return { name: "UpgradedEvent", executableHash: c.array32("executable_hash") };
  }
  if (bytesEqual(disc, anchorEventDisc("ForwarderCallExecutedEvent"))) {
    return {
      name: "ForwarderCallExecutedEvent",
      untrustedForwarder: c.array32("untrusted_forwarder"),
      input: c.vecU8("input"),
      output: c.vecU8("output"),
    };
  }
  throw new EventDecodeError(`unknown event discriminator ${toHex(disc).slice(2)}`);
}
