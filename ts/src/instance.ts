// The adapter deployed at an address, as pa-evm's bindings return it
// (`contract::protocol_adapter`): its reads under their EVM names, from its
// state and marker accounts, as the Rust crate's `instance` reads them.

import { sha256 } from "@noble/hashes/sha2";
import BN from "bn.js";
import {
  Connection,
  PublicKey,
  TransactionInstruction,
  TransactionMessage,
  VersionedTransaction,
  type AccountMeta,
  type TransactionError,
  type TransactionReturnData,
} from "@solana/web3.js";

import { coder } from "./coder.js";
import { PADDING_LEAF } from "./constants.js";
import { chainIdOfGenesisHash, deployment, type Deployment, type DeploymentRecord, type Environment } from "./deployments.js";
import { idl, idlConstant, idlSeed } from "./idl.js";
import {
  deriveEventAuthorityPda,
  deriveNullifierPda,
  deriveProgramDataAddress,
  deriveRootMarkerPda,
  deriveVerifierRouterPdas,
} from "./pda.js";

/** Why an instance could not be made or could not answer. */
export type InstanceErrorDetail =
  /** The environment records no deployed adapter on the cluster. */
  | { kind: "NoDeployment"; chainId: string }
  /** An account the answer is read from does not exist. */
  | { kind: "AccountMissing"; address: PublicKey }
  /** An RPC request failed: retrying may succeed. */
  | { kind: "Rpc"; what: string; error: string }
  /** What the chain holds does not decode. */
  | { kind: "Decode"; what: string; error: string }
  /** A simulated instruction failed. */
  | { kind: "Simulation"; error: TransactionError; logs: string[] };

function describe(detail: InstanceErrorDetail): string {
  switch (detail.kind) {
    case "NoDeployment":
      return `no deployed adapter is recorded on ${detail.chainId}`;
    case "AccountMissing":
      return `the account ${detail.address.toBase58()} does not exist`;
    case "Rpc":
      return `failed to fetch ${detail.what}: ${detail.error}`;
    case "Decode":
      return `${detail.what} does not decode: ${detail.error}`;
    case "Simulation":
      return `the simulation failed: ${JSON.stringify(detail.error)}; logs: ${JSON.stringify(detail.logs)}`;
  }
}

export class InstanceError extends Error {
  constructor(
    readonly detail: InstanceErrorDetail,
    options?: ErrorOptions,
  ) {
    super(describe(detail), options);
    this.name = "InstanceError";
  }
}

async function rpc<T>(what: string, request: Promise<T>): Promise<T> {
  try {
    return await request;
  } catch (error) {
    throw new InstanceError({ kind: "Rpc", what, error: String(error) }, { cause: error });
  }
}

/**
 * The adapter `environment` records on the cluster whose genesis hash is
 * `genesisHash`, in `record` (the package's `deployments.json` by default).
 */
export function recordedDeployment(
  environment: Environment,
  genesisHash: string,
  record?: DeploymentRecord,
): Deployment {
  const id = chainIdOfGenesisHash(genesisHash);
  const recorded = deployment(environment, id, record);
  if (recorded === undefined) throw new InstanceError({ kind: "NoDeployment", chainId: id });
  return recorded;
}

/**
 * The adapter `environment` records on the cluster behind `connection`, as
 * pa-evm's `contract::protocol_adapter(provider, environment)`.
 */
export async function protocolAdapter(connection: Connection, environment: Environment): Promise<ProtocolAdapter> {
  const genesisHash = await rpc("the genesis hash", connection.getGenesisHash());
  return ProtocolAdapter.at(connection, recordedDeployment(environment, genesisHash).programId);
}

/**
 * The accounts every settlement instruction (`settle`, `simulate_execute`,
 * `settle_from_txdata`) names besides its payer and its upload, then its
 * remaining accounts.
 */
export interface SettlementAccounts {
  paState: PublicKey;
  /** The marker PDA of the root a settlement that creates commitments produces; `null` for one that creates nothing. */
  newRootMarker: PublicKey | null;
  verifierRouterProgram: PublicKey;
  router: PublicKey;
  verifierEntry: PublicKey;
  verifierProgram: PublicKey;
  /** The nullifier PDAs, each external call's CPI segment, and the historical root markers. */
  remainingAccounts: AccountMeta[];
}

