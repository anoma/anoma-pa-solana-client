// The adapter deployed at an address, as pa-evm's bindings return it
// (`contract::protocol_adapter`): its reads under their EVM names, from its
// state and marker accounts, as the Rust crate's `instance` reads them.

import { sha256 } from "@noble/hashes/sha2";
import {
  Connection,
  PublicKey,
  TransactionInstruction,
  TransactionMessage,
  VersionedTransaction,
  type TransactionError,
  type TransactionReturnData,
} from "@solana/web3.js";

import { coder } from "./coder.js";
import { PADDING_LEAF } from "./constants.js";
import { chainId, cluster, deployment, type Deployment, type DeploymentRecord, type Environment } from "./deployments.js";
import { idl, idlConstant, idlSeed } from "./idl.js";
import { deriveNullifierPda, deriveRootMarkerPda, deriveVerifierRouterPdas } from "./pda.js";

/** Why an instance could not be made or could not answer. */
export type InstanceErrorDetail =
  /** The cluster behind the RPC has a genesis hash no public cluster has. */
  | { kind: "UnknownCluster"; genesisHash: string }
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
    case "UnknownCluster":
      return `no public cluster has the genesis hash ${detail.genesisHash}`;
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
  const found = cluster(genesisHash);
  if (found === undefined) throw new InstanceError({ kind: "UnknownCluster", genesisHash });
  const id = chainId(found);
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
   * `risc_zero_verifier_paused`, for the deployment's `verifierRouter` and
   * `proofSelector`: its accounts in the IDL's order and with its flags.
   */
  riscZeroVerifierPausedIx(verifierRouter: PublicKey, proofSelector: Uint8Array): TransactionInstruction {
    const name = "risc_zero_verifier_paused";
    const accounts: Record<string, PublicKey> = {
      pa_state: this.paState(),
      verifier_entry: deriveVerifierRouterPdas(verifierRouter, proofSelector).entry,
    };
    const keys = idl.instructions
      .find((i) => i.name === name)!
      .accounts.map((a) => {
        const pubkey = accounts[a.name];
        if ("accounts" in a || pubkey === undefined) throw new Error(`${name} takes an account ${a.name} not given`);
        return { pubkey, isSigner: a.signer ?? false, isWritable: a.writable ?? false };
      });
    return new TransactionInstruction({ programId: this.programId, keys, data: coder.instruction.encode(name, {}) });
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
