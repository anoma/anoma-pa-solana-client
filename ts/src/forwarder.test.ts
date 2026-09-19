import { TOKEN_PROGRAM_ID } from "@solana/spl-token";
import { Keypair, SYSVAR_INSTRUCTIONS_PUBKEY, SystemProgram } from "@solana/web3.js";
import { describe, expect, it } from "vitest";

import { FORWARDER_UNWRAP_NUM_ACCOUNTS, FORWARDER_WRAP_NUM_ACCOUNTS } from "./constants.js";
import {
  buildUnwrapForwarderAccounts,
  buildWrapForwarderAccounts,
  initNonceBitmapIx,
  nonceWordIndex,
} from "./forwarder.js";
import {
  deriveAssociatedTokenAddress,
  deriveForwarderConfigPda,
  deriveForwarderEscrowPda,
  deriveNonceBitmapPda,
} from "./pda.js";

const forwarder = Keypair.generate().publicKey;
const mint = Keypair.generate().publicKey;

describe("forwarder segment builders", () => {
  it("wrap segment is the forwarder layout", () => {
    const user = Keypair.generate().publicKey;
    const accounts = buildWrapForwarderAccounts(forwarder, user, mint, 300n);
    expect(accounts).toHaveLength(FORWARDER_WRAP_NUM_ACCOUNTS);

    const [escrowPda] = deriveForwarderEscrowPda(forwarder, mint);
    expect(accounts.map((a) => a.pubkey.toBase58())).toEqual(
      [
        forwarder,
        deriveForwarderConfigPda(forwarder)[0],
        SYSVAR_INSTRUCTIONS_PUBKEY,
        deriveAssociatedTokenAddress(user, mint),
        deriveAssociatedTokenAddress(escrowPda, mint),
        escrowPda,
        deriveNonceBitmapPda(forwarder, user, 1n)[0], // nonce 300 is in word 1
        TOKEN_PROGRAM_ID,
      ].map((k) => k.toBase58()),
    );
    expect(accounts.map((a) => a.isWritable)).toEqual([false, false, false, true, true, false, true, false]);
    expect(accounts.every((a) => !a.isSigner)).toBe(true);
  });

  it("unwrap segment is the forwarder layout", () => {
    const recipient = Keypair.generate().publicKey;
    const accounts = buildUnwrapForwarderAccounts(forwarder, recipient, mint);
    expect(accounts).toHaveLength(FORWARDER_UNWRAP_NUM_ACCOUNTS);

    const [escrowPda] = deriveForwarderEscrowPda(forwarder, mint);
    expect(accounts.map((a) => a.pubkey.toBase58())).toEqual(
      [
        forwarder,
        deriveForwarderConfigPda(forwarder)[0],
        SYSVAR_INSTRUCTIONS_PUBKEY,
        deriveAssociatedTokenAddress(escrowPda, mint),
        deriveAssociatedTokenAddress(recipient, mint),
        escrowPda,
        TOKEN_PROGRAM_ID,
      ].map((k) => k.toBase58()),
    );
    expect(accounts.map((a) => a.isWritable)).toEqual([false, false, false, true, true, false, false]);
    expect(accounts.every((a) => !a.isSigner)).toBe(true);
  });

  it("nonce word index covers 256 nonces per word", () => {
    expect(nonceWordIndex(0n)).toBe(0n);
    expect(nonceWordIndex(255n)).toBe(0n);
    expect(nonceWordIndex(256n)).toBe(1n);
  });

  it("init_nonce_bitmap instruction matches the forwarder IDL", () => {
    const payer = Keypair.generate().publicKey;
    const user = Keypair.generate().publicKey;
    const ix = initNonceBitmapIx(forwarder, payer, user, 7n);

    expect(ix.programId.equals(forwarder)).toBe(true);
    // Discriminator from idl/spl_token_forwarder.json, then the two args.
    expect(Array.from(ix.data.subarray(0, 8))).toEqual([214, 13, 125, 121, 72, 220, 241, 42]);
    expect(Array.from(ix.data.subarray(8, 40))).toEqual(Array.from(user.toBytes()));
    expect(Array.from(ix.data.subarray(40))).toEqual([7, 0, 0, 0, 0, 0, 0, 0]);

    const [expectedBitmap] = deriveNonceBitmapPda(forwarder, user, 7n);
    expect(ix.keys.map((k) => [k.pubkey.toBase58(), k.isSigner, k.isWritable])).toEqual([
      [payer.toBase58(), true, true],
      [expectedBitmap.toBase58(), false, true],
      [SystemProgram.programId.toBase58(), false, false],
    ]);
  });
});
