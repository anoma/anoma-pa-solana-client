# `anoma-pa-solana-client` — Requirements

This document specifies what the `anoma-pa-solana-client` package must provide. Implementation follows from this spec.

The repository ships **two language packages from one source of truth**: a Rust crate (under `rust/`) and a TypeScript/npm package (under `ts/`). They expose the same surface — instruction builders, account decoders, PDA helpers, event decoders, and constants — adapted to each language's idioms. They are released together with matching versions.

---

## 1. Purpose

The AnomaPay Solana stack has three integrators: the backend (Rust), the frontend (TypeScript), and the indexer (Elixir, currently; consumes JSON over HTTP from a Solana indexing service). Each integrator must call the Solana Protocol Adapter (PA) and the SPL Token Forwarder, build correctly-shaped settle transactions, decode PA state, and decode Anchor events. The wire-level details — Anchor discriminators, PDA seeds, Borsh field layouts, instruction account orderings, compute-budget minimums — are owned by the PA-side programs and change with PA releases.

Without a published bindings package, each integrator reimplements these details by hand. Every PA release becomes a cross-repo patching exercise, with at least one integrator missing the update each time. This package eliminates the duplication: PA-side primitives live here, versioned in lockstep with the PA, and every integrator imports them.

This is the Solana analog of the existing EVM bindings: `anoma-pa-evm-bindings` (PA) and `anomapay-erc20-forwarder-bindings` (forwarder). The Solana version intentionally combines both programs into one client package, because every AnomaPay flow that touches one touches the other.

---

## 2. Audience and integration

| Integrator | How it depends on this package |
|------------|-------------------------------|
| `anomapay-backend` (Rust) | Adds the Rust crate as a Cargo dependency. Uses instruction builders, PA state decoder, forwarder CPI account assembly, settle-transaction orchestration. |
| `pay-interface-app` (TypeScript) | Adds the npm package as a dependency. Uses wrap-message construction, `Approve`/ATA helpers, PA state decoder, forwarder PDA derivation. |
| `galileo-indexer` (Elixir) | Consumes the IDL files (`idl/*.json`) for Anchor event decoding via whichever indexing service it uses. Does not link the language packages directly. |

Each integrator must be able to point its build at a specific tagged release. Major-version bumps signal PA-incompatible changes.

---

## 3. Functional surface

The package must expose, in both Rust and TypeScript, the items below. Names are illustrative; final naming follows each language's conventions.

### 3.1 Program identifiers

- `PA_PROGRAM_ID` — the on-chain address of the Protocol Adapter.
- `FORWARDER_PROGRAM_ID` — the on-chain address of the SPL Token Forwarder.
- `ED25519_PROGRAM_ID` — Solana's native ed25519 verification program (`Ed25519SigVerify111111111111111111111111111`).
- These are environment-parameterizable (devnet vs mainnet) with sensible defaults shipped per release.

### 3.2 Anchor discriminators

- 8-byte instruction discriminators for every PA and forwarder instruction the integrator may call.
- 8-byte event discriminators for every event the indexer must decode.
- Computed once at build time from instruction/event names; not hand-typed constants.

### 3.3 Instruction builders — Protocol Adapter

- `txdata_init`
- `txdata_write` (chunked; chunk size constant is shipped from this package)
- `settle_from_txdata`
- `txdata_close`

Each builder takes typed inputs and returns a fully-formed Solana `Instruction` value (Rust) or `TransactionInstruction` (TypeScript) with the correct accounts, discriminator, and serialized arguments.

### 3.4 Instruction builders — Forwarder

- Wrap entry point (with full CPI account list assembled internally).
- Unwrap entry point (with full CPI account list assembled internally).
- The integrator does not have to know `num_accounts` per call or the account ordering; the builder produces a correct slice.

### 3.5 Settle transaction orchestration

- A helper that assembles the full settle transaction: `SetComputeUnitLimit`, `RequestHeapFrame`, optional `Ed25519Program` verify ix(es) for wrap authorizations, and the `settle_from_txdata` ix with correct `remaining_accounts`.
- Computes `ed25519_ix_index` from the actual transaction layout, not from a constant.
- Chunked upload helper: bincode-serializes the ARM `Transaction`, splits into appropriately-sized chunks, returns the sequence of `txdata_init` / N × `txdata_write` / `settle_from_txdata` / `txdata_close` instructions.

