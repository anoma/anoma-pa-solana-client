// The adapter deployed at an address, as pa-evm's bindings return it
// (`contract::protocol_adapter`): its reads under their EVM names, from its
// state and marker accounts, as the Rust crate's `contract` reads them.

import type { IdlInstructionAccount, IdlSeed, IdlSeedConst } from "@anchor-lang/core";
import { sha256 } from "@noble/hashes/sha2";
import {
  AccountRole,
  address,
  appendTransactionMessageInstruction,
  blockhash,
  compileTransaction,
  containsBytes,
  createTransactionMessage,
  fetchEncodedAccount,
  getAddressEncoder,
  getBase64EncodedWireTransaction,
  getBase64Encoder,
  getProgramDerivedAddress,
  pipe,
  setTransactionMessageFeePayer,
  setTransactionMessageLifetimeUsingBlockhash,
  type AccountMeta,
  type Address,
  type Instruction,
  type Rpc,
  type SolanaRpcApiMainnet,
} from "@solana/kit";

import { coder } from "./coder.js";
import { PADDING_LEAF } from "./constants.js";
import { clusterOfGenesisHash, protocolAdapterAddress, type Environment } from "./addresses.js";
import { BindingsError } from "./error.js";
import { idl, idlConstant, idlSeed } from "./idl.js";
import {
  deriveEventAuthorityPda,
  deriveNullifierPda,
  deriveProgramDataAddress,
  deriveRootMarkerPda,
  deriveVerifierRouterPdas,
} from "./pda.js";

/** The RPC's answer `response`, a failed request thrown as `RpcTransportError`. */
async function answered<T>(what: string, response: Promise<T>): Promise<T> {
  try {
    return await response;
  } catch (error) {
    throw new BindingsError({ kind: "RpcTransportError", what, error: String(error) }, { cause: error });
  }
}

/** The adapter `environment` records on the cluster whose genesis hash is `genesisHash`. */
export function recordedAddress(environment: Environment, genesisHash: string): Address {
  const cluster = clusterOfGenesisHash(genesisHash);
  if (cluster === undefined) throw new BindingsError({ kind: "ChainIdUnknown" });
  const recorded = protocolAdapterAddress(environment, cluster);
  if (recorded === undefined) throw new BindingsError({ kind: "UnsupportedChain", cluster });
  return recorded;
}

/** Returns a protocol adapter instance of the environment for the given RPC. */
export async function protocolAdapter(
  rpc: Rpc<SolanaRpcApiMainnet>,
  environment: Environment,
): Promise<ProtocolAdapterInstance> {
  const genesisHash = await answered("the genesis hash", rpc.getGenesisHash().send());
  return new ProtocolAdapterInstance(recordedAddress(environment, genesisHash), rpc);
}

/**
 * The accounts every settlement instruction (`settle`, `simulate_execute`,
 * `settle_from_txdata`) names besides its payer and its upload, then its
 * remaining accounts.
 */
export interface SettlementAccounts {
  /** The marker PDA of the root a settlement that creates commitments produces; `null` for one that creates nothing. */
  newRootMarker: Address | null;
  verifierRouterProgram: Address;
  router: Address;
  verifierEntry: Address;
  verifierProgram: Address;
  /** The nullifier PDAs, each external call's CPI segment, and the historical root markers. */
  remainingAccounts: AccountMeta[];
}

/**
 * A logic ref `deny_logic_refs` adds to a denylist, as pa-evm's
 * `DeniedLogicRef`: `consumed` names the denylist for consumed resources,
 * else the one for created resources.
 */
export interface DeniedLogicRef {
  logicRef: Uint8Array;
  consumed: boolean;
}

/** The settlement accounts by their IDL names, which every settlement instruction takes. */
function settlementNamed(settlement: SettlementAccounts): Record<string, Address | null> {
  return {
    new_root_marker: settlement.newRootMarker,
    verifier_router_program: settlement.verifierRouterProgram,
    router: settlement.router,
    verifier_entry: settlement.verifierEntry,
    verifier_program: settlement.verifierProgram,
  };
}

const isConst = (seed: IdlSeed): seed is IdlSeedConst => seed.kind === "const";

