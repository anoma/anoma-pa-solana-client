import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

import { plain } from "../fixture-form.mjs";
import { decodeCpiEvent, EVENT_IX_TAG, EventDecodeError } from "./events.js";
import { idl } from "./idl.js";

const fixture = JSON.parse(
  readFileSync(fileURLToPath(new URL("../../fixtures/events_fixture.json", import.meta.url)), "utf8"),
) as {
  event_ix_tag_hex: string;
  events: { source: string; name: string; data_b64: string; expected: Record<string, unknown> }[];
};

const b64 = (s: string): Uint8Array => Uint8Array.from(Buffer.from(s, "base64"));

const typeDef = (name: string) => {
  const def = idl.types!.find((t) => t.name === name);
  if (def === undefined) throw new Error(`the IDL declares no type ${name}`);
  return def.type;
};

const entryData = (name: string): Uint8Array => {
  const entry = fixture.events.find((e) => e.name === name);
  if (entry === undefined) throw new Error(`the fixture has no ${name}`);
  return b64(entry.data_b64);
};

describe("decodeCpiEvent (cross-package fixture)", () => {
  it("strips Anchor's event tag, the fixture's", () => {
    expect(Buffer.from(EVENT_IX_TAG).toString("hex")).toBe(fixture.event_ix_tag_hex);
  });

  it("decodes every fixture entry to the recorded values, covering every IDL event", () => {
    const seen = new Set<string>();
    for (const [i, e] of fixture.events.entries()) {
      const entry = `#${i} ${e.name} (${e.source})`;
      const event = decodeCpiEvent(b64(e.data_b64));
      console.log(entry, event);
      expect(event.name, entry).toBe(e.name);
      const def = typeDef(event.name);
      if (def.kind !== "struct" || def.fields === undefined) throw new Error(`${entry}: no struct`);
      const decoded = Object.fromEntries(
        def.fields.map((f) => {
          if (typeof f !== "object" || !("name" in f)) throw new Error(`${entry}: tuple field`);
          return [f.name, plain(idl, (event.data as Record<string, unknown>)[f.name], f.type)];
        }),
      );
      expect(decoded, entry).toEqual(e.expected);
      seen.add(event.name);
    }
    expect([...seen].sort()).toEqual(idl.events!.map((e) => e.name).sort());
  });

  it("decodes a body larger than 1000 bytes", () => {
    const data = entryData("ForwarderCallExecuted");
    const event = decodeCpiEvent(data);
    const blob = Buffer.alloc(3000, 0xab);
    // The body is untrusted_forwarder (32), then input and output, each a u32
    // length and its bytes: replace the output with 3000 bytes.
    const inputLength = Buffer.from(data).readUInt32LE(16 + 32);
    const head = data.subarray(0, 16 + 32 + 4 + inputLength);
    const length = Buffer.alloc(4);
    length.writeUInt32LE(blob.length);
    const large = Buffer.concat([head, length, blob]);
    const decoded = decodeCpiEvent(large);
    expect(decoded.name).toBe(event.name);
    expect(Buffer.from((decoded.data as { output: Uint8Array }).output)).toEqual(blob);
  });

  it("rejects instruction data without the event tag", () => {
    const data = entryData("TransactionExecuted");
    for (const bad of [data.subarray(8), data.subarray(0, 4), new Uint8Array()]) {
      expect(() => decodeCpiEvent(bad)).toThrow(EventDecodeError);
      expect(() => decodeCpiEvent(bad)).toThrow(/does not start with the event tag/);
    }
  });

  it("rejects trailing bytes", () => {
    for (const e of fixture.events) {
      const data = b64(e.data_b64);
      const longer = new Uint8Array(data.length + 1);
      longer.set(data);
      expect(() => decodeCpiEvent(longer), e.name).toThrow(EventDecodeError);
      expect(() => decodeCpiEvent(longer), e.name).toThrow(/1 trailing byte/);
    }
  });

  it("rejects a truncated body", () => {
    for (const e of fixture.events) {
      const data = b64(e.data_b64).subarray(0, -1);
      expect(() => decodeCpiEvent(data), e.name).toThrow(EventDecodeError);
    }
  });

  it("rejects an unknown discriminator", () => {
    const data = Uint8Array.from(entryData("TransactionExecuted"));
    data[8]! ^= 0xff;
    expect(() => decodeCpiEvent(data)).toThrow(EventDecodeError);
    expect(() => decodeCpiEvent(data)).toThrow(/no IDL event has the discriminator/);
  });

  it("rejects an invalid bool", () => {
    const data = Uint8Array.from(entryData("LogicRefDenied"));
    data[data.length - 1] = 2;
    expect(() => decodeCpiEvent(data)).toThrow(EventDecodeError);
  });
});