### 3.6 PDA derivation

- `derive_pa_state_pda()`
- `derive_tx_data_pda(authority, upload_id)`
- `derive_nullifier_pda(pa_state, nullifier_bytes)`
- `derive_root_marker_pda(pa_state, root_bytes)`
- `derive_forwarder_escrow_pda(mint)` — `[b"escrow", mint]` seed schema
- `derive_associated_token_address(mint, owner)` — convenience wrapper

Each function takes the canonical inputs and returns the `(Pubkey, bump)` pair.

### 3.7 Account decoders

- `decode_pa_state(bytes) -> PAStateAccount` — **cursor-based parser** that walks the Borsh schema field by field. No hardcoded offsets. The struct shape must match the on-chain `PAStateAccount` exactly: `bump`, `authority`, `verifier_router`, `proof_selector: [u8; 4]`, `pending_authority: Option<Pubkey>`, `lifecycle`, `root: [u8; 32]`, `next_index: u64`, `current_depth: u8`, `frontier: Vec<[u8; 32]>`, `min_expiry_slots: u64`, `max_expiry_slots: u64`.
- The schema is sourced from the PA repository at the commit tagged by this package's version. When the PA's struct changes, the new struct shape ships in the next release.

### 3.8 Event decoders

- Decoders for every Anchor event emitted by the PA:
  - `TransactionExecutedEvent { tags: Vec<[u8; 32]>, logic_refs: Vec<[u8; 32]> }`
  - `ResourcePayloadEvent { index: u32, blob: Vec<u8>, ... }`
  - `DiscoveryPayloadEvent { index: u32, blob: Vec<u8>, ... }`
  - `ExternalPayloadEvent { index: u32, blob: Vec<u8>, ... }`
  - `ApplicationPayloadEvent { index: u32, blob: Vec<u8>, ... }`
  - `ActionExecutedEvent { ... }`
- A high-level helper that takes a transaction's log lines, identifies `Program data: <base64>` entries, dispatches on the 8-byte discriminator, and returns typed event values.
- The indexer consumes the IDL files (`idl/protocol_adapter.json`) for the same decoding in non-Rust/TS contexts.

### 3.9 Wrap message construction

- `WrapMessage` type with fields `forwarder: [u8; 32]`, `mint: [u8; 32]`, `amount: u64`, `nonce: u64`, `deadline: i64`, `action_tree_root: [u8; 32]`. **`deadline` is `i64`, not `u64`** — match the on-chain forwarder.
- `serialize_wrap_message(WrapMessage) -> [u8; 120]` — fixed 120-byte little-endian layout per the forwarder's expectation.
- `hash_wrap_message(WrapMessage) -> [u8; 32]` — SHA-256 over the 120-byte serialization.
- `base64_wrap_digest(WrapMessage) -> String` — base64-encoded SHA-256 digest as UTF-8 text. This is the exact byte string the wallet's `signMessage` must sign and the on-chain `Ed25519Program` instruction must carry.
- `build_ed25519_verify_ix(pubkey, signature, base64_text) -> Instruction` — wraps `solana_ed25519_program::new_ed25519_instruction_with_signature`.

### 3.10 External call construction

- `SolanaExternalCall { program_id: [u8; 32], instruction_data: Vec<u8>, expected_output: Vec<u8>, output_mode: OutputMode, num_accounts: u8 }` — exact match for the PA's `solana-pa-prototype/src/types.rs`.
- `OutputMode = ReturnData | OutputAccount { index: u8, offset: u32, len: u32 }`.
- Convenience constructors for wrap and unwrap calls that fill `num_accounts` and `output_mode` correctly.

### 3.11 Forwarder CPI account assembly

- `build_wrap_cpi_accounts(...)` — assembles the 12-account segment in the correct order for a wrap CPI.
- `build_unwrap_cpi_accounts(...)` — assembles the 9-account segment in the correct order for an unwrap CPI.
- The integrator passes the relevant pubkeys (mint, user ATA, recipient ATA, etc.); ordering is determined inside the helper.

### 3.12 Approve helper (frontend-facing)

- `build_approve_ix(user_ata, mint, owner, amount) -> Instruction` — constructs the SPL `Approve` instruction naming the forwarder's escrow PDA as delegate. The user's wallet signs the transaction containing this; the helper does not sign.