/** A u64 argument in the form the coder encodes. */
const u64 = (value: bigint): BN => new BN(value.toString());

/** The adapter at an address, read over a connection at its commitment. */
export class ProtocolAdapter {
  private constructor(
    private readonly connection: Connection,
    /** The address the adapter is deployed at. */
    readonly programId: PublicKey,
  ) {}

  /** The adapter at `programId`, as alloy's `IProtocolAdapter::new(address, provider)`. */
  static at(connection: Connection, programId: PublicKey): ProtocolAdapter {
    return new ProtocolAdapter(connection, programId);
  }

  private paState(): PublicKey {
    return PublicKey.findProgramAddressSync([idlSeed("PA_STATE_SEED")], this.programId)[0];
  }

  /** The adapter's state, as the coder decodes `PAStateAccount`: its IDL field names and the coder's values. */
  private async state(): Promise<any> {
    const address = this.paState();
    const what = `the adapter's state ${address.toBase58()}`;
    const account = await rpc(what, this.connection.getAccountInfo(address));
    if (account === null) throw new InstanceError({ kind: "AccountMissing", address });
    // The layout number sits right after the discriminator in every layout.
    const discriminator = coder.accounts.accountDiscriminator("PAStateAccount");
    const schemaVersion = idlConstant("SCHEMA_VERSION");
    const found = account.data[discriminator.length];
    if (account.data.subarray(0, discriminator.length).equals(discriminator) && found !== schemaVersion) {
      throw new InstanceError({
        kind: "Decode",
        what,
        error: `its schema version is ${found}; this client reads ${schemaVersion}`,
      });
    }
    try {
      return coder.accounts.decode("PAStateAccount", account.data);
    } catch (error) {
      throw new InstanceError({ kind: "Decode", what, error: String(error) }, { cause: error });
    }
  }

  /** Whether the account at `address` exists and the adapter owns it: a marker the adapter created. */
  private async isMarker(address: PublicKey, what: string): Promise<boolean> {
    const account = await rpc(`${what} ${address.toBase58()}`, this.connection.getAccountInfo(address));
    return account !== null && account.owner.equals(this.programId);
  }

  /** `paused()`. */
  async paused(): Promise<boolean> {
    return (await this.state()).paused;
  }

  /**
   * `riscZeroVerifierPaused()`: the adapter's `risc_zero_verifier_paused`
   * simulated, with `feePayer`, any funded system account, paying the
   * simulation's fee (nothing is signed or sent).
   */
  async riscZeroVerifierPaused(feePayer: PublicKey): Promise<boolean> {
    const state = await this.state();
    const ix = this.riscZeroVerifierPausedIx(state.verifier_router, Uint8Array.from(state.proof_selector));
    const message = new TransactionMessage({
      payerKey: feePayer,
      // The RPC replaces it with its latest.
      recentBlockhash: PublicKey.default.toBase58(),
      instructions: [ix],
    }).compileToLegacyMessage();
    const { value } = await rpc(
      "the simulation of risc_zero_verifier_paused",
      this.connection.simulateTransaction(new VersionedTransaction(message), {
        sigVerify: false,
        replaceRecentBlockhash: true,
      }),
    );
    if (value.err !== null) {
      throw new InstanceError({ kind: "Simulation", error: value.err, logs: value.logs ?? [] });
    }
    return returnedBool(this.programId, value.returnData ?? null);
  }

  /** `RISC_ZERO_VERIFIER_ROUTER()`. */
  async riscZeroVerifierRouter(): Promise<PublicKey> {
    return (await this.state()).verifier_router;
  }

  /** `RISC_ZERO_VERIFIER_SELECTOR()`. */
  async riscZeroVerifierSelector(): Promise<number[]> {
    return (await this.state()).proof_selector;
  }

  /** `commitmentCount()`. */
  async commitmentCount(): Promise<bigint> {
    return BigInt((await this.state()).next_index.toString());
  }

  /** `commitmentTreeDepth()`. */
  async commitmentTreeDepth(): Promise<number> {
    return (await this.state()).current_depth;
  }

  /** `commitmentTreeCapacity()`. */
  async commitmentTreeCapacity(): Promise<bigint> {
    return capacity((await this.state()).current_depth);
  }

