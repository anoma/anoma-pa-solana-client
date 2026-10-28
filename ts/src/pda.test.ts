import {
  address,
  getAddressDecoder,
  getAddressEncoder,
  getProgramDerivedAddress,
  type Address,
  type ReadonlyUint8Array,
} from "@solana/kit";
import { describe, expect, it } from "vitest";

import { MOCK_SELECTOR } from "./constants.js";
import {
  deriveEventAuthorityPda,
  deriveNullifierPda,
  deriveProgramDataAddress,
  deriveRootMarkerPda,
  deriveVerifierRouterPdas,
} from "./pda.js";
import { idl } from "./idl.js";

/** The address of 32 bytes `n`, as the Rust tests' `key(n)`. */
const key = (n: number): Address => getAddressDecoder().decode(new Uint8Array(32).fill(n));
const pdaOf = async (program: Address, ...seeds: (ReadonlyUint8Array | string)[]) =>
  (await getProgramDerivedAddress({ programAddress: program, seeds }))[0];

describe("PDA derivation", () => {
  it("event authority PDA matches the Rust crate's pin for the devnet V2 adapter", async () => {
    // Same literal as the Rust test; both sides derive ["__event_authority"]
    // under the program, which is what Anchor's #[event_cpi] expects.
    const pa = address("5zeqkB3kc9fd1RvaXB2GeMB53Jgf98QJtaFK38e6tTsc");
    const [eventAuthority, bump] = await deriveEventAuthorityPda(pa);
    console.log(`event authority of ${pa}: ${eventAuthority}, bump ${bump}`);
    expect(eventAuthority).toBe("5ZycgCWUwuJzmVnvxtsTcb4C7Zjh8y66XcpPpwreZDRM");
    expect(bump).toBe(255);
  });

  // The adapter's markers: ["nullifier" | "root", pa_state, the 32 bytes].
  it("derives the nullifier and root markers under the adapter's state", async () => {
    const [pa, paState] = [key(1), key(2)];
    const bytes = new Uint8Array(32).fill(7);
    for (const [seed, derive] of [
      ["nullifier", deriveNullifierPda],
      ["root", deriveRootMarkerPda],
    ] as const) {
      const expected = await pdaOf(pa, seed, getAddressEncoder().encode(paState), bytes);
      const [derived] = await derive(pa, paState, bytes);
      console.log(`${seed}: ${derived}`);
      expect(derived, seed).toBe(expected);
    }
  });

  it("the verifier entry is the router's entry for the selector it is given", async () => {
    // An adapter initialized with the mock selector settles through the
    // router's entry for 0xffffffff, not the Groth16 one.
    const routerProgram = key(3);
    const selector = MOCK_SELECTOR;
    const { router, entry } = await deriveVerifierRouterPdas(routerProgram, selector);
    console.log({ router, entry });
    expect(router).toBe(await pdaOf(routerProgram, "router"));
    expect(entry).toBe(await pdaOf(routerProgram, "verifier", selector));
  });

  // The IDL records `initialize`'s program_data for the program it describes.
  it("the program data address is the upgradeable loader's account for the program", async () => {
    const programData = idl.instructions
      .find((i) => i.name === "initialize")!
      .accounts.find((a) => a.name === "program_data");
    if (programData === undefined || "accounts" in programData) throw new Error("initialize names no program_data");
    const derived = await deriveProgramDataAddress(address(idl.address));
    console.log(`program data of ${idl.address}: ${derived}`);
    expect(derived).toBe(programData.address);
  });
});
