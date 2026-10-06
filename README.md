# anoma-pa-solana-client

Releases: [`RELEASE_CHECKLIST.md`](RELEASE_CHECKLIST.md). The Rust crate is `anoma-pa-solana-client` on crates.io and the npm package `@anomaorg/pa-solana-client`.

Client bindings for the Solana Protocol Adapter. One source of truth for instruction builders, account decoders, PDA helpers, event decoders, and constants — published as a Rust crate and a TypeScript/npm package from this repository. The SPL token forwarder's bindings are in [anoma/anomapay-spl-token-forwarder](https://github.com/anoma/anomapay-spl-token-forwarder) (`crates/client` and `ts/`).

**Status:** paired with the V2 protocol adapter (branch `anthony/arm-v2-port`, commit in `PA_COMMIT.txt`; devnet program `5zeqkB3kc9fd1RvaXB2GeMB53Jgf98QJtaFK38e6tTsc`). See [REQUIREMENTS.md](REQUIREMENTS.md) for the intended surface.

## Layout

| Path | Purpose |
|------|---------|
| `REQUIREMENTS.md` | Authoritative spec for what this package must provide. Implementation follows from it. |
| `rust/` | Rust crate (`Cargo.toml`, `src/`). Cargo target. |
| `tools/settle-fixture/` | Pairing check: settles one adapter fixture on a cluster through the crate's builders, then verifies the replayed root and decodes the settlement's events. |
| `ts/` | TypeScript / npm package (`package.json`, `src/`). npm target. |
| `idl/` | Anchor IDL files extracted from the adapter repo's programs, regenerated per release. |
| `PA_COMMIT.txt` | The `solana-protocol-adapter` commit this release pairs with. Updated per release. |

## Integrators

- `anomapay-backend` (Rust): Cargo dependency on `rust/`.
- `pay-interface-app` (TypeScript): npm dependency on `ts/`.
- `galileo-indexer` (Elixir): consumes `idl/*.json` for event decoding via its hosted indexing service.

## Pairing check

`tools/settle-fixture` exercises the whole client surface against a live adapter: it decodes the state account, plans the settlement with `plan_settlement` (the upload, the remaining accounts with any historical-root markers, and the predicted new root), uploads the fixture transaction, settles it, asserts the on-chain root equals the replay, and decodes the settlement's CPI events, printing the compute units the settlement consumed. Run it against devnet, or against a local validator set up from the adapter repo's `solana-pa-prototype/`:

```bash
./scripts/dev.sh validator-deploy                                    # every program loaded at genesis; keeps running
PA_OWNER=<pubkey> PA_VERIFIER_ROUTER=BetEAE4npinksQBxvqUN1KkCVjYFJywWao45MSWtp5yg PA_PROOF_SELECTOR=73c457ba \
  ./scripts/dev.sh init --cluster localnet
./scripts/dev.sh lookup-table --cluster localnet
```

Then settle a fixture:

```bash
cargo run -p settle-fixture -- \
  --url http://127.0.0.1:8899 \
  --keypair ~/.config/solana/id.json \
  --fixture <adapter repo>/solana-pa-prototype/tests/fixtures/batch_groth16.json \
  --call-accounts 3mesRGxMv9wRB1xp7X4uxbf7GwnQC9PpHSJyCzcXwrsf,SysvarC1ock11111111111111111111111111111111 \
  --lookup-table <address>
```

`--call-accounts` is the account segment of one external call (repeat it per call, in call order); the adapter's committed fixtures call the block-time forwarder with the forwarder and the clock sysvar. The fixture must have been proven for the deployed build: a fixture's proof binds the circuit image ids the deployment was built with.

The settle step is a v0 transaction against the deployment's settlement lookup table (`--lookup-table`, default `SETTLE_LOOKUP_TABLE`, the devnet table). The tool prints the wire size and how many keys the table absorbed; the adapter repo's `dev.sh lookup-table` command creates a deployment's table.

`fixtures/events_fixture.json`, the cross-package check of the event decoders, is regenerated from such a run by `node tools/capture-events-fixture.mjs <rpc url> <signature>...`: it records the adapter events of the listed transactions with the values @anchor-lang/core's BorshCoder decodes from them, and encodes one entry for each IDL event type the transactions did not emit. After an IDL change that keeps the events' layout, `node tools/capture-events-fixture.mjs --recorded` decodes the events the fixture already records anew.

## Release coupling

This package is paired with a specific `solana-protocol-adapter` commit (recorded in `PA_COMMIT.txt`). A PA release that changes the wire-level contracts in `REQUIREMENTS.md` §3 triggers a new release here, against the new PA commit. The Rust crate and TS package release together with matching SemVer versions.
