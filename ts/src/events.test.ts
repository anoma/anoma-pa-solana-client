import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

import { keccak_256 } from "@noble/hashes/sha3";

import { toHex } from "./codecs.js";
import { decodeEventInstruction, EventDecodeError } from "./events.js";

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
          expect(hex(ev.untrustedForwarder), entry).toBe(exp.untrusted_forwarder);
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
        case "OwnershipTransferredEvent":
          expect(hex(ev.previousOwner), entry).toBe(exp.previous_owner);
          expect(hex(ev.newOwner), entry).toBe(exp.new_owner);
          break;
        case "UpgradedEvent":
          expect(hex(ev.executableHash), entry).toBe(exp.executable_hash);
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

  it("rejects a truncated body", () => {
    for (const [i, e] of fixture.events.entries()) {
      const data = b64(e.data_b64).slice(0, -1);
      expect(() => decodeEventInstruction(data), `#${i} ${e.name}`).toThrow(EventDecodeError);
      expect(() => decodeEventInstruction(data), `#${i} ${e.name}`).toThrow(/event truncated while reading/);
    }
  });
});
