import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

import { keccak_256 } from "@noble/hashes/sha3";

import { toHex } from "./codecs.js";
import { anchorEventDisc } from "./discriminator.js";
import {
  decodeEventInstruction,
  decodeForwarderEventInstruction,
  EVENT_IX_TAG,
  EventDecodeError,
  type ForwarderEvent,
} from "./events.js";

const fixture = JSON.parse(
  readFileSync(fileURLToPath(new URL("../../fixtures/events_fixture.json", import.meta.url)), "utf8"),
) as {
  events: { source: string; name: string; data_b64: string; expected: Record<string, unknown> }[];
};

const b64 = (s: string): Uint8Array => Uint8Array.from(Buffer.from(s, "base64"));
const hex = (b: Uint8Array): string => toHex(b).slice(2);

const idlEvents = (
  JSON.parse(
    readFileSync(fileURLToPath(new URL("../../idl/protocol_adapter.json", import.meta.url)), "utf8"),
  ) as { events: { name: string }[] }
).events.map((e) => e.name);

describe("decodeEventInstruction (cross-package fixture)", () => {
  it("decodes every fixture entry to the recorded values", () => {
    const seen = new Set<string>();
    for (const [i, e] of fixture.events.entries()) {
      const entry = `#${i} ${e.name} (${e.source})`;
      const ev = decodeEventInstruction(b64(e.data_b64));
      expect(ev.name, entry).toBe(e.name);
      seen.add(ev.name);
      const exp = e.expected;
      switch (ev.name) {
        case "ResourcePayloadEvent":
        case "DiscoveryPayloadEvent":
        case "ExternalPayloadEvent":
        case "ApplicationPayloadEvent":
          expect(hex(ev.tag), entry).toBe(exp.tag);
          expect(ev.index, entry).toBe(exp.index);
          expect(hex(ev.blob), entry).toBe(exp.blob);
          break;
        case "ActionExecutedEvent":
          expect(hex(ev.actionTreeRoot), entry).toBe(exp.action_tree_root);
          expect(ev.nullifiers.map(hex), entry).toEqual(exp.nullifiers);
          expect(ev.consumedLogicRefs.map(hex), entry).toEqual(exp.consumed_logic_refs);
          expect(ev.commitments.map(hex), entry).toEqual(exp.commitments);
          expect(ev.createdLogicRefs.map(hex), entry).toEqual(exp.created_logic_refs);
          break;
        case "TransactionExecutedEvent":
          expect(hex(ev.transactionId), entry).toBe(exp.transaction_id);
          break;
        case "ForwarderCallExecutedEvent":
          expect(hex(ev.forwarder), entry).toBe(exp.forwarder);
          expect(hex(ev.input), entry).toBe(exp.input);
          expect(hex(ev.output), entry).toBe(exp.output);
          break;
        case "CommitmentTreeRootAddedEvent":
          expect(hex(ev.root), entry).toBe(exp.root);
          break;
        case "KindTableCommitmentUpdatedEvent":
          expect(hex(ev.kindTableCommitment), entry).toBe(exp.kind_table_commitment);
          break;
        case "LogicRefDeniedEvent":
          expect(hex(ev.logicRef), entry).toBe(exp.logic_ref);
          break;
        case "PausedEvent":
        case "UnpausedEvent":
          expect(hex(ev.account), entry).toBe(exp.account);
          break;
      }
    }
    expect([...seen].sort()).toEqual([...idlEvents].sort());
  });

  // pa-evm's transaction id: Keccak-256 of the transaction's action tree
  // roots, concatenated in order. Each recorded settlement's
  // TransactionExecuted must carry the id of its own ActionExecuted roots.
  it("carries the Keccak-256 of the settlement's action tree roots as its transaction id", () => {
    const settlements = new Map<string, { roots: Uint8Array[]; id?: Uint8Array }>();
    for (const e of fixture.events) {
      const ev = decodeEventInstruction(b64(e.data_b64));
      const s = settlements.get(e.source) ?? { roots: [] };
      if (ev.name === "ActionExecutedEvent") s.roots.push(ev.actionTreeRoot);
      if (ev.name === "TransactionExecutedEvent") s.id = ev.transactionId;
      settlements.set(e.source, s);
    }
    const checked = [...settlements.values()].filter((s) => s.id);
    expect(checked.length).toBeGreaterThan(0);
    for (const s of checked) {
      expect(hex(s.id!)).toBe(hex(keccak_256(Buffer.concat(s.roots))));
    }
  });

  it("rejects instruction data without the event tag", () => {
    const data = b64(fixture.events[0]!.data_b64).slice(8);
    expect(() => decodeEventInstruction(data)).toThrow(EventDecodeError);
    expect(() => decodeEventInstruction(data)).toThrow(/event tag/);
  });

  it("rejects trailing bytes", () => {
    const data = b64(fixture.events[2]!.data_b64);
    const longer = new Uint8Array(data.length + 1);
    longer.set(data);
    expect(() => decodeEventInstruction(longer)).toThrow(/1 trailing byte/);
  });

  it("rejects an unknown discriminator", () => {
    const data = b64(fixture.events[2]!.data_b64);
    data[8] ^= 0xff;
    expect(() => decodeEventInstruction(data)).toThrow(/unknown event discriminator/);
  });
});

