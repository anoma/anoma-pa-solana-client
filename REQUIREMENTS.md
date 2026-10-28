# `anoma-pa-solana-client` — Requirements

This document specifies what the `anoma-pa-solana-client` package must provide. Implementation follows from this spec.

The repository ships **two language packages from one source of truth**: a Rust crate (under `rust/`) and a TypeScript/npm package (under `ts/`). Both carry the wire-level core — discriminators, PDA helpers, account and event decoders, and constants — adapted to each language's idioms. Both build every adapter instruction (§3.3); the settlement orchestration (§3.4) is in the Rust crate, and the TypeScript package adds the frontend's confirmation helper. They are released together with matching versions.

---

## 1. Purpose

The AnomaPay Solana stack has three integrators: the backend (Rust), the frontend (TypeScript), and the indexer (Elixir, currently; consumes JSON over HTTP from a Solana indexing service). Each integrator must call the Solana Protocol Adapter (PA), build correctly-shaped settle transactions, decode PA state, and decode Anchor events. The wire-level details — Anchor discriminators, PDA seeds, Borsh field layouts, instruction account orderings, compute-budget minimums — are owned by the PA and change with PA releases.

Without a published bindings package, each integrator reimplements these details by hand. Every PA release becomes a cross-repo patching exercise, with at least one integrator missing the update each time. This package eliminates the duplication: PA-side primitives live here, versioned in lockstep with the PA, and every integrator imports them.

This is the Solana analog of the existing EVM bindings `anoma-pa-evm-bindings`. The SPL token forwarder's bindings, the analog of `anomapay-erc20-forwarder-bindings`, are in [anoma/anomapay-spl-token-forwarder](https://github.com/anoma/anomapay-spl-token-forwarder) (`crates/client` and `ts/`).

---

## 2. Audience and integration

| Integrator | How it depends on this package |
|------------|-------------------------------|
| `anomapay-backend` (Rust) | Adds the Rust crate as a Cargo dependency. Uses instruction builders, PA state decoder, settle-transaction orchestration. |
| `pay-interface-app` (TypeScript) | Adds the npm package as a dependency. Uses the PA state decoder. |
| `galileo-indexer` (Elixir) | Consumes the IDL files (`rust/idls/*.json`) for Anchor event decoding via whichever indexing service it uses. Does not link the language packages directly. |

Each integrator must be able to point its build at a specific tagged release. Major-version bumps signal PA-incompatible changes.

---

## 3. Functional surface

The package must expose, in both Rust and TypeScript, the items below. Names are illustrative; final naming follows each language's conventions.

### 3.1 Generated bindings

- In Rust, Anchor's `declare_program!` generates `generated::protocol_adapter` from `rust/idls/protocol_adapter.json`, and `generated::verifier_router` from `rust/idls/verifier_router.json`: the program id (`ID`), the account types (`accounts::PAStateAccount`, `accounts::TxDataAccount`), the event types (`events::*`), the defined types (`types::*`, `U256` as its 32 little-endian bytes), the constants (`constants::*`, the PDA seeds among them), the error enum (`error::ProtocolAdapterError`, whose `u32` is the custom program error code a failed transaction reports; `build.rs` generates its `TryFrom<u32>` from the IDL's errors), and every instruction's accounts and arguments (`client::accounts`, `client::args`). Each instruction, account and event type carries its discriminator (`DISCRIMINATOR`), read from the IDL.
- In TypeScript, `@anchor-lang/core`'s `BorshCoder` over the IDL the package ships (`idl`, a copy of `rust/idls/protocol_adapter.json`, §3.12): `coder` encodes and decodes the adapter's accounts, events, instructions and types, each with its IDL discriminator; `idl.address` is the adapter's address and `idlConstant` reads the IDL's constants.
- The program id is the one the IDL records; the instruction builders take the address of the deployment they address, since a build sets it (`declare_id!` from the environment).

### 3.2 Anchor discriminators

