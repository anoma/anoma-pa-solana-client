// Cross-package fixture: verifies that the TS package serializes the canonical
// WrapMessage to the same bytes the Rust crate produces (verified by
// `rust/tests/cross_package_fixture.rs`). Both implementations against the same
// input must agree byte-for-byte; any divergence is a wire incompatibility
// against the PA.

import { describe, expect, it } from "vitest";

import { base64WrapDigest, buildWrapMessage, hashWrapMessage } from "./wrapMessage.js";

const FIXTURE = {
  forwarderId: new Uint8Array(32).fill(0x01),
  tokenMint: new Uint8Array(32).fill(0x02),
  amount: 1_000_000n,
  nonce: 7n,
  deadline: 1_700_000_000n,
  actionTreeRoot: new Uint8Array(32).fill(0x03),
};

const FIXTURE_SERIALIZED_HEX =
  "0101010101010101010101010101010101010101010101010101010101010101" + // forwarder
  "0202020202020202020202020202020202020202020202020202020202020202" + // mint
  "40420f0000000000" + // amount=1_000_000 u64 LE
  "0700000000000000" + // nonce=7 u64 LE
  "00f1536500000000" + // deadline=1_700_000_000 i64 LE
  "0303030303030303030303030303030303030303030303030303030303030303"; // action_tree_root

const FIXTURE_SHA256_DIGEST_HEX =
  "96a1c836883ef3c223d918728ee2698fbd48998e2017ee028aa23c221b882886";

const toHex = (bytes: Uint8Array) =>
  Array.from(bytes).map(b => b.toString(16).padStart(2, "0")).join("");

describe("cross-package fixture", () => {
  it("TS serialize matches Rust serialize byte-for-byte", () => {
    expect(toHex(buildWrapMessage(FIXTURE))).toBe(FIXTURE_SERIALIZED_HEX);
  });

  it("TS sha256 digest matches Rust sha256 digest", () => {
    expect(toHex(hashWrapMessage(FIXTURE))).toBe(FIXTURE_SHA256_DIGEST_HEX);
  });

  it("TS base64 digest is 44 chars", () => {
    expect(base64WrapDigest(FIXTURE).length).toBe(44);
  });
});
