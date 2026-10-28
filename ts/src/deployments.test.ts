import { PublicKey } from "@solana/web3.js";
import { describe, expect, it } from "vitest";

import record from "./data/deployments.json" with { type: "json" };
import { chainIdOfGenesisHash, deployment, type DeploymentRecord, type Environment } from "./deployments.js";
import { idl } from "./idl.js";

const ENVIRONMENTS: Environment[] = ["staging", "production"];
const raw = record as DeploymentRecord;

describe("the deployment record", () => {
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
    const devnet = raw.staging.filter((e) => e.chainId === "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1");
    expect(devnet.length, JSON.stringify(devnet)).toBe(1);
    console.log(`staging devnet: ${devnet[0]!.programId}, IDL: ${idl.address}`);
    expect(devnet[0]!.programId).toBe(idl.address);
  });
});

describe("chainIdOfGenesisHash", () => {
  it("is solana: and the hash's first 32 characters (CAIP-30)", () => {
    const hash = "EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG";
    console.log(`${hash}: ${chainIdOfGenesisHash(hash)}`);
    expect(chainIdOfGenesisHash(hash)).toBe("solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1");
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