### 3.13 ATA helpers

- `derive_associated_token_address(mint, owner)` — standard ATA derivation.
- `build_create_ata_idempotent_ix(payer, owner, mint)` — idempotent ATA creation instruction for unwrap destinations.

### 3.14 Commitment tree primitives

- `hash_two(left: [u8; 32], right: [u8; 32]) -> [u8; 32]` — SHA-256 over the concatenation, matching the PA's `hash_two`.
- `padding_leaf() -> [u8; 32]` and the zero-hash sequence `[H_0, H_1, ..., H_31]` precomputed.
- `CommitmentTreeState` type wrapping `{ root, next_index, current_depth, frontier }` with an `append(commitment)` method that updates the state and returns the new root. Used by backend to derive the new root-marker PDA for a settlement.

### 3.15 Constants

- `MIN_COMPUTE_UNIT_LIMIT` — the minimum CU value the settle ix requires (currently 500,000). Source of truth.
- `MIN_HEAP_FRAME_BYTES` — the minimum heap frame the settle ix requires (currently 262,144). Source of truth.
- `TXDATA_WRITE_CHUNK_BYTES` — the chunk size for `txdata_write` (currently 900).
- `FORWARDER_WRAP_NUM_ACCOUNTS = 12`, `FORWARDER_UNWRAP_NUM_ACCOUNTS = 9`.
- These must change here when they change in the PA. The integrator imports them; bumping the PA without bumping this package is a release-process error.

### 3.16 Forwarder escrow registry (optional but recommended)

- A per-mint mapping `{ mint, escrow_pda, escrow_ata }` for supported SPL mints (e.g. USDC devnet/mainnet).
- Either precomputed and shipped as static data, or derived on-demand via §3.6 helpers.
- Allows integrators to avoid re-deriving and gives a single audit point for "where wrapped USDC lives."

### 3.17 IDL files

- `idl/protocol_adapter.json` — Anchor IDL for the PA program at this version.
- `idl/spl_token_forwarder.json` — Anchor IDL for the forwarder at this version.
- Regenerated from the PA repo on every release. The integrator's indexer (Elixir or any other non-Rust/TS consumer) reads these directly.

---

## 4. Non-functional requirements

### 4.1 Versioning

- SemVer. Major version bump on any wire-incompatible PA change.
- Rust crate version and TS package version must match exactly per release.
- Git tags follow `vMAJOR.MINOR.PATCH` and produce both a Cargo publish and an npm publish.

### 4.2 Source-of-truth coupling to the PA

- This package is paired with a specific PA commit. The pinned commit is recorded in the repo (e.g. `PA_COMMIT.txt` or in CI config) and the IDLs are generated from that commit.
- A PA security or feature release that changes anything in §3 triggers a new release of this package against the new PA commit.
- The package does not "track main" of the PA — releases are explicit, tagged, and reviewed.

### 4.3 Generation vs hand-writing

- Anything Anchor can generate (instruction discriminators, account decoders, IDL-derived types) should be generated, not hand-typed.
- Hand-written code is acceptable for: orchestration (settle-tx assembly), helpers (wrap message construction, base64 encoding), and constants the IDL doesn't express.
- All hand-written code that mirrors on-chain shapes must be round-trip-tested against the on-chain serializer.

### 4.4 Testing

- Unit tests for every PDA derivation, every account decoder, every instruction builder.
- Round-trip tests: serialize a `WrapMessage` and confirm it matches what the on-chain forwarder produces.
- Integration tests: a local validator with PA and forwarder deployed, exercising deposit → transfer → unwrap end-to-end using this package's helpers. The test must fail loudly if any wire-level disagreement exists.
- CI runs Rust and TS tests against the same pinned PA commit on every push.

### 4.5 Documentation

- Each exported function has a doc comment / TSDoc explaining its purpose and the PA-side invariant it enforces.
- A `CHANGELOG.md` per release noting which PA changes drove this release.

---

## 5. Out of scope

The following are **not** in this package, regardless of how convenient they might be to colocate:

