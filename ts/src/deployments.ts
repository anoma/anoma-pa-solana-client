// The adapter's deployments, as `rust/deployments.json` records them (the
// package's `copy-data` script copies it into `src/data/`): per environment,
// the program on each cluster, under the cluster's CAIP-2 chain id.

import { PublicKey, type Cluster } from "@solana/web3.js";

import recordJson from "./data/deployments.json" with { type: "json" };

/** The deployment environment of a recorded adapter: staging on devnet, production on mainnet-beta. */
export type Environment = "staging" | "production";

/** A recorded adapter deployment. */
export interface Deployment {
  /** The cluster's CAIP-2 chain id. */
  chainId: string;
  /** The adapter program. */
  programId: PublicKey;
  /** The slot of the program's first deployment, where its history starts. */
  deploymentSlot: number;
}

/** A deployment record, as `deployments.json` holds one. A `null` slot is a program not deployed yet. */
export type DeploymentRecord = Record<
  Environment,
  { chainId: string; programId: string; deploymentSlot: number | null }[]
>;

const RECORD: DeploymentRecord = recordJson;

/** The genesis hash of each public cluster, as `solana-cluster-type` records them for the Rust crate. */
const GENESIS_HASHES: Record<Cluster, string> = {
  "mainnet-beta": "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d",
  devnet: "EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG",
  testnet: "4uhcVJyU9pJkvQyS88uRDiswHXSCkY3zQawwpjk2NsNY",
};

/** The CAIP-2 chain id of `cluster`: `solana:` and the first 32 characters of its genesis hash (CAIP-30). */
export function chainId(cluster: Cluster): string {
  return `solana:${GENESIS_HASHES[cluster].slice(0, 32)}`;
}

/** The public cluster whose genesis hash is `genesisHash`, if any. */
export function cluster(genesisHash: string): Cluster | undefined {
  return (Object.keys(GENESIS_HASHES) as Cluster[]).find((c) => GENESIS_HASHES[c] === genesisHash);
}

/**
 * The adapter `environment` records on the cluster `chainId` names, or
 * `undefined` when it records none there or records it as not yet deployed.
 */
export function deployment(
  environment: Environment,
  chainId: string,
  record: DeploymentRecord = RECORD,
): Deployment | undefined {
  const entry = record[environment].find((e) => e.chainId === chainId);
  if (entry === undefined || entry.deploymentSlot === null) return undefined;
  return { chainId, programId: new PublicKey(entry.programId), deploymentSlot: entry.deploymentSlot };
}
