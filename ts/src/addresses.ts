// The adapter's deployments, as `rust/deployments.json` records them (the
// package's `copy-data` script copies it into `src/data/`): per environment,
// the program on each cluster, under the cluster's CAIP-2 chain id (`solana:`
// and the first 32 characters of its genesis hash in base58, CAIP-30).

import { address, type Address } from "@solana/kit";

import recordJson from "./data/deployments.json" with { type: "json" };

/** The deployment environment of a recorded protocol adapter program: staging on devnet, production on mainnet-beta. */
export type Environment = "staging" | "production";

/** A cluster with a known genesis hash, as `solana-cluster-type`'s `ClusterType` names it. */
export type ClusterType = "mainnet-beta" | "testnet" | "devnet";

/** Each cluster's genesis hash, as `solana-cluster-type`'s `ClusterType::get_genesis_hash` gives it. */
const GENESIS_HASHES: Record<ClusterType, string> = {
  "mainnet-beta": "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d",
  testnet: "4uhcVJyU9pJkvQyS88uRDiswHXSCkY3zQawwpjk2NsNY",
  devnet: "EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG",
};

const CLUSTERS = Object.keys(GENESIS_HASHES) as ClusterType[];

/** The cluster whose genesis hash is `genesisHash`, if any. */
export function clusterOfGenesisHash(genesisHash: string): ClusterType | undefined {
  return CLUSTERS.find((cluster) => GENESIS_HASHES[cluster] === genesisHash);
}

/** The CAIP-2 chain id of `cluster`. */
export function chainId(cluster: ClusterType): string {
  return `solana:${GENESIS_HASHES[cluster].slice(0, 32)}`;
}

/** A deployment record, as `deployments.json` holds one. A `null` slot is a program not deployed yet. */
export type DeploymentRecord = Record<
  Environment,
  { chainId: string; programId: string; deploymentSlot: number | null }[]
>;

/**
 * The programs `record` records for each environment, by cluster: the
 * entries with a deployment slot. An entry whose chain id is no known
 * cluster's, whose program id does not parse, or whose cluster another entry
 * records too throws, naming it.
 */
export function deploymentsMaps(record: DeploymentRecord): Record<Environment, Map<ClusterType, Address>> {
  const toMap = (environment: Environment): Map<ClusterType, Address> => {
    const seen = new Set<ClusterType>();
    const map = new Map<ClusterType, Address>();
    for (const entry of record[environment]) {
      const named = `deployments.json: ${environment} entry ${JSON.stringify(entry)}`;
      const cluster = CLUSTERS.find((c) => chainId(c) === entry.chainId);
      if (cluster === undefined) throw new Error(`${named}: chain id ${entry.chainId} is no known cluster's`);
      if (seen.has(cluster)) throw new Error(`${named}: ${cluster} is recorded twice`);
      seen.add(cluster);
      let program: Address;
      try {
        program = address(entry.programId);
      } catch (error) {
        throw new Error(`${named}: program id: ${String(error)}`, { cause: error });
      }
      if (entry.deploymentSlot !== null) map.set(cluster, program);
    }
    return map;
  };
  return { staging: toMap("staging"), production: toMap("production") };
}

const DEPLOYMENTS = deploymentsMaps(recordJson);

/** Returns a map of the protocol adapter programs recorded for the environment. */
export function protocolAdapterDeploymentsMap(environment: Environment): Map<ClusterType, Address> {
  return new Map(DEPLOYMENTS[environment]);
}

/** Returns the protocol adapter program recorded for the environment on the provided cluster, if any. */
export function protocolAdapterAddress(environment: Environment, cluster: ClusterType): Address | undefined {
  return DEPLOYMENTS[environment].get(cluster);
}