- 8-byte instruction discriminators for every PA instruction the integrator may call.
- 8-byte event discriminators for every event the indexer must decode: the first 8 bytes of `sha256("event:" + signature)`, the signature being the event's name and its field types in order, as the IDL records them.
- Read from the IDL; not hand-typed constants.

### 3.3 Instruction builders — Protocol Adapter

- `initialize` (the deployer, as the program's upgrade authority, sets the owner, verifier router and proof selector)
- `txdata_init`
- `txdata_write` (chunked; chunk size constant is shipped from this package)
- `settle` (the settlement input in the instruction itself)
- `settle_from_txdata`
- `simulate_execute` (pa-evm's `simulateExecute`: `settle`'s accounts and input, and whether to skip verifying the RISC Zero proof; it always fails, with `Simulated` when the settlement would execute)
- `txdata_close`

Each builder takes typed inputs and returns a fully-formed Solana `Instruction` value (Rust) or `@solana/kit` `Instruction` (TypeScript) with the correct accounts, discriminator, and serialized arguments. In Rust each is a thin function over the generated `client::accounts` and `client::args`, taking the adapter's address and deriving every account that address and the instruction's own arguments determine (the state, the upload and the event accounts); the settlement builders take the accounts every settlement names as `SettlementAccounts`.

### 3.4 Settle transaction orchestration

- Every transaction is a v1 transaction (SIMD-0385: up to 4,096 bytes, no lookup tables, the compute budget in the message header). Each planned transaction (`PlannedTransaction`) carries its instructions and its settings, and compiles its own message; the settings state the loaded-account-data limit, which a v1 transaction does not default.
- `plan_settlement` plans one settlement from its input, the adapter's state, the resources it touches, the external calls' account segments and the instructions that precede the adapter's (a forwarder's signature checks and account creations). When the settlement fits one transaction (`transaction_size` at most 4,096 bytes), it is that transaction: the preceding instructions, then `settle` with the input. Otherwise its input is uploaded (`txdata_init`, N × `txdata_write`, and `txdata_close` after the settlement) and the settlement transaction is the preceding instructions then `settle_from_txdata`. `plan_simulation` plans the transaction simulating the same settlement: the one settlement transaction with `simulate_execute` in place of `settle`, for the RPC's simulation, which changes nothing and needs no signature; a simulation too large for one transaction is refused, since it is never uploaded. The settlement's remaining accounts are the nullifier PDAs, then the external-call segments, then the root marker of every consumed root other than the padding leaf; the new root is predicted from the adapter's stored frontier. A settlement whose transaction names more accounts than a transaction locks (64), or is larger than 4,096 bytes even with its input uploaded, is refused.
- The settlement input (the `arm` feature): a proven ARM `Transaction` with its aggregation proof re-encoded as the verifier-router seal, bincode-serialized (`settlement_input`); the input of `simulate_execute` (`simulation_input`): a proven transaction as `settlement_input` encodes it, an unproven one (its aggregation proof empty) with the adapter's proof selector as its proof; the nullifiers, consumed roots and created commitments its settlement touches (`settled_resources`), and the external calls it runs, in the adapter's order (`external_calls`), whose CPI accounts the submitter supplies.
- Submission (the `rpc` feature), the same for every application. Each forwarder's own client implements `Forwarder`: given all of a settlement's calls to its program and the index in the settlement transaction where its preceding instructions start, it returns `CallAccounts`, each call's CPI segment and the instructions that must precede the settlement (a call that names the absolute index of one, such as a wrap's signature check, names it from that start). A submitter registers the forwarders it supports in `Forwarders`, which asks each forwarder once per settlement, in the order of its first call, its preceding instructions after those of the forwarders before it, and checks each segment against the account count the proof commits and against the forwarder program that leads it. `Submission::prepare` takes the adapter's state (`read_adapter_state`) and the verifier program the router names for the adapter's selector (`read_verifier_program`), both read over a nonblocking `solana-rpc-client` (a missing account is its own error, not a failed request), and gathers the calls' accounts from the forwarders. `Submission::plan_settlement` plans the settlement; `Submission::simulate` plans its simulation (`plan_simulation`), runs it unsigned at the RPC client's commitment, and reports how it ended: executable (the adapter's `Simulated`), failed (`Failure`), or otherwise. `settlement_failure` reads the `Failure` from the runtime's log of a settlement, a simulation's or a send's preflight, from the first failure logged: the adapter's own error, the verifier refusing the proof, or another program's custom error. A truncated log is an error, since its first failure line can then belong to another program.

### 3.5 PDA derivation

- `derive_pa_state_pda()`, `derive_tx_data_pda(authority, upload_id)`, `derive_upgrade_authority_pda()` — from the seeds the IDL declares (`PA_STATE_SEED`, `TX_DATA_SEED`, `UPGRADE_AUTHORITY_SEED`); the generated client derives no PDA. A test checks each against the IDL's `pda.seeds` of an account that uses it.
- `derive_nullifier_pda(pa_state, nullifier_bytes)`, `derive_root_marker_pda(pa_state, root_bytes)` — the remaining accounts of a settlement, from the IDL's `NULLIFIER_SEED` and `ROOT_SEED`
- `derive_event_authority_pda(program)` — Anchor's `["__event_authority"]`
- `derive_verifier_router_pdas(router_program, selector)` — the router's state PDA and its verifier entry for the adapter's proof selector; `derive_verifier_entry_pda(router_program, selector)` — the entry alone
- `derive_program_data_address(program)` — the upgradeable loader's ProgramData account, which `initialize` checks
- The TypeScript package derives the PDAs the IDL gives no seeds for: `deriveNullifierPda` and `deriveRootMarkerPda` (from the IDL's `NULLIFIER_SEED` and `ROOT_SEED`), `deriveEventAuthorityPda` and `deriveVerifierRouterPdas`.

### 3.6 Account decoders

- `decode_pa_state(bytes) -> PAStateAccount` — the adapter's state account. In Rust, the generated `PAStateAccount` read with its discriminator checked, after refusing a schema version other than the IDL's `SCHEMA_VERSION` (the adapter refuses every instruction on such an account). In TypeScript, `coder.accounts.decode("PAStateAccount", data)`, which checks the discriminator; the instance (§3.13) also refuses another schema version.
- The verifier router's `VerifierEntry` (`selector: [u8; 4]`, `verifier`, `estopped: bool`): the verifier program a settlement passes for the adapter's selector. In Rust, the generated `verifier_router::accounts::VerifierEntry`, from the router IDL the adapter repo commits (`solana-pa-prototype/idls/verifier_router.json`).
- The schema is sourced from the PA repository at the commit tagged by this package's version. When the PA's struct changes, the new struct shape ships in the next release.

### 3.7 Event decoders

- Every Anchor event emitted by the PA, as the IDL declares it:
  - `ResourcePayload`, `DiscoveryPayload`, `ExternalPayload`, `ApplicationPayload` — `{ tag: [u8; 32], index: u256, blob: Vec<u8> }`; the index is `U256LeBytes`, its 32 little-endian bytes, in Rust and the coder's `bigint` in TypeScript
  - `ActionExecuted { action_tree_root, nullifiers, consumed_logic_refs, commitments, created_logic_refs }`
  - `TransactionExecuted { transaction_id: [u8; 32] }` — Keccak-256 of the concatenated action tree roots
  - `ForwarderCallExecuted { untrusted_forwarder, input, output }`
  - `CommitmentTreeRootAdded { root }`, `KindTreeRootUpdated { kind_tree_root }`, `LogicRefDenied { logic_ref, consumed }`
  - `Paused { account }`, `Unpaused { account }`
  - `OwnershipTransferred { previous_owner, new_owner }`, `Initialized { version: u64 }`, `Upgraded { implementation_hash: [u8; 32] }`
- The PA emits every event as a self-invocation (Anchor `#[event_cpi]`), never in the program log. In Rust, the event types are the generated ones, and the framing is hand-written, with no list of events: `cpi_event_discriminator` takes one such inner instruction's data (the 8-byte event tag, the discriminator, the Borsh body) and returns its discriminator, which a caller matches against the generated types' `DISCRIMINATOR`; `decode_cpi_event::<E>` returns the event `E`, or `None` when the data holds another event, and refuses data without the tag, a body that is not one `E`, and trailing bytes. In TypeScript, `decodeCpiEvent(data)` checks the tag (`EVENT_IX_TAG`) and decodes the rest with the coder, returning the event's IDL name and its fields under their IDL names; it refuses (`EventDecodeError`) data without the tag, a discriminator no IDL event has, a body that does not decode or ends short, and bytes after the body.
- The indexer consumes the IDL file (`rust/idls/protocol_adapter.json`) for the same decoding in non-Rust/TS contexts.

### 3.8 Errors

- Every error the PA returns, by the custom program error code a failed transaction reports (Anchor's `6000` plus its position in the program's `PAError`): the generated `ProtocolAdapterError` in Rust, with `TryFrom<u32>` and `u32::from` for its code, and the IDL's `errors` in TypeScript (`idl.errors`, which `@anchor-lang/core`'s `parseIdlErrors` and `translateError` read). A test checks the Rust mapping against the IDL's names and codes, so a caller matches errors by type, never by name or log text.

### 3.9 External call construction

- `SolanaExternalCall { program_id: [u8; 32], instruction_data: Vec<u8>, expected_output: Vec<u8>, output_mode: OutputMode, num_accounts: u8 }` — exact match for the PA's `solana-pa-prototype/src/types.rs`.
- `OutputMode = ReturnData`.

### 3.10 Commitment tree primitives

- `hash_two(left: [u8; 32], right: [u8; 32]) -> [u8; 32]` — SHA-256 over the concatenation, matching the PA's `hash_two`.
- `padding_leaf() -> [u8; 32]` and the zero-hash sequence `[H_0, H_1, ..., H_31]` precomputed.
- `CommitmentTreeState` type wrapping `{ root, next_index, current_depth, frontier }` with an `append(commitment)` method that updates the state and returns the new root. Used by backend to derive the new root-marker PDA for a settlement.

### 3.11 Constants

- `SETTLE_COMPUTE_UNIT_LIMIT` — the compute units the settlement transaction requests (currently 500,000). Source of truth.
- `SETTLE_HEAP_FRAME_BYTES` — the heap frame the settlement transaction requests (currently 262,144). Source of truth.
- `UPLOAD_COMPUTE_UNIT_LIMIT` — the compute units an upload's other transactions request (currently 200,000, the runtime's default for one instruction).
- `MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES` — the account data every transaction may load (64 MiB, the runtime's maximum).
- `TXDATA_CHUNK_SIZE` — the chunk size for `txdata_write`: the largest whose write transaction fits a v1 transaction (currently 3,856).
- These must change here when they change in the PA. The integrator imports them; bumping the PA without bumping this package is a release-process error.

### 3.12 IDL files

- `rust/idls/protocol_adapter.json` — Anchor IDL for the PA program at this version, and `rust/idls/verifier_router.json`, the verifier router's: the files committed at the adapter tag in `PA_TAG.txt`, byte for byte, which a test checks. `declare_program!` reads them from the `idls/` directory beside the crate's manifest.
- Copied from the PA repo on every release. The integrator's indexer (Elixir or any other non-Rust/TS consumer) reads it directly.
- The TS package's `copy-data` script copies `protocol_adapter.json` and `rust/deployments.json` into `ts/src/data/` (git-ignored) on `npm install`, `npm pack` and before every build, type check and test, so the package ships, and its tests read, the files the Rust crate is built from.

### 3.13 Deployments and the adapter instance

- `rust/deployments.json` records, per environment (`staging` on devnet, `production` on mainnet-beta), the adapter program on each cluster under its CAIP-2 chain id (`solana:` and the first 32 characters of the genesis hash, CAIP-30), with the slot of its first deployment, `null` until it is deployed. `deployment(environment, chain_id)` reads it; `chain_id_of_genesis_hash(genesis_hash)` (TypeScript: `chainIdOfGenesisHash`) computes a cluster's chain id from its genesis hash, and in Rust `chain_id(cluster)` gives a `solana-cluster-type` cluster's, which the record's tests check every entry against.
- `protocol_adapter(rpc, environment)` (TypeScript: `protocolAdapter(rpc, environment)`, over a `@solana/kit` `Rpc`) is pa-evm's `contract::protocol_adapter`: the adapter the environment records on the cluster behind the RPC, found by the chain id of its genesis hash, or `NoDeployment` with that chain id when the environment records no deployed adapter there. `ProtocolAdapter::at` (TypeScript: `ProtocolAdapter.at`) is the adapter at a given address. Its reads carry pa-evm's names (`paused`, `riscZeroVerifierPaused`, `RISC_ZERO_VERIFIER_ROUTER`, `RISC_ZERO_VERIFIER_SELECTOR`, `commitmentCount`, `commitmentTreeDepth`, `commitmentTreeCapacity`, `commitmentTreeLeftSiblings`, `commitmentTreeEmptySubtreeRoots`, `latestCommitmentTreeRoot`, `isCommitmentTreeRootHistorical`, `isNullifierContained`, `getKindTreeRoot`, `EMPTY_KIND_TREE_ROOT`, `isLogicRefDenied`, `owner`), in snake case in Rust and camel case in TypeScript, and fail with `InstanceError`. In TypeScript the u64 reads (`commitmentCount`, `commitmentTreeCapacity`), the builders' u64 arguments and the integers of the events `decodeCpiEvent` returns are `bigint`, and every address is a `@solana/kit` `Address`. In both languages it builds the adapter's instructions at its address (`initialize_ix` / `initializeIx` and so on): in TypeScript, each through `coder.instruction.encode` with its accounts in the IDL's order and with the IDL's flags, and both packages check their builders against `fixtures/instructions_fixture.json`, the bytes and accounts the Rust crate's builders produce for fixed inputs.

---

## 4. Non-functional requirements

### 4.1 Versioning

- SemVer. Major version bump on any wire-incompatible PA change.
- Rust crate version and TS package version must match exactly per release.
- Git tags follow `bindings/vMAJOR.MINOR.PATCH`; consumers pin the tag. Neither package is published to a registry.

### 4.2 Source-of-truth coupling to the PA

- This package is paired with a specific PA release. Its tag is recorded in `PA_TAG.txt`, and the adapter IDL is the one committed at that tag.
- A PA security or feature release that changes anything in §3 triggers a new release of this package against the new PA commit.
- The package does not "track main" of the PA — releases are explicit, tagged, and reviewed.

### 4.3 Generation vs hand-writing

- Anything Anchor can generate (instruction discriminators, account decoders, IDL-derived types) should be generated, not hand-typed.
- Hand-written code is acceptable for: orchestration (settle-tx assembly), helpers, and constants the IDL doesn't express.
- All hand-written code that mirrors on-chain shapes must be round-trip-tested against the on-chain serializer.

### 4.4 Testing

- Unit tests for every PDA derivation, every account decoder, every instruction builder.
- Integration tests: a local validator with the PA deployed, settling transactions end-to-end using this package's helpers. The test must fail loudly if any wire-level disagreement exists.
- CI runs Rust and TS tests against the same pinned PA commit on every push.

### 4.5 Documentation

- Each exported function has a doc comment / TSDoc explaining its purpose and the PA-side invariant it enforces.
- A `CHANGELOG.md` per release noting which PA changes drove this release.

---

## 5. Out of scope

The following are **not** in this package, regardless of how convenient they might be to colocate:

- **Proof generation.** Routing to the AnomaPay workers queue is the backend's concern.
- **Sending and confirming transactions.** Signing, blockhash-expiry-aware confirmation polling and retry logic are the integrator's. The `rpc` feature only reads accounts and simulates, through `solana-rpc-client`; an integrator with another RPC client uses the planners without it.
- **Wallet integration.** `@solana/wallet-adapter` use, Phantom-specific behavior, wallet UI — frontend concern.
- **Indexer sync mechanics.** Cursor management, rate-limit handling, hosted-indexer choice — indexer's concern. This package only supplies the IDL.
- **Merkle tree replay.** The backend replays the PA's commitment tree to derive the next root-marker PDA; this package supplies `hash_two`, `padding_leaf`, and `CommitmentTreeState` as primitives, but the *use* of those primitives (when to replay, how to coordinate concurrent submissions) belongs to the backend.

---

## 6. Release process

1. **Trigger.** A PA release (new tag in `solana-protocol-adapter`) that changes any item in §3.
2. **Pin update.** Update this repo's `PA_TAG.txt` to the new tag.
3. **Regenerate.** Copy the adapter's IDL at that tag to `rust/idls/protocol_adapter.json`; `declare_program!` regenerates the Rust bindings from it. Hand-written items are reviewed against the new PA source.
4. **Test.** Full CI pass: unit, round-trip, and integration tests against the new PA commit.
5. **Tag.** Cut a SemVer release; major bump for wire-incompatible changes.
6. **Notify integrators.** Backend, frontend, and indexer maintainers receive a release notification with the changelog and a summary of which fields/values changed.

Integrators bump their dependency at their own pace, but the package's release is the single coordination event — they no longer have to chase a multi-repo cross-team patch.

---

## 7. Open questions

These need resolution before implementation begins.

1. **Maintainer.** Does the PA team own this package directly, or is it shared with the integrator teams? Recommend: PA team owns it (they have the source of truth for every contract in §3); integrators contribute via PRs for helpers they need that aren't yet exposed.
2. **Auto-generation pipeline.** Anchor's IDL emission gives us instruction discriminators and some types, but the cursor-based `PAStateAccount` decoder needs either custom generators or hand-written code. Decision: which items in §3 are codegen targets vs hand-written?
3. **Workspace organization.** Should `rust/` be a single crate or a Cargo workspace with sub-crates (`pa-instructions`, `pa-accounts`, etc.)? Single crate is simpler; workspace allows the indexer to depend on only the event-decoding subset. Recommend: start as a single crate; split later if a real consumer benefits.
4. **TS package name.** Resolved: `@anomaorg/pa-solana-client`, the scope the organization publishes under (`@anomaorg/anoma-app-sdk`, `@anomaorg/arm-bindings`).
5. **Anchor version compatibility.** Different consumers may want different Anchor versions in their toolchain. Decision: pin to one Anchor version per release, or support multiple?
6. **Solana SDK major version.** `solana-sdk` 2.x is current; will the package support 1.x backports?
7. **CI for cross-package consistency.** A test that builds the Rust crate and the TS package against the same PA fixture and confirms every shared constant matches byte-for-byte. Worth setting up as part of v0.1.
8. **License.** Resolved: MIT, as pa-evm's bindings; the crate and the npm package each ship the license text (`rust/LICENSE`, `ts/LICENSE`).

---

## 8. Initial scope for v0.1

Smallest useful version. Ship these and integrators can start adopting:

- Program ID constant for the PA (devnet default).
- Instruction discriminators for the four PA instructions.
- PDA derivation helpers for the PDAs in §3.5.
- `decode_pa_state` cursor-based decoder.
- `SolanaExternalCall` type + `OutputMode` enum.
- Constants in §3.11.
- IDL files in `rust/idls/`.
- Basic README pointing at this REQUIREMENTS.md.

Items deferred to v0.2+: full settle-tx orchestrator, commitment-tree replay primitives, full event-decoder set with log-line dispatch.

---

## 9. Acceptance criteria for v0.1

- `anomapay-backend` can replace its inline PA-instruction-builder and PDA-derivation code with imports from this crate, with no behavior change.
- `pay-interface-app` can replace its inline PA state decoder with imports from this package.
- Both packages produce byte-identical output to the existing inline implementations on a fixed test vector.
- All §4.4 tests pass in CI against the pinned PA commit.