- **Proof generation.** Routing to the AnomaPay workers queue is the backend's concern.
- **Transaction submission and confirmation.** Blockhash-expiry-aware confirmation polling, retry logic, and RPC client choice are the integrator's.
- **Wallet integration.** `@solana/wallet-adapter` use, Phantom-specific behavior, wallet UI — frontend concern.
- **Indexer sync mechanics.** Cursor management, rate-limit handling, hosted-indexer choice — indexer's concern. This package only supplies the IDL.
- **Merkle tree replay.** The backend replays the PA's commitment tree to derive the next root-marker PDA; this package supplies `hash_two`, `padding_leaf`, and `CommitmentTreeState` as primitives, but the *use* of those primitives (when to replay, how to coordinate concurrent submissions) belongs to the backend.

---

## 6. Release process

1. **Trigger.** A PA release (new tag in `solana-protocol-adapter`) that changes any item in §3.
2. **Pin update.** Update this repo's `PA_COMMIT.txt` to the new tag.
3. **Regenerate.** Run the IDL extraction and code generation. Hand-written items are reviewed against the new PA source.
4. **Test.** Full CI pass: unit, round-trip, and integration tests against the new PA commit.
5. **Tag.** Cut a SemVer release; major bump for wire-incompatible changes.
6. **Publish.** Cargo + npm publishes from CI.
7. **Notify integrators.** Backend, frontend, and indexer maintainers receive a release notification with the changelog and a summary of which fields/values changed.

Integrators bump their dependency at their own pace, but the package's release is the single coordination event — they no longer have to chase a multi-repo cross-team patch.

---

## 7. Open questions

These need resolution before implementation begins.

1. **Maintainer.** Does the PA team own this package directly, or is it shared with the integrator teams? Recommend: PA team owns it (they have the source of truth for every contract in §3); integrators contribute via PRs for helpers they need that aren't yet exposed.
2. **Auto-generation pipeline.** Anchor's IDL emission gives us instruction discriminators and some types, but the cursor-based `PAStateAccount` decoder, the wrap message layout, and the forwarder CPI account ordering need either custom generators or hand-written code. Decision: which items in §3 are codegen targets vs hand-written?
3. **Forwarder escrow registry shape.** Static JSON shipped in the package, or derived on-demand? Static is more auditable; on-demand handles new mints without a release. Recommend: ship a static registry of officially-supported mints + the derivation helper for anything else.
4. **Workspace organization.** Should `rust/` be a single crate or a Cargo workspace with sub-crates (`pa-instructions`, `pa-accounts`, `forwarder`, etc.)? Single crate is simpler; workspace allows the indexer to depend on only the event-decoding subset. Recommend: start as a single crate; split later if a real consumer benefits.
5. **TS package name.** `@anoma/pa-solana-client` requires an `@anoma` npm scope. Confirm availability and whether to use a scoped name or a flat one.
6. **Anchor version compatibility.** Different consumers may want different Anchor versions in their toolchain. Decision: pin to one Anchor version per release, or support multiple?
7. **Solana SDK major version.** `solana-sdk` 2.x is current; will the package support 1.x backports?
8. **CI for cross-package consistency.** A test that builds the Rust crate and the TS package against the same PA fixture and confirms every shared constant matches byte-for-byte. Worth setting up as part of v0.1.
9. **License.** Match the PA's license (likely Apache-2.0 or MIT, pending PA team confirmation).

---

## 8. Initial scope for v0.1

Smallest useful version. Ship these and integrators can start adopting:

- Program ID constants for PA, forwarder, ed25519 program (devnet defaults).
- Instruction discriminators for the four PA instructions and the forwarder's wrap/unwrap.
- PDA derivation helpers for all five PDAs in §3.6.
- `decode_pa_state` cursor-based decoder.
- `WrapMessage` type + serializer + SHA-256 + base64 helper.
- `SolanaExternalCall` type + `OutputMode` enum.
- Constants in §3.15.
- IDL files in `idl/`.
- Basic README pointing at this REQUIREMENTS.md.

Items deferred to v0.2+: full settle-tx orchestrator, commitment-tree replay primitives, forwarder CPI account assembly helpers, full event-decoder set with log-line dispatch.

---

## 9. Acceptance criteria for v0.1

- `anomapay-backend` can replace its inline PA-instruction-builder and PDA-derivation code with imports from this crate, with no behavior change.
- `pay-interface-app` can replace its inline wrap-message construction, base64 hashing, and PA state decoder with imports from this package.
- Both packages produce byte-identical output to the existing inline implementations on a fixed test vector.
- All §4.4 tests pass in CI against the pinned PA commit.