  /** `commitmentTreeLeftSiblings()`, from the leaf level up. */
  async commitmentTreeLeftSiblings(): Promise<number[][]> {
    return (await this.state()).frontier;
  }

  /** `commitmentTreeEmptySubtreeRoots()`, from the leaf level up. */
  async commitmentTreeEmptySubtreeRoots(): Promise<number[][]> {
    return emptySubtreeRoots((await this.state()).current_depth);
  }

  /** `latestCommitmentTreeRoot()`. */
  async latestCommitmentTreeRoot(): Promise<number[]> {
    return (await this.state()).root;
  }

  /** `isCommitmentTreeRootHistorical(root)`: the adapter holds `root`'s marker. */
  async isCommitmentTreeRootHistorical(root: Uint8Array): Promise<boolean> {
    return this.isMarker(deriveRootMarkerPda(this.programId, this.paState(), root)[0], "the root marker");
  }

  /** `isNullifierContained(nullifier)`: the adapter holds `nullifier`'s marker. */
  async isNullifierContained(nullifier: Uint8Array): Promise<boolean> {
    return this.isMarker(deriveNullifierPda(this.programId, this.paState(), nullifier)[0], "the nullifier marker");
  }

  /** `getKindTreeRoot()`. */
  async getKindTreeRoot(): Promise<number[]> {
    return (await this.state()).kind_tree_root;
  }

  /** `EMPTY_KIND_TREE_ROOT()`. */
  emptyKindTreeRoot(): number[] {
    return idlConstant("EMPTY_KIND_TREE_ROOT") as number[];
  }

  /** `isLogicRefDenied(logicRef, consumed)`. */
  async isLogicRefDenied(logicRef: Uint8Array, consumed: boolean): Promise<boolean> {
    return isLogicRefDenied(await this.state(), logicRef, consumed);
  }

  /** `owner()`: the zero key once the ownership is renounced. */
  async owner(): Promise<PublicKey> {
    return (await this.state()).owner;
  }

  /**
   * The instruction `name` at the adapter's address: `accounts` by their IDL
   * names in the IDL's order and with its flags (an account the IDL gives a
   * fixed address may be left out; `null` is an absent optional account),
   * then `remaining`, and `args` encoded by the coder.
   */
  private instruction(
    name: string,
    accounts: Record<string, PublicKey | null>,
    args: object,
    remaining: AccountMeta[] = [],
  ): TransactionInstruction {
    const ix = idl.instructions.find((i) => i.name === name);
    if (ix === undefined) throw new Error(`the adapter's IDL declares no instruction ${name}`);
    const names = ix.accounts.map((a) => a.name);
    const unknown = Object.keys(accounts).filter((n) => !names.includes(n));
    if (unknown.length > 0) throw new Error(`${name} takes no account ${unknown.join(", ")}`);
    const keys = ix.accounts.map((a) => {
      if ("accounts" in a) throw new Error(`${name}: the account group ${a.name} is not supported`);
      const given = accounts[a.name];
      if (given === null) {
        if (!a.optional) throw new Error(`${name}: ${a.name} is not optional`);
        // Anchor reads an optional account as absent when it is the address of the program it runs.
        return { pubkey: this.programId, isSigner: false, isWritable: false };
      }
      const pubkey = given ?? (a.address === undefined ? undefined : new PublicKey(a.address));
      if (pubkey === undefined) throw new Error(`${name} takes an account ${a.name} not given`);
      return { pubkey, isSigner: a.signer ?? false, isWritable: a.writable ?? false };
    });
    return new TransactionInstruction({
      programId: this.programId,
      keys: [...keys, ...remaining],
      data: coder.instruction.encode(name, args),
    });
  }

  /** The accounts Anchor's `#[event_cpi]` adds to an instruction that emits events. */
  private events(): Record<string, PublicKey> {
    return { event_authority: deriveEventAuthorityPda(this.programId)[0], program: this.programId };
  }

  private upgradeAuthority(): PublicKey {
    return PublicKey.findProgramAddressSync([idlSeed("UPGRADE_AUTHORITY_SEED")], this.programId)[0];
  }

