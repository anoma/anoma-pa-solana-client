import { describe, expect, it } from "vitest";

import {
  chainId,
  clusterOfGenesisHash,
  deploymentsMaps,
  protocolAdapterAddress,
  protocolAdapterDeploymentsMap,
  type DeploymentRecord,
} from "./addresses.js";
import { idl } from "./idl.js";

const DEVNET = "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1";
const MAINNET = "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp";

describe("the clusters", () => {
  it("have the chain ids CAIP-30 gives them", () => {
    for (const [cluster, expected] of [
      ["mainnet-beta", MAINNET],
      ["devnet", DEVNET],
      ["testnet", "solana:4uhcVJyU9pJkvQyS88uRDiswHXSCkY3z"],
    ] as const) {
      console.log(`${cluster}: ${chainId(cluster)}`);
      expect(chainId(cluster), cluster).toBe(expected);
    }
  });

  it("are found by their genesis hashes", () => {
    expect(clusterOfGenesisHash("EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG")).toBe("devnet");
    expect(clusterOfGenesisHash("5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d")).toBe("mainnet-beta");
    expect(clusterOfGenesisHash("11111111111111111111111111111111")).toBeUndefined();
  });
});

describe("the deployment record", () => {
  // One entry in each environment: staging's on devnet with the slot
  // `stagingSlot`, production's on mainnet-beta with slot 9.
  const record = (stagingSlot: number | null): DeploymentRecord => ({
    staging: [{ chainId: DEVNET, programId: "2gH5DnTwDLLFk3oYxnbrceSFdTMtvaxQz29x5W8Etmpx", deploymentSlot: stagingSlot }],
    production: [{ chainId: MAINNET, programId: "11111111111111111111111111111111", deploymentSlot: 9 }],
  });

  it("maps each environment's entry to its cluster", () => {
    const maps = deploymentsMaps(record(7));
    console.log(maps);
    expect([...maps.staging]).toEqual([["devnet", "2gH5DnTwDLLFk3oYxnbrceSFdTMtvaxQz29x5W8Etmpx"]]);
    expect([...maps.production]).toEqual([["mainnet-beta", "11111111111111111111111111111111"]]);
  });

  it("holds no deployment for an entry whose program is not deployed yet (a null slot)", () => {
    expect(deploymentsMaps(record(null)).staging.size).toBe(0);
  });

  it("refuses a chain id of no known cluster, naming the entry", () => {
    expect(() =>
      deploymentsMaps({ staging: [{ chainId: "solana:a", programId: "11111111111111111111111111111111", deploymentSlot: 1 }], production: [] }),
    ).toThrow(/staging entry .*"solana:a".*: chain id solana:a is no known cluster's/);
  });

  it("refuses a cluster recorded twice", () => {
    const entry = { chainId: DEVNET, programId: "11111111111111111111111111111111", deploymentSlot: 1 };
    expect(() => deploymentsMaps({ staging: [entry, { ...entry, deploymentSlot: null }], production: [] })).toThrow(
      /devnet is recorded twice/,
    );
  });

  it("refuses a program id that does not parse", () => {
    expect(() =>
      deploymentsMaps({ staging: [{ chainId: DEVNET, programId: "not a key", deploymentSlot: null }], production: [] }),
    ).toThrow(/program id: /);
  });

  // The client decodes with the IDL of the program staging runs.
  it("records the vendored IDL's address as staging's devnet program", () => {
    const devnet = protocolAdapterAddress("staging", "devnet");
    console.log(`staging devnet: ${devnet}, IDL: ${idl.address}`);
    expect(devnet).toBe(idl.address);
    expect(protocolAdapterDeploymentsMap("staging").get("devnet")).toBe(idl.address);
  });
});
