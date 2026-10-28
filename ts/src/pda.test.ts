import { describe, expect, it } from "vitest";
import { Keypair, PublicKey } from "@solana/web3.js";

import { MOCK_SELECTOR } from "./constants.js";
import {
  deriveEventAuthorityPda,
  deriveNullifierPda,
  deriveProgramDataAddress,
  deriveRootMarkerPda,
  deriveVerifierRouterPdas,
} from "./pda.js";
import { idl } from "./idl.js";

const encode = (s: string) => new TextEncoder().encode(s);

describe("PDA derivation", () => {
  it("event authority PDA matches the Rust crate's pin for the devnet V2 adapter", () => {
    // Same literal as the Rust test; both sides derive ["__event_authority"]
    // under the program, which is what Anchor's #[event_cpi] expects.
    const pa = new PublicKey("5zeqkB3kc9fd1RvaXB2GeMB53Jgf98QJtaFK38e6tTsc");
    const [eventAuthority, bump] = deriveEventAuthorityPda(pa);
    expect(eventAuthority.toBase58()).toBe("5ZycgCWUwuJzmVnvxtsTcb4C7Zjh8y66XcpPpwreZDRM");
    expect(bump).toBe(255);
  });

  // The adapter's markers: ["nullifier" | "root", pa_state, the 32 bytes].
  it("derives the nullifier and root markers under the adapter's state", () => {
    const pa = Keypair.generate().publicKey;
    const paState = Keypair.generate().publicKey;
    const bytes = new Uint8Array(32).fill(7);
    for (const [seed, derive] of [
      ["nullifier", deriveNullifierPda],
      ["root", deriveRootMarkerPda],
    ] as const) {
      const [expected] = PublicKey.findProgramAddressSync([encode(seed), paState.toBuffer(), bytes], pa);
      expect(derive(pa, paState, bytes)[0].equals(expected), seed).toBe(true);
    }
  });

  it("the verifier entry is the router's entry for the selector it is given", () => {
    // An adapter initialized with the mock selector settles through the
    // router's entry for 0xffffffff, not the Groth16 one.
    const routerProgram = Keypair.generate().publicKey;
    const selector = MOCK_SELECTOR;
    const { router, entry } = deriveVerifierRouterPdas(routerProgram, selector);
    const [expectedRouter] = PublicKey.findProgramAddressSync([encode("router")], routerProgram);
    const [expectedEntry] = PublicKey.findProgramAddressSync([encode("verifier"), selector], routerProgram);
    expect(router.equals(expectedRouter)).toBe(true);
    expect(entry.equals(expectedEntry)).toBe(true);
  });

  // The IDL records `initialize`'s program_data for the program it describes.
  it("the program data address is the upgradeable loader's account for the program", () => {
    const programData = idl.instructions
      .find((i) => i.name === "initialize")!
      .accounts.find((a) => a.name === "program_data");
    if (programData === undefined || "accounts" in programData) throw new Error("initialize names no program_data");
    expect(deriveProgramDataAddress(new PublicKey(idl.address)).toBase58()).toBe(programData.address);
  });
});
