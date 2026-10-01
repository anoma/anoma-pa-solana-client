import { describe, expect, it } from "vitest";
import { getAssociatedTokenAddressSync } from "@solana/spl-token";
import { Keypair, PublicKey } from "@solana/web3.js";

import {
  deriveAssociatedTokenAddress,
  deriveEventAuthorityPda,
  deriveForwarderEscrowAuthority,
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
    const pa = new PublicKey("5zeqkB3kc9fd1RvaXB2GeMB53Jgf98QJtaFK38e6tTsc");
    const [eventAuthority, bump] = deriveEventAuthorityPda(pa);
    expect(eventAuthority.toBase58()).toBe("5ZycgCWUwuJzmVnvxtsTcb4C7Zjh8y66XcpPpwreZDRM");
    expect(bump).toBe(255);
  });

  it("forwarder escrow authority matches the one the adapter repo derives", () => {
    // Independent pin: the adapter repo's deriveEscrowAuthority (seed "escrow")
    // for the V2 forwarder; the Rust crate pins the same value.
    const [authority, bump] = deriveForwarderEscrowAuthority(FORWARDER_PROGRAM_ID);
    expect(authority.toBase58()).toBe("G78SQtzYuo4YKDEECzh25rckXeJFjLMXy44iWKKG5rDG");
    expect(bump).toBe(255);
  });

  it("ATA derivation accepts a PDA owner, which the forwarder's escrow authority is", () => {
    const mint = Keypair.generate().publicKey;
    const [escrowAuthority] = deriveForwarderEscrowAuthority(FORWARDER_PROGRAM_ID);
    expect(PublicKey.isOnCurve(escrowAuthority.toBytes())).toBe(false);
    expect(
      deriveAssociatedTokenAddress(escrowAuthority, mint).equals(getAssociatedTokenAddressSync(mint, escrowAuthority, true)),
    ).toBe(true);
  });

  it("ATA derivation agrees with the SPL token library, owner then mint", () => {
    const owner = Keypair.generate().publicKey;
    const mint = Keypair.generate().publicKey;
    const expected = getAssociatedTokenAddressSync(mint, owner);
    expect(deriveAssociatedTokenAddress(owner, mint).equals(expected)).toBe(true);
  });
});
