// The adapter's deployments, as `rust/deployments.json` records them (the
// package's `copy-data` script copies it into `src/data/`): per environment,
// the program on each cluster, under the cluster's CAIP-2 chain id.

import { address, type Address } from "@solana/kit";

import recordJson from "./data/deployments.json" with { type: "json" };

/** The deployment environment of a recorded adapter: staging on devnet, production on mainnet-beta. */
export type Environment = "staging" | "production";

/** A recorded adapter deployment. */
export interface Deployment {
  /** The cluster's CAIP-2 chain id. */
  chainId: string;
  /** The adapter program. */
  programId: Address;
  /** The slot of the program's first deployment, where its history starts. */
  deploymentSlot: number;
}

/** A deployment record, as `deployments.json` holds one. A `null` slot is a program not deployed yet. */
export type DeploymentRecord = Record<
  Environment,
  { chainId: string; programId: string; deploymentSlot: number | null }[]
>;

const RECORD: DeploymentRecord = recordJson;

/** The CAIP-2 chain id of the cluster whose genesis hash is `genesisHash`: `solana:` and its first 32 characters (CAIP-30). */
export function chainIdOfGenesisHash(genesisHash: string): string {
  return `solana:${genesisHash.slice(0, 32)}`;
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
  return { chainId, programId: address(entry.programId), deploymentSlot: entry.deploymentSlot };
}