const MINT = new Uint8Array(32).fill(1);
const USER = new Uint8Array(32).fill(2);
const OTHER = new Uint8Array(32).fill(3);
const ROOT = new Uint8Array(32).fill(4);

const u64Le = (v: bigint): Uint8Array => {
  const b = new Uint8Array(8);
  new DataView(b.buffer).setBigUint64(0, v, true);
  return b;
};

/** Event tag, discriminator of `name`, then the body. */
const eventIx = (name: string, body: Uint8Array[]): Uint8Array =>
  Uint8Array.from(Buffer.concat([EVENT_IX_TAG, anchorEventDisc(name), ...body]));

/** Every forwarder event with its hand-built instruction data and the value it must decode to. */
const forwarderCases: [ForwarderEvent["name"], Uint8Array, ForwarderEvent][] = [
  [
    "Wrapped",
    eventIx("Wrapped", [MINT, USER, u64Le(1_000_000n), u64Le(77n), ROOT]),
    { name: "Wrapped", tokenMint: MINT, from: USER, amount: 1_000_000n, nonce: 77n, actionTreeRoot: ROOT },
  ],
  [
    "Unwrapped",
    eventIx("Unwrapped", [MINT, USER, u64Le(2n ** 64n - 1n)]),
    { name: "Unwrapped", tokenMint: MINT, to: USER, amount: 2n ** 64n - 1n },
  ],
  [
    "EmergencyCallerSet",
    eventIx("EmergencyCallerSet", [USER, OTHER]),
    { name: "EmergencyCallerSet", emergencyCaller: USER, setBy: OTHER },
  ],
  [
    "EmergencyWithdraw",
    eventIx("EmergencyWithdraw", [MINT, USER, u64Le(42n), OTHER]),
    { name: "EmergencyWithdraw", tokenMint: MINT, to: USER, amount: 42n, caller: OTHER },
  ],
  ["Initialized", eventIx("Initialized", [u64Le(2n)]), { name: "Initialized", version: 2n }],
];

const forwarderIdlEvents = (
  JSON.parse(
    readFileSync(fileURLToPath(new URL("../../idl/spl_token_forwarder.json", import.meta.url)), "utf8"),
  ) as { events: { name: string; discriminator: number[] }[] }
).events;

describe("decodeForwarderEventInstruction", () => {
  it("decodes every forwarder event from hand-built bytes", () => {
    for (const [name, data, expected] of forwarderCases) {
      expect(decodeForwarderEventInstruction(data), name).toEqual(expected);
    }
  });

  it("covers every IDL event with the IDL discriminator", () => {
    expect(forwarderCases.map(([name]) => name).sort()).toEqual(forwarderIdlEvents.map((e) => e.name).sort());
    for (const e of forwarderIdlEvents) {
      expect(Array.from(anchorEventDisc(e.name)), e.name).toEqual(e.discriminator);
    }
  });

  it("rejects instruction data without the event tag", () => {
    const data = forwarderCases[0]![1].slice(8);
    expect(() => decodeForwarderEventInstruction(data)).toThrow(EventDecodeError);
    expect(() => decodeForwarderEventInstruction(data)).toThrow(/does not start with the Anchor event tag/);
  });

  it("rejects an unknown discriminator", () => {
    // A PA event is not a forwarder event.
    const data = eventIx("TransactionExecutedEvent", [ROOT]);
    expect(() => decodeForwarderEventInstruction(data)).toThrow(
      `unknown event discriminator ${hex(anchorEventDisc("TransactionExecutedEvent"))}`,
    );
  });

  it("rejects a truncated body", () => {
    for (const [name, data] of forwarderCases) {
      expect(() => decodeForwarderEventInstruction(data.slice(0, -1)), name).toThrow(EventDecodeError);
      expect(() => decodeForwarderEventInstruction(data.slice(0, -1)), name).toThrow(/event truncated while reading/);
    }
    expect(() => decodeForwarderEventInstruction(forwarderCases[0]![1].slice(0, -1))).toThrow(
      "event truncated while reading action_tree_root",
    );
  });

  it("rejects trailing bytes", () => {
    for (const [name, data] of forwarderCases) {
      const longer = new Uint8Array(data.length + 1);
      longer.set(data);
      expect(() => decodeForwarderEventInstruction(longer), name).toThrow("1 trailing byte(s) after the event body");
    }
  });
});