  private txData(authority: PublicKey, uploadId: bigint): PublicKey {
    const id = new Uint8Array(8);
    new DataView(id.buffer).setBigUint64(0, uploadId, true);
    return PublicKey.findProgramAddressSync([idlSeed("TX_DATA_SEED"), authority.toBuffer(), id], this.programId)[0];
  }

  /** `initialize`, with `payer` the program's upgrade authority. */
  initializeIx(
    payer: PublicKey,
    initialOwner: PublicKey,
    verifierRouter: PublicKey,
    proofSelector: Uint8Array,
  ): TransactionInstruction {
    return this.instruction(
      "initialize",
      {
        pa_state: this.paState(),
        payer,
        program_data: deriveProgramDataAddress(this.programId),
        upgrade_authority: this.upgradeAuthority(),
        verifier_entry: deriveVerifierRouterPdas(verifierRouter, proofSelector).entry,
        ...this.events(),
      },
      { initial_owner: initialOwner, verifier_router: verifierRouter, proof_selector: Array.from(proofSelector) },
    );
  }

  /** `pause`, by the owner `authority`. */
  pauseIx(authority: PublicKey): TransactionInstruction {
    return this.instruction("pause", { pa_state: this.paState(), authority, ...this.events() }, {});
  }

  /** `unpause`, by the owner `authority`. */
  unpauseIx(authority: PublicKey): TransactionInstruction {
    return this.instruction("unpause", { pa_state: this.paState(), authority, ...this.events() }, {});
  }

  /** `set_kind_tree_root`, by the owner `authority`. */
  setKindTreeRootIx(authority: PublicKey, newKindTreeRoot: Uint8Array): TransactionInstruction {
    return this.instruction(
      "set_kind_tree_root",
      { pa_state: this.paState(), authority, ...this.events() },
      { new_kind_tree_root: Array.from(newKindTreeRoot) },
    );
  }

  /** `transfer_ownership`, by the owner `authority`. */
  transferOwnershipIx(authority: PublicKey, newOwner: PublicKey): TransactionInstruction {
    return this.instruction(
      "transfer_ownership",
      { pa_state: this.paState(), authority, ...this.events() },
      { new_owner: newOwner },
    );
  }

  /** `renounce_ownership`, by the owner `authority`. */
  renounceOwnershipIx(authority: PublicKey): TransactionInstruction {
    return this.instruction("renounce_ownership", { pa_state: this.paState(), authority, ...this.events() }, {});
  }

  /** `update_expiry_config`, by the owner `authority`. */
  updateExpiryConfigIx(
    authority: PublicKey,
    newMinExpirySlots: bigint,
    newMaxExpirySlots: bigint,
  ): TransactionInstruction {
    return this.instruction(
      "update_expiry_config",
      { pa_state: this.paState(), authority },
      { new_min_expiry_slots: u64(newMinExpirySlots), new_max_expiry_slots: u64(newMaxExpirySlots) },
    );
  }

  /** `upgrade` to `buffer`'s code, by the owner `authority`. */
  upgradeIx(authority: PublicKey, buffer: PublicKey, spill: PublicKey): TransactionInstruction {
    return this.instruction(
      "upgrade",
      {
        pa_state: this.paState(),
        authority,
        program_data: deriveProgramDataAddress(this.programId),
        upgraded_program: this.programId,
        buffer,
        spill,
        upgrade_authority: this.upgradeAuthority(),
        ...this.events(),
      },
      {},
    );
  }

  /** `risc_zero_verifier_paused`, for the deployment's `verifierRouter` and `proofSelector`. */
  riscZeroVerifierPausedIx(verifierRouter: PublicKey, proofSelector: Uint8Array): TransactionInstruction {
    return this.instruction(
      "risc_zero_verifier_paused",
      { pa_state: this.paState(), verifier_entry: deriveVerifierRouterPdas(verifierRouter, proofSelector).entry },
      {},
    );
  }

  /** `version`. */
  versionIx(): TransactionInstruction {
    return this.instruction("version", {}, {});
  }

  /** `txdata_init` of `authority`'s upload `uploadId`. */
  txdataInitIx(authority: PublicKey, uploadId: bigint, capacity: number, expiresSlot: bigint): TransactionInstruction {
    return this.instruction(
      "txdata_init",
      { pa_state: this.paState(), tx_data: this.txData(authority, uploadId), authority },
      { upload_id: u64(uploadId), capacity, expires_slot: u64(expiresSlot) },
    );
  }

