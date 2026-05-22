import { describe, expect, it } from "vitest";
import { Keypair, PublicKey } from "@solana/web3.js";

import {
  deriveAssociatedTokenAddress,
  deriveForwarderEscrowPda,
  derivePaStatePda,
} from "./pda.js";
import { FORWARDER_PROGRAM_ID, PA_PROGRAM_ID, SPL_TOKEN_PROGRAM_ID, ASSOCIATED_TOKEN_PROGRAM_ID } from "./programIds.js";

describe("PDA derivation", () => {
  it("pa_state PDA is deterministic", () => {
    const [a] = derivePaStatePda(PA_PROGRAM_ID);
    const [b] = derivePaStatePda(PA_PROGRAM_ID);
    expect(a.equals(b)).toBe(true);
  });

  it("forwarder escrow PDA differs per mint", () => {
    const mint1 = Keypair.generate().publicKey;
    const mint2 = Keypair.generate().publicKey;
    const [e1] = deriveForwarderEscrowPda(FORWARDER_PROGRAM_ID, mint1);
    const [e2] = deriveForwarderEscrowPda(FORWARDER_PROGRAM_ID, mint2);
    expect(e1.equals(e2)).toBe(false);
  });

  it("ATA derivation matches the standard SPL seed order", () => {
    const wallet = Keypair.generate().publicKey;
    const mint = Keypair.generate().publicKey;
    const ata = deriveAssociatedTokenAddress(wallet, mint);
    const [expected] = PublicKey.findProgramAddressSync(
      [wallet.toBuffer(), SPL_TOKEN_PROGRAM_ID.toBuffer(), mint.toBuffer()],
      ASSOCIATED_TOKEN_PROGRAM_ID,
    );
    expect(ata.equals(expected)).toBe(true);
  });
});
