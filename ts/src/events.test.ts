import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

import { fromHex, toHex } from "./codecs.js";
import { decodeEventInstruction, EventDecodeError } from "./events.js";

const fixture = JSON.parse(
  readFileSync(fileURLToPath(new URL("../../fixtures/events_fixture.json", import.meta.url)), "utf8"),
) as {
  events: { source: string; name: string; data_b64: string; expected: Record<string, unknown> }[];
};

const b64 = (s: string): Uint8Array => Uint8Array.from(Buffer.from(s, "base64"));
const hex = (b: Uint8Array): string => toHex(b).slice(2);

describe("decodeEventInstruction (cross-package fixture)", () => {
  it("decodes every fixture entry to the recorded values", () => {
    expect(fixture.events).toHaveLength(13);
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
          expect(ev.actionTagCount, entry).toBe(exp.action_tag_count);
          break;
        case "TransactionExecutedEvent":
          expect(ev.tags.map(hex), entry).toEqual(exp.tags);
          expect(ev.logicRefs.map(hex), entry).toEqual(exp.logic_refs);
          expect(ev.isConsumed, entry).toEqual(exp.is_consumed);
          expect(ev.isConsumed.includes(true) && ev.isConsumed.includes(false), entry).toBe(true);
          break;
        case "ForwarderCallExecutedEvent":
          expect(hex(ev.forwarder), entry).toBe(exp.forwarder);
          expect(hex(ev.input), entry).toBe(exp.input);
          expect(hex(ev.output), entry).toBe(exp.output);
          break;
      }
    }
    expect(seen.size).toBe(7);
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