/** The role of an account with the IDL's `signer` and `writable` flags. */
function role(signer: boolean, writable: boolean): AccountRole {
  if (signer) return writable ? AccountRole.WRITABLE_SIGNER : AccountRole.READONLY_SIGNER;
  return writable ? AccountRole.WRITABLE : AccountRole.READONLY;
}

/** The adapter at an address, read over an RPC at its commitment. */
export class ProtocolAdapterInstance {
  /** The adapter at `address`, as alloy's `ProtocolAdapterInstance::new(address, provider)`. */
  constructor(
    /** The address the adapter is deployed at. */
    readonly address: Address,
    private readonly rpc: Rpc<SolanaRpcApiMainnet>,
  ) {}

  /** The addresses that depend only on the adapter's address, each derived on first use. */
  private readonly derived = new Map<string, Promise<Address>>();

  private once(key: string, derive: () => Promise<Address>): Promise<Address> {
    let found = this.derived.get(key);
    if (found === undefined) {
      found = derive();
      this.derived.set(key, found);
    }
    return found;
  }

  private async pda(...seeds: Uint8Array[]): Promise<Address> {
    return (await getProgramDerivedAddress({ programAddress: this.address, seeds }))[0];
  }

  /** The adapter's PDA of the constant `seeds`. */
  private constPda(seeds: Uint8Array[]): Promise<Address> {
    return this.once(`pda ${seeds.map((seed) => seed.join(",")).join(" ")}`, () => this.pda(...seeds));
  }

  private paState(): Promise<Address> {
    return this.constPda([idlSeed("PA_STATE_SEED")]);
  }

  private eventAuthority(): Promise<Address> {
    return this.once("event_authority", async () => (await deriveEventAuthorityPda(this.address))[0]);
  }

  private programData(): Promise<Address> {
    return this.once("program_data", () => deriveProgramDataAddress(this.address));
  }

  /** The data and owner of the account at `address` (`what`), or `null` when it does not exist. */
  private async account(address: Address, what: string): Promise<{ data: Uint8Array; owner: Address } | null> {
    const account = await answered(what, fetchEncodedAccount(this.rpc, address));
    return account.exists ? { data: account.data, owner: account.programAddress } : null;
  }

  /** The adapter's state, as the coder decodes `PAStateAccount`: its IDL field names and the coder's values. */
  private async state(): Promise<any> {
    const address = await this.paState();
    const what = `the adapter's state ${address}`;
    const account = await this.account(address, what);
    if (account === null) throw new BindingsError({ kind: "AccountMissing", address });
    // The layout number sits right after the discriminator in every layout.
    const discriminator = coder.accounts.accountDiscriminator("PAStateAccount");
    const schemaVersion = idlConstant("SCHEMA_VERSION");
    const found = account.data[discriminator.length];
    if (containsBytes(account.data, discriminator, 0) && found !== schemaVersion) {
      throw new BindingsError({
        kind: "Decode",
        what,
        error: `its schema version is ${found}; this client reads ${schemaVersion}`,
      });
    }
    try {
      return coder.accounts.decode("PAStateAccount", account.data);
    } catch (error) {
      throw new BindingsError({ kind: "Decode", what, error: String(error) }, { cause: error });
    }
  }

