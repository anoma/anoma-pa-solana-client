import { describe, expect, it } from "vitest";
import { Keypair, PublicKey } from "@solana/web3.js";

import { deriveEventAuthorityPda, derivePaStatePda, deriveVerifierRouterPdas } from "./pda.js";
import { PA_PROGRAM_ID } from "./programIds.js";

describe("PDA derivation", () => {
  it("pa_state PDA is deterministic", () => {
    const [a] = derivePaStatePda(PA_PROGRAM_ID);
    const [b] = derivePaStatePda(PA_PROGRAM_ID);
    expect(a.equals(b)).toBe(true);
  });

  it("event authority PDA matches the Rust crate's pin for the devnet V2 adapter", () => {
    // Same literal as the Rust test; both sides derive ["__event_authority"]
    // under the program, which is what Anchor's #[event_cpi] expects.
    const pa = new PublicKey("5zeqkB3kc9fd1RvaXB2GeMB53Jgf98QJtaFK38e6tTsc");
    const [eventAuthority, bump] = deriveEventAuthorityPda(pa);
    expect(eventAuthority.toBase58()).toBe("5ZycgCWUwuJzmVnvxtsTcb4C7Zjh8y66XcpPpwreZDRM");
    expect(bump).toBe(255);
  });

  it("the verifier entry is the router's entry for the selector it is given", () => {
    // An adapter initialized with the mock selector settles through the
    // router's entry for 0xffffffff, not the Groth16 one.
    const routerProgram = Keypair.generate().publicKey;
    const selector = new Uint8Array([0xff, 0xff, 0xff, 0xff]);
    const { router, entry } = deriveVerifierRouterPdas(routerProgram, selector);
    const [expectedRouter] = PublicKey.findProgramAddressSync([new TextEncoder().encode("router")], routerProgram);
    const [expectedEntry] = PublicKey.findProgramAddressSync([new TextEncoder().encode("verifier"), selector], routerProgram);
    expect(router.equals(expectedRouter)).toBe(true);
    expect(entry.equals(expectedEntry)).toBe(true);
  });
});
