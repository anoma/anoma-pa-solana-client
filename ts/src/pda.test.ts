import { describe, expect, it } from "vitest";
import { getAssociatedTokenAddressSync } from "@solana/spl-token";
import { Keypair, PublicKey } from "@solana/web3.js";

import {
  deriveAssociatedTokenAddress,
  deriveEventAuthorityPda,
  deriveForwarderEscrowPda,
  derivePaStatePda,
} from "./pda.js";
import { FORWARDER_PROGRAM_ID, PA_PROGRAM_ID } from "./programIds.js";

describe("PDA derivation", () => {
  it("pa_state PDA is deterministic", () => {
    const [a] = derivePaStatePda(PA_PROGRAM_ID);
    const [b] = derivePaStatePda(PA_PROGRAM_ID);
    expect(a.equals(b)).toBe(true);
  });

  it("event authority PDA matches the Rust crate's pin for the devnet V2 adapter", () => {
    // Same literal as the Rust test; both sides derive ["__event_authority"]
    // under the program, which is what Anchor's #[event_cpi] expects.
    const pa = new PublicKey("28Hvr1YFv2ouGN2fS99aF3ZzYXzkncJVVaHcZNhquLFT");
    const [eventAuthority, bump] = deriveEventAuthorityPda(pa);
    expect(eventAuthority.toBase58()).toBe("9G3rrSgAHcJCW75RFGXnZme7hNZNXmgiphDLFGbxpSbv");
    expect(bump).toBe(255);
  });

  it("forwarder escrow PDA differs per mint", () => {
    const mint1 = Keypair.generate().publicKey;
    const mint2 = Keypair.generate().publicKey;
    const [e1] = deriveForwarderEscrowPda(FORWARDER_PROGRAM_ID, mint1);
    const [e2] = deriveForwarderEscrowPda(FORWARDER_PROGRAM_ID, mint2);
    expect(e1.equals(e2)).toBe(false);
  });

  it("ATA derivation agrees with the SPL token library, owner then mint", () => {
    const owner = Keypair.generate().publicKey;
    const mint = Keypair.generate().publicKey;
    const expected = getAssociatedTokenAddressSync(mint, owner);
    expect(deriveAssociatedTokenAddress(owner, mint).equals(expected)).toBe(true);
  });
});
