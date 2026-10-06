// Regenerate fixtures/events_fixture.json, the cross-package check of the
// Rust and TS event decoders.
//
//   node tools/capture-events-fixture.mjs <rpc url> <signature>...
//   node tools/capture-events-fixture.mjs --recorded
//
// Each listed transaction's adapter event self-invocations (inner
// instructions to the adapter whose data starts with Anchor's event tag) are
// recorded as they ran on chain. `--recorded` takes the transactions' events
// the fixture already records instead, with their sources, and decodes them
// anew: for an IDL change that keeps the events' layout (a renamed field)
// once the transactions are no longer on chain. The expected values come from
// @anchor-lang/core's BorshCoder against idl/protocol_adapter.json, a decoder
// independent of this package's. Every event type in the IDL that none of
// the transactions emitted gets one entry encoded by that coder from fixed
// values, so the fixture covers the IDL's whole event set. Byte fields are
// recorded as lowercase hex, u256 fields as their 32 little-endian bytes in
// lowercase hex.
import { createRequire } from "node:module";
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const require = createRequire(new URL("../ts/package.json", import.meta.url));
const { BN, BorshCoder } = require("@anchor-lang/core");
const { Connection, PublicKey } = require("@solana/web3.js");
const bs58 = require("bs58").default;

const repo = (p) => fileURLToPath(new URL(`../${p}`, import.meta.url));
const idl = JSON.parse(readFileSync(repo("idl/protocol_adapter.json"), "utf8"));
const coder = new BorshCoder(idl);
const EVENT_IX_TAG = Buffer.from("e445a52e51cb9a1d", "hex");

const [url, ...signatures] = process.argv.slice(2);
if (!url || (url !== "--recorded" && signatures.length === 0)) {
  throw new Error("usage: capture-events-fixture.mjs <rpc url> <signature>... | --recorded");
}
const CODER_SOURCE = "anchor BorshCoder.types.encode";

// Whether an IDL type is a u256, directly or through a type alias.
function isU256(type) {
  return type === "u256" || (type.defined && idl.types.find((t) => t.name === type.defined.name).type.alias === "u256");
}

// A decoded value of an IDL type in the fixture's form: byte arrays, keys and
// u256s as hex, numbers as numbers, vectors element by element.
function plain(value, type) {
  if (isU256(type)) return value.toArrayLike(Buffer, "le", 32).toString("hex");
  if (value instanceof PublicKey) return value.toBuffer().toString("hex");
  if (Buffer.isBuffer(value) || value instanceof Uint8Array) return Buffer.from(value).toString("hex");
  if (Array.isArray(value)) {
    return value.every((v) => typeof v === "number")
      ? Buffer.from(value).toString("hex")
      : value.map((v) => plain(v, type.vec));
  }
  if (typeof value === "number") return value;
  if (value && typeof value.toNumber === "function") return value.toNumber();
  throw new Error(`unexpected decoded value ${value}`);
}

function entry(source, data) {
  const decoded = coder.events.decode(data.subarray(EVENT_IX_TAG.length).toString("base64"));
  if (!decoded) throw new Error(`${source}: no IDL event has this discriminator`);
  const fields = idl.types.find((t) => t.name === decoded.name).type.fields;
  const expected = Object.fromEntries(
    fields.map((f) => {
      const v = decoded.data[f.name];
      if (v === undefined) throw new Error(`${source}: ${decoded.name} has no field ${f.name}`);
      return [f.name, plain(v, f.type)];
    }),
  );
  return { source, name: decoded.name, data_b64: data.toString("base64"), expected };
}

const events = [];
if (url === "--recorded") {
  const recorded = JSON.parse(readFileSync(repo("fixtures/events_fixture.json"), "utf8")).events;
  for (const e of recorded) {
    if (e.source !== CODER_SOURCE) events.push(entry(e.source, Buffer.from(e.data_b64, "base64")));
  }
}
const connection = url === "--recorded" ? null : new Connection(url, "confirmed");
for (const signature of url === "--recorded" ? [] : signatures) {
  const tx = await connection.getTransaction(signature, { commitment: "confirmed", maxSupportedTransactionVersion: 0 });
  if (!tx) throw new Error(`transaction ${signature} not found`);
  const keys = tx.transaction.message.getAccountKeys({ accountKeysFromLookups: tx.meta.loadedAddresses });
  for (const group of tx.meta.innerInstructions ?? []) {
    for (const ix of group.instructions) {
      if (keys.get(ix.programIdIndex).toBase58() !== idl.address) continue;
      const data = Buffer.from(bs58.decode(ix.data));
      if (!data.subarray(0, EVENT_IX_TAG.length).equals(EVENT_IX_TAG)) continue;
      events.push(entry(signature, data));
    }
  }
}

// Fixed values for the event types no transaction emitted.
function sample(type) {
  if (type === "pubkey") return new PublicKey(Buffer.alloc(32, 0x11));
  if (type === "u32") return 7;
  // Sets the top byte, so a decoder that drops any of the 32 bytes fails.
  if (isU256(type)) return new BN(1).shln(255).addn(7);
  if (type === "bytes") return Buffer.from([1, 2, 3]);
  if (type.array) return Array(32).fill(0x22);
  if (type.vec) return [sample(type.vec)];
  throw new Error(`no sample for ${JSON.stringify(type)}`);
}
for (const event of idl.events) {
  if (events.some((e) => e.name === event.name)) continue;
  const def = idl.types.find((t) => t.name === event.name);
  const value = Object.fromEntries(def.type.fields.map((f) => [f.name, sample(f.type)]));
  const body = coder.types.encode(event.name, value);
  events.push(entry(CODER_SOURCE, Buffer.concat([EVENT_IX_TAG, Buffer.from(event.discriminator), body])));
}

writeFileSync(
  repo("fixtures/events_fixture.json"),
  JSON.stringify(
    {
      _comment:
        "Anchor CPI event instruction data emitted by the protocol adapter: 8-byte event tag, 8-byte event discriminator, Borsh body. Entries sourced by a signature are the event self-invocations of that transaction; entries sourced by the coder are encoded by @anchor-lang/core's BorshCoder for event types those transactions did not emit. Expected values are that coder's decoding against idl/protocol_adapter.json (byte fields as lowercase hex). Regenerated by tools/capture-events-fixture.mjs. The Rust crate and the TS package must decode every entry to `expected`.",
      event_ix_tag_hex: EVENT_IX_TAG.toString("hex"),
      events,
    },
    null,
    2,
  ) + "\n",
);
console.log(`${events.length} events written: ${[...new Set(events.map((e) => e.name))].join(", ")}`);