  /** `txdata_extend` of `authority`'s upload `uploadId`. */
  txdataExtendIx(authority: PublicKey, uploadId: bigint, newExpiresSlot: bigint): TransactionInstruction {
    return this.instruction(
      "txdata_extend",
      { pa_state: this.paState(), tx_data: this.txData(authority, uploadId), authority },
      { upload_id: u64(uploadId), new_expires_slot: u64(newExpiresSlot) },
    );
  }

  /** `txdata_close` of `authority`'s upload `uploadId`, its rent going to `refund`. */
  txdataCloseIx(authority: PublicKey, refund: PublicKey, uploadId: bigint): TransactionInstruction {
    return this.instruction(
      "txdata_close",
      { tx_data: this.txData(authority, uploadId), authority, refund },
      { upload_id: u64(uploadId) },
    );
  }

  /** `txdata_close_expired` of `authority`'s upload `uploadId`, by `payer`, its rent going to `refund`. */
  txdataCloseExpiredIx(
    payer: PublicKey,
    refund: PublicKey,
    authority: PublicKey,
    uploadId: bigint,
  ): TransactionInstruction {
    return this.instruction(
      "txdata_close_expired",
      { tx_data: this.txData(authority, uploadId), payer, refund },
      { upload_id: u64(uploadId), _authority: authority },
    );
  }

  /** `settle_from_txdata` of `authority`'s upload `uploadId`. */
  settleFromTxdataIx(authority: PublicKey, uploadId: bigint, settlement: SettlementAccounts): TransactionInstruction {
    return this.instruction(
      "settle_from_txdata",
      {
        pa_state: settlement.paState,
        tx_data: this.txData(authority, uploadId),
        authority,
        new_root_marker: settlement.newRootMarker,
        verifier_router_program: settlement.verifierRouterProgram,
        router: settlement.router,
        verifier_entry: settlement.verifierEntry,
        verifier_program: settlement.verifierProgram,
        ...this.events(),
      },
      { _upload_id: u64(uploadId) },
      settlement.remainingAccounts,
    );
  }
}

/** `commitmentTreeCapacity()` of a tree of depth `depth`: two to its power. */
export function capacity(depth: number): bigint {
  return 1n << BigInt(depth);
}

/**
 * `commitmentTreeEmptySubtreeRoots()` of a tree of depth `depth`: the root of
 * an empty subtree of each height from 0 (the padding leaf) to the depth.
 */
export function emptySubtreeRoots(depth: number): number[][] {
  const roots = [Array.from(PADDING_LEAF)];
  while (roots.length <= depth) {
    const below = roots[roots.length - 1]!;
    roots.push(Array.from(sha256(Uint8Array.from([...below, ...below]))));
  }
  return roots;
}

/** `isLogicRefDenied(logicRef, consumed)` of `state`: the denylist of the side `consumed` names holds `logicRef`. */
export function isLogicRefDenied(
  state: { denied_consumed_logic_refs: number[][]; denied_created_logic_refs: number[][] },
  logicRef: Uint8Array,
  consumed: boolean,
): boolean {
  const denied = consumed ? state.denied_consumed_logic_refs : state.denied_created_logic_refs;
  return denied.some((r) => r.length === logicRef.length && r.every((b, i) => b === logicRef[i]));
}

/** The bool `program` returned, as a simulation reports it. */
export function returnedBool(program: PublicKey, returnData: TransactionReturnData | null): boolean {
  const what = `the return data of ${program.toBase58()}`;
  const fail = (error: string) => new InstanceError({ kind: "Decode", what, error });
  if (returnData === null) throw fail("the simulation returned nothing");
  if (returnData.programId !== program.toBase58()) throw fail(`${returnData.programId} returned it`);
  const bytes = Uint8Array.from(atob(returnData.data[0]), (c) => c.charCodeAt(0));
  if (bytes.length !== 1 || bytes[0]! > 1) throw fail(`${bytes.length} byte(s), not one bool: ${Array.from(bytes)}`);
  return bytes[0] === 1;
}
