// Why an instance could not be made or could not answer, as pa-evm's
// bindings' `error` module and the Rust crate's `error` name it.

import type { Address, TransactionError } from "@solana/kit";

import type { ClusterType } from "./addresses.js";
import { toJson } from "./json.js";

/** Why an instance could not be made or could not answer. */
export type BindingsErrorDetail =
  /** The genesis hash the RPC transport returned is no known cluster's. */
  | { kind: "ChainIdUnknown" }
  /** An RPC request failed: retrying may succeed. */
  | { kind: "RpcTransportError"; what: string; error: string }
  /** The environment records no deployed adapter on the cluster. */
  | { kind: "UnsupportedChain"; cluster: ClusterType }
  /** An account the answer is read from does not exist. */
  | { kind: "AccountMissing"; address: Address }
  /** What the chain holds does not decode. */
  | { kind: "Decode"; what: string; error: string }
  /** A simulated instruction failed. */
  | { kind: "Simulation"; error: TransactionError; logs: readonly string[] };

function describe(detail: BindingsErrorDetail): string {
  switch (detail.kind) {
    case "ChainIdUnknown":
      return "The genesis hash returned by the RPC transport is not in the list of known clusters.";
    case "RpcTransportError":
      return `The RPC transport returned an error fetching ${detail.what}: ${detail.error}`;
    case "UnsupportedChain":
      return `The current protocol adapter version has not been deployed on the provided chain '${detail.cluster}'.`;
    case "AccountMissing":
      return `the account ${detail.address} does not exist`;
    case "Decode":
      return `${detail.what} does not decode: ${detail.error}`;
    case "Simulation":
      return `the simulation failed: ${toJson(detail.error)}; logs: ${toJson(detail.logs)}`;
  }
}

export class BindingsError extends Error {
  constructor(
    readonly detail: BindingsErrorDetail,
    options?: ErrorOptions,
  ) {
    super(describe(detail), options);
    this.name = "BindingsError";
  }
}
