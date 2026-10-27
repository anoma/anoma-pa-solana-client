import { PublicKey, type Cluster } from "@solana/web3.js";
import { describe, expect, it } from "vitest";

import record from "./data/deployments.json" with { type: "json" };
import { chainId, cluster, deployment, type DeploymentRecord, type Environment } from "./deployments.js";
import { idl } from "./idl.js";

const ENVIRONMENTS: Environment[] = ["staging", "production"];
const CLUSTERS: Cluster[] = ["mainnet-beta", "devnet", "testnet"];
const raw = record as DeploymentRecord;

/** The cluster `id` names, or a failure naming the id. */
function clusterOf(id: string): Cluster {
  const found = CLUSTERS.find((c) => chainId(c) === id);
  if (found === undefined) throw new Error(`chain id ${id} is no known cluster's`);
  return found;
}

describe("the deployment record", () => {
  it("names every chain by a known cluster's chain id", () => {
    for (const environment of ENVIRONMENTS) {
      for (const entry of raw[environment]) {
        console.log(`${environment}: ${entry.chainId} is ${clusterOf(entry.chainId)}`);
      }
    }
  });

  it("repeats no chain id within an environment", () => {
    for (const environment of ENVIRONMENTS) {
      const ids = raw[environment].map((e) => e.chainId);
      expect(new Set(ids).size, `${environment}: ${ids}`).toBe(ids.length);
    }
  });

  it("holds program ids that parse", () => {
    for (const environment of ENVIRONMENTS) {
      for (const entry of raw[environment]) {
        console.log(`${environment}: ${new PublicKey(entry.programId).toBase58()}`);
      }
    }
  });

  // The client decodes with the IDL of the program staging runs.
  it("records the vendored IDL's address as staging's devnet program", () => {
    const devnet = raw.staging.filter((e) => clusterOf(e.chainId) === "devnet");
    expect(devnet.length, JSON.stringify(devnet)).toBe(1);
    console.log(`staging devnet: ${devnet[0]!.programId}, IDL: ${idl.address}`);
    expect(devnet[0]!.programId).toBe(idl.address);
  });
});

describe("chain ids and clusters", () => {
  it("gives the public clusters the chain ids CAIP-30 gives them", () => {
    const expected: Record<Cluster, string> = {
      "mainnet-beta": "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp",
      devnet: "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1",
      testnet: "solana:4uhcVJyU9pJkvQyS88uRDiswHXSCkY3z",
    };
    for (const c of CLUSTERS) {
      console.log(`${c}: ${chainId(c)}`);
      expect(chainId(c), c).toBe(expected[c]);
    }
  });

  it("finds each cluster by its genesis hash, and none by another hash", () => {
    const genesis: Record<Cluster, string> = {
      "mainnet-beta": "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d",
      devnet: "EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG",
      testnet: "4uhcVJyU9pJkvQyS88uRDiswHXSCkY3zQawwpjk2NsNY",
    };
    for (const c of CLUSTERS) expect(cluster(genesis[c]), c).toBe(c);
    expect(cluster(PublicKey.default.toBase58())).toBeUndefined();
  });
});

describe("deployment", () => {
  // One entry in each environment: `stagingSlot` the devnet entry's slot,
  // mainnet-beta's 9.
  const record = (stagingSlot: number | null): DeploymentRecord => ({
    staging: [
      { chainId: "solana:a", programId: "2gH5DnTwDLLFk3oYxnbrceSFdTMtvaxQz29x5W8Etmpx", deploymentSlot: stagingSlot },
    ],
    production: [{ chainId: "solana:b", programId: "11111111111111111111111111111111", deploymentSlot: 9 }],
  });

  it("is the environment's entry on the chain", () => {
    const found = deployment("production", "solana:b", record(7));
    console.log(found);
    expect(found).toEqual({ chainId: "solana:b", programId: PublicKey.default, deploymentSlot: 9 });
    expect(deployment("staging", "solana:a", record(7))?.deploymentSlot).toBe(7);
    expect(deployment("staging", "solana:b", record(7))).toBeUndefined();
    expect(deployment("production", "solana:c", record(7))).toBeUndefined();
  });

  it("is none for an entry whose program is not deployed yet (a null slot)", () => {
    expect(deployment("staging", "solana:a", record(null))).toBeUndefined();
  });
});
