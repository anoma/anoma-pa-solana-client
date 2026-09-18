// Builders for the forwarder's CPI account segments inside a settle
// transaction's remaining accounts, and for the forwarder's own
// `init_nonce_bitmap` instruction. Ordering is owned by the forwarder program;
// integrators must use these builders rather than hand-rolling the slice.

import { TOKEN_PROGRAM_ID } from "@solana/spl-token";
import {
  type AccountMeta,
  SYSVAR_INSTRUCTIONS_PUBKEY,
  SystemProgram,
  TransactionInstruction,
  type PublicKey,
} from "@solana/web3.js";

import { FORWARDER_UNWRAP_NUM_ACCOUNTS, FORWARDER_WRAP_NUM_ACCOUNTS } from "./constants.js";
import { anchorInstructionDisc } from "./discriminator.js";
import {
  deriveAssociatedTokenAddress,
  deriveForwarderConfigPda,
  deriveForwarderEscrowPda,
  deriveNonceBitmapPda,
} from "./pda.js";

/**
 * Nonces per nonce-bitmap account. The bitmap for `nonce` is the one for word
 * `nonce / NONCES_PER_WORD`.
 */
export const NONCES_PER_WORD = 256n;

/** The nonce-bitmap word a wrap nonce falls in. */
export function nonceWordIndex(nonce: bigint): bigint {
  return nonce / NONCES_PER_WORD;
}

/**
 * Build the forwarder's permissionless `init_nonce_bitmap` instruction, which
 * creates `user`'s bitmap for `wordIndex` with `payer` funding the rent.
 *
 * A wrap needs the bitmap for its nonce's word to exist, so a settlement whose
 * word has no bitmap yet (the account at
 * `deriveNonceBitmapPda(forwarderProgram, user, wordIndex)` is absent) carries
 * this instruction before the settle instruction. It fits in the settlement
 * transaction after the ed25519 instruction.
 */
export function initNonceBitmapIx(
  forwarderProgram: PublicKey,
  payer: PublicKey,
  user: PublicKey,
  wordIndex: bigint,
): TransactionInstruction {
  const [nonceBitmapPda] = deriveNonceBitmapPda(forwarderProgram, user, wordIndex);
  const data = new Uint8Array(8 + 32 + 8);
  data.set(anchorInstructionDisc("init_nonce_bitmap"), 0);
  data.set(user.toBytes(), 8);
  new DataView(data.buffer).setBigUint64(40, wordIndex, true);
  return new TransactionInstruction({
    programId: forwarderProgram,
    keys: [
      { pubkey: payer, isSigner: true, isWritable: true },
      { pubkey: nonceBitmapPda, isSigner: false, isWritable: true },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
    ],
    data: Buffer.from(data),
  });
}

/**
 * Build the wrap forwarder CPI segment.
 *
 * Order: `[forwarder_program, config, ix_sysvar, user_ata, escrow_ata,
 * escrow_pda, nonce_bitmap_pda, token_program]`.
 */
export function buildWrapForwarderAccounts(
  forwarderProgram: PublicKey,
  user: PublicKey,
  tokenMint: PublicKey,
  nonce: bigint,
): AccountMeta[] {
  const [configPda] = deriveForwarderConfigPda(forwarderProgram);
  const [escrowPda] = deriveForwarderEscrowPda(forwarderProgram, tokenMint);
  const [nonceBitmapPda] = deriveNonceBitmapPda(forwarderProgram, user, nonceWordIndex(nonce));
  const accounts: AccountMeta[] = [
    { pubkey: forwarderProgram, isSigner: false, isWritable: false },
    { pubkey: configPda, isSigner: false, isWritable: false },
    { pubkey: SYSVAR_INSTRUCTIONS_PUBKEY, isSigner: false, isWritable: false },
    { pubkey: deriveAssociatedTokenAddress(user, tokenMint), isSigner: false, isWritable: true },
    { pubkey: deriveAssociatedTokenAddress(escrowPda, tokenMint), isSigner: false, isWritable: true },
    { pubkey: escrowPda, isSigner: false, isWritable: false },
    { pubkey: nonceBitmapPda, isSigner: false, isWritable: true },
    { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
  ];
  if (accounts.length !== FORWARDER_WRAP_NUM_ACCOUNTS) {
    throw new Error(`wrap segment has ${accounts.length} accounts, expected ${FORWARDER_WRAP_NUM_ACCOUNTS}`);
  }
  return accounts;
}

/**
 * Build the unwrap forwarder CPI segment.
 *
 * Order: `[forwarder_program, config, ix_sysvar, escrow_ata, recipient_ata,
 * escrow_pda, token_program]`.
 */
export function buildUnwrapForwarderAccounts(
  forwarderProgram: PublicKey,
  recipient: PublicKey,
  tokenMint: PublicKey,
): AccountMeta[] {
  const [configPda] = deriveForwarderConfigPda(forwarderProgram);
  const [escrowPda] = deriveForwarderEscrowPda(forwarderProgram, tokenMint);
  const accounts: AccountMeta[] = [
    { pubkey: forwarderProgram, isSigner: false, isWritable: false },
    { pubkey: configPda, isSigner: false, isWritable: false },
    { pubkey: SYSVAR_INSTRUCTIONS_PUBKEY, isSigner: false, isWritable: false },
    { pubkey: deriveAssociatedTokenAddress(escrowPda, tokenMint), isSigner: false, isWritable: true },
    { pubkey: deriveAssociatedTokenAddress(recipient, tokenMint), isSigner: false, isWritable: true },
    { pubkey: escrowPda, isSigner: false, isWritable: false },
    { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
  ];
  if (accounts.length !== FORWARDER_UNWRAP_NUM_ACCOUNTS) {
    throw new Error(`unwrap segment has ${accounts.length} accounts, expected ${FORWARDER_UNWRAP_NUM_ACCOUNTS}`);
  }
  return accounts;
}