  /** Whether the account at `address` exists and the adapter owns it: a marker the adapter created. */
  private async isMarker(address: Address, what: string): Promise<boolean> {
    const account = await this.account(address, `${what} ${address}`);
    return account !== null && account.owner === this.address;
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
  async riscZeroVerifierPaused(feePayer: Address): Promise<boolean> {
    const state = await this.state();
    const ix = await this.riscZeroVerifierPausedIx(state.verifier_router, Uint8Array.from(state.proof_selector));
    const transaction = compileTransaction(
      pipe(
        createTransactionMessage({ version: "legacy" }),
        (m) => setTransactionMessageFeePayer(feePayer, m),
        // The RPC replaces it with its latest.
        (m) =>
          setTransactionMessageLifetimeUsingBlockhash(
            { blockhash: blockhash("11111111111111111111111111111111"), lastValidBlockHeight: 0n },
            m,
          ),
        (m) => appendTransactionMessageInstruction(ix, m),
      ),
    );
    const { value } = await answered(
      "the simulation of risc_zero_verifier_paused",
      this.rpc
        .simulateTransaction(getBase64EncodedWireTransaction(transaction), {
          encoding: "base64",
          sigVerify: false,
          replaceRecentBlockhash: true,
        })
        .send(),
    );
    if (value.err !== null) {
      throw new BindingsError({ kind: "Simulation", error: value.err, logs: value.logs ?? [] });
    }
    return returnedBool(this.address, value.returnData);
  }

  /** `RISC_ZERO_VERIFIER_ROUTER()`. */
  async riscZeroVerifierRouter(): Promise<Address> {
    return (await this.state()).verifier_router;
  }

  /** `RISC_ZERO_VERIFIER_SELECTOR()`. */
  async riscZeroVerifierSelector(): Promise<number[]> {
    return (await this.state()).proof_selector;
  }

  /** `commitmentCount()`. */
  async commitmentCount(): Promise<bigint> {
    return (await this.state()).next_index;
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

  /**
   * `isCommitmentTreeRootHistorical(root)`: `root` is the tree's initial root,
   * the padding leaf, which has no marker, or the adapter holds `root`'s marker.
   */
  async isCommitmentTreeRootHistorical(root: Uint8Array): Promise<boolean> {
    if (root.length === PADDING_LEAF.length && root.every((byte, i) => byte === PADDING_LEAF[i])) return true;
    const [marker] = await deriveRootMarkerPda(this.address, await this.paState(), root);
    return this.isMarker(marker, "the root marker");
  }

  /** `isNullifierContained(nullifier)`: the adapter holds `nullifier`'s marker. */
  async isNullifierContained(nullifier: Uint8Array): Promise<boolean> {
    const [marker] = await deriveNullifierPda(this.address, await this.paState(), nullifier);
    return this.isMarker(marker, "the nullifier marker");
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
  async owner(): Promise<Address> {
    return (await this.state()).owner;
  }

  /**
   * The instruction `name` at the adapter's address: `accounts` by their IDL
   * names in the IDL's order and with its flags, then `remaining`, and `args`
   * encoded by the coder. `null` is an absent optional account. The accounts
   * no caller decides are filled in: the IDL's fixed addresses and PDAs of
   * constant seeds, and the event authority and program Anchor's
   * `#[event_cpi]` adds to an instruction that emits events.
   */
  private async instruction(
    name: string,
    accounts: Record<string, Address | null>,
    args: object,
    remaining: AccountMeta[] = [],
  ): Promise<Instruction> {
    const ix = idl.instructions.find((i) => i.name === name);
    if (ix === undefined) throw new Error(`the adapter's IDL declares no instruction ${name}`);
    const names = ix.accounts.map((a) => a.name);
    const unknown = Object.keys(accounts).filter((n) => !names.includes(n));
    if (unknown.length > 0) throw new Error(`${name} takes no account ${unknown.join(", ")}`);
    const emitsEvents = names.includes("event_authority") && names.includes("program");
    const metas = await Promise.all(
      ix.accounts.map(async (a): Promise<AccountMeta> => {
        if ("accounts" in a) throw new Error(`${name}: the account group ${a.name} is not supported`);
        const given = accounts[a.name];
        if (given === null) {
          if (!a.optional) throw new Error(`${name}: ${a.name} is not optional`);
          // Anchor reads an optional account as absent when it is the address of the program it runs.
          return { address: this.address, role: AccountRole.READONLY };
        }
        const account = given ?? (await this.filled(a, emitsEvents));
        if (account === undefined) throw new Error(`${name} takes an account ${a.name} not given`);
        return { address: account, role: role(a.signer ?? false, a.writable ?? false) };
      }),
    );
    return {
      programAddress: this.address,
      accounts: [...metas, ...remaining],
      data: coder.instruction.encode(name, args),
    };
  }

  /** The address of the IDL account `a` no caller decides, or `undefined` when a caller does. */
  private async filled(a: IdlInstructionAccount, emitsEvents: boolean): Promise<Address | undefined> {
    if (a.address !== undefined) return address(a.address);
    const seeds = a.pda?.seeds;
    if (a.pda?.program === undefined && seeds?.every(isConst)) {
      return this.constPda(seeds.map((seed) => Uint8Array.from(seed.value)));
    }
    if (emitsEvents && a.name === "event_authority") return this.eventAuthority();
    if (emitsEvents && a.name === "program") return this.address;
    return undefined;
  }

  private txData(authority: Address, uploadId: bigint): Promise<Address> {
    const id = new Uint8Array(8);
    new DataView(id.buffer).setBigUint64(0, uploadId, true);
    return this.pda(idlSeed("TX_DATA_SEED"), Uint8Array.from(getAddressEncoder().encode(authority)), id);
  }

  /** `initialize`, with `payer` the program's upgrade authority. */
  async initializeIx(
    payer: Address,
    initialOwner: Address,
    verifierRouter: Address,
    proofSelector: Uint8Array,
  ): Promise<Instruction> {
    const [programData, { entry }] = await Promise.all([
      this.programData(),
      deriveVerifierRouterPdas(verifierRouter, proofSelector),
    ]);
    return this.instruction(
      "initialize",
      { payer, program_data: programData, verifier_entry: entry },
      { initial_owner: initialOwner, verifier_router: verifierRouter, proof_selector: Array.from(proofSelector) },
    );
  }

  /** `pause`, by the owner `authority`. */
  pauseIx(authority: Address): Promise<Instruction> {
    return this.instruction("pause", { authority }, {});
  }

  /** `unpause`, by the owner `authority`. */
  unpauseIx(authority: Address): Promise<Instruction> {
    return this.instruction("unpause", { authority }, {});
  }

  /** `deny_logic_refs`, by the owner `authority`, who pays for the entries. */
  denyLogicRefsIx(authority: Address, logicRefs: readonly DeniedLogicRef[]): Promise<Instruction> {
    return this.instruction(
      "deny_logic_refs",
      { authority },
      { logic_refs: logicRefs.map((r) => ({ logic_ref: Array.from(r.logicRef), consumed: r.consumed })) },
    );
  }

  /** `set_kind_tree_root`, by the owner `authority`. */
  setKindTreeRootIx(authority: Address, newKindTreeRoot: Uint8Array): Promise<Instruction> {
    return this.instruction("set_kind_tree_root", { authority }, { new_kind_tree_root: Array.from(newKindTreeRoot) });
  }

  /** `transfer_ownership`, by the owner `authority`. */
  transferOwnershipIx(authority: Address, newOwner: Address): Promise<Instruction> {
    return this.instruction("transfer_ownership", { authority }, { new_owner: newOwner });
  }

  /** `renounce_ownership`, by the owner `authority`. */
  renounceOwnershipIx(authority: Address): Promise<Instruction> {
    return this.instruction("renounce_ownership", { authority }, {});
  }

  /** `update_expiry_config`, by the owner `authority`. */
  updateExpiryConfigIx(authority: Address, newMinExpirySlots: bigint, newMaxExpirySlots: bigint): Promise<Instruction> {
    return this.instruction(
      "update_expiry_config",
      { authority },
      { new_min_expiry_slots: newMinExpirySlots, new_max_expiry_slots: newMaxExpirySlots },
    );
  }

  /** `upgrade` to `buffer`'s code, by the owner `authority`. */
  async upgradeIx(authority: Address, buffer: Address, spill: Address): Promise<Instruction> {
    return this.instruction(
      "upgrade",
      { authority, program_data: await this.programData(), upgraded_program: this.address, buffer, spill },
      {},
    );
  }

  /** `risc_zero_verifier_paused`, for the deployment's `verifierRouter` and `proofSelector`. */
  async riscZeroVerifierPausedIx(verifierRouter: Address, proofSelector: Uint8Array): Promise<Instruction> {
    const { entry } = await deriveVerifierRouterPdas(verifierRouter, proofSelector);
    return this.instruction("risc_zero_verifier_paused", { verifier_entry: entry }, {});
  }

  /** `version`. */
  versionIx(): Promise<Instruction> {
    return this.instruction("version", {}, {});
  }

  /** `txdata_init` of `authority`'s upload `uploadId`. */
  async txdataInitIx(authority: Address, uploadId: bigint, capacity: number, expiresSlot: bigint): Promise<Instruction> {
    return this.instruction(
      "txdata_init",
      { tx_data: await this.txData(authority, uploadId), authority },
      { upload_id: uploadId, capacity, expires_slot: expiresSlot },
    );
  }

  /** `txdata_write` of `chunk` at `offset` into `authority`'s upload `uploadId`. */
  async txdataWriteIx(authority: Address, uploadId: bigint, offset: number, chunk: Uint8Array): Promise<Instruction> {
    return this.instruction(
      "txdata_write",
      { tx_data: await this.txData(authority, uploadId), authority },
      { upload_id: uploadId, offset, data: chunk },
    );
  }

  /** `txdata_extend` of `authority`'s upload `uploadId`. */
  async txdataExtendIx(authority: Address, uploadId: bigint, newExpiresSlot: bigint): Promise<Instruction> {
    return this.instruction(
      "txdata_extend",
      { tx_data: await this.txData(authority, uploadId), authority },
      { upload_id: uploadId, new_expires_slot: newExpiresSlot },
    );
  }

  /** `txdata_close` of `authority`'s upload `uploadId`, its rent going to `refund`. */
  async txdataCloseIx(authority: Address, refund: Address, uploadId: bigint): Promise<Instruction> {
    return this.instruction(
      "txdata_close",
      { tx_data: await this.txData(authority, uploadId), authority, refund },
      { upload_id: uploadId },
    );
  }

  /** `txdata_close_expired` of `authority`'s upload `uploadId`, by `payer`, its rent going to `refund`. */
  async txdataCloseExpiredIx(
    payer: Address,
    refund: Address,
    authority: Address,
    uploadId: bigint,
  ): Promise<Instruction> {
    return this.instruction(
      "txdata_close_expired",
      { tx_data: await this.txData(authority, uploadId), payer, refund },
      { upload_id: uploadId, authority },
    );
  }

  /** `settle` of `input`, paid for by `payer`. */
  settleIx(payer: Address, input: Uint8Array, settlement: SettlementAccounts): Promise<Instruction> {
    return this.instruction(
      "settle",
      { payer, ...settlementNamed(settlement) },
      { transaction_data: input },
      settlement.remainingAccounts,
    );
  }

  /** `simulate_execute` of `input` (pa-evm's `simulateExecute`). */
  simulateExecuteIx(
    payer: Address,
    input: Uint8Array,
    skipRiscZeroProofVerification: boolean,
    settlement: SettlementAccounts,
  ): Promise<Instruction> {
    return this.instruction(
      "simulate_execute",
      { payer, ...settlementNamed(settlement) },
      { transaction_data: input, skip_risc_zero_proof_verification: skipRiscZeroProofVerification },
      settlement.remainingAccounts,
    );
  }

  /** `settle_from_txdata` of `authority`'s upload `uploadId`. */
  async settleFromTxdataIx(authority: Address, uploadId: bigint, settlement: SettlementAccounts): Promise<Instruction> {
    return this.instruction(
      "settle_from_txdata",
      { tx_data: await this.txData(authority, uploadId), authority, ...settlementNamed(settlement) },
      { upload_id: uploadId },
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
export function returnedBool(
  program: Address,
  returnData: Readonly<{ data: readonly [string, "base64"]; programId: Address }> | null,
): boolean {
  const what = `the return data of ${program}`;
  const fail = (error: string) => new BindingsError({ kind: "Decode", what, error });
  if (returnData === null) throw fail("the simulation returned nothing");
  if (returnData.programId !== program) throw fail(`${returnData.programId} returned it`);
  const bytes = getBase64Encoder().encode(returnData.data[0]);
  if (bytes.length !== 1 || bytes[0]! > 1) throw fail(`${bytes.length} byte(s), not one bool: ${Array.from(bytes)}`);
  return bytes[0] === 1;
}
