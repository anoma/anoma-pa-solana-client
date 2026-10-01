# anoma-pa-solana-client

Releases: [`RELEASE_CHECKLIST.md`](RELEASE_CHECKLIST.md). The Rust crate is `anoma-pa-solana-client` on crates.io and the npm package `@anomaorg/pa-solana-client`.

PA-side client bindings for the Solana Protocol Adapter and SPL Token Forwarder. One source of truth for instruction builders, account decoders, PDA helpers, event decoders, and constants — published as a Rust crate and a TypeScript/npm package from this repository.

**Status:** paired with the V2 protocol adapter (branch `anthony/arm-v2-port`, commit in `PA_COMMIT.txt`; devnet program `28Hvr1YFv2ouGN2fS99aF3ZzYXzkncJVVaHcZNhquLFT`). The SPL Token Forwarder bindings describe the V2 forwarder (program `5CrHbBeHjg53UyL3Htn9dCYYTy68fMcrbDoeAdo4yQrx`, deployed with the adapter). See [REQUIREMENTS.md](REQUIREMENTS.md) for the intended surface.

## Layout

| Path | Purpose |
|------|---------|
| `REQUIREMENTS.md` | Authoritative spec for what this package must provide. Implementation follows from it. |
| `rust/` | Rust crate (`Cargo.toml`, `src/`). Cargo target. |
| `tools/settle-fixture/` | Pairing check: settles one adapter fixture on a cluster through the crate's builders, then verifies the replayed root and decodes the settlement's events. |
| `ts/` | TypeScript / npm package (`package.json`, `src/`). npm target. |
| `idl/` | Anchor IDL files extracted from the PA and forwarder programs, regenerated per release. |
| `PA_COMMIT.txt` | The `solana-protocol-adapter` commit this release pairs with. Updated per release. |

## Integrators

- `anomapay-backend` (Rust): Cargo dependency on `rust/`.
- `pay-interface-app` (TypeScript): npm dependency on `ts/`.
- `galileo-indexer` (Elixir): consumes `idl/*.json` for event decoding via its hosted indexing service.

## Pairing check

`tools/settle-fixture` exercises the whole client surface against a live adapter: it decodes the state account, replays the commitment tree to predict the new root marker, uploads a fixture transaction with the `txdata_*` builders, settles it with `settle_from_txdata_ix`, asserts the on-chain root equals the replay, and decodes the settlement's CPI events, printing the compute units the settlement consumed. Run it against devnet, or against a local validator set up from the adapter repo's `solana-pa-prototype/`:

```bash
./scripts/dev.sh validator-deploy                                    # every program loaded at genesis; keeps running
PA_VERIFIER_ROUTER=BetEAE4npinksQBxvqUN1KkCVjYFJywWao45MSWtp5yg PA_PROOF_SELECTOR=73c457ba \
  ./scripts/dev.sh init --cluster localnet
STF_LOGIC_REF=<logic ref hex> STF_EMERGENCY_COMMITTEE=<pubkey> STF_TOKEN_MINT=<mint> \
  ./scripts/dev.sh forwarder init --cluster localnet                 # for a wrap fixture
STF_TOKEN_MINTS=<mint> ./scripts/dev.sh lookup-table --cluster localnet
```

A wrap fixture also needs its seeded mint and user: the mint created, the user's token account holding the amount, and the forwarder's escrow authority approved as delegate. Then:

```bash
cargo run -p settle-fixture -- \
  --url http://127.0.0.1:8899 \
  --keypair ~/.config/solana/id.json \
  --fixture <adapter repo>/solana-pa-prototype/tests/fixtures/batch_groth16.json \
  --call-accounts 3mesRGxMv9wRB1xp7X4uxbf7GwnQC9PpHSJyCzcXwrsf,SysvarC1ock11111111111111111111111111111111 \
  --lookup-table <address>
```

The pairing with the adapter at `cea162c` settled `batch_groth16.json` (186,222 compute units) and `spl_token_wrap.json` (a wrap through the escrow authority, with the nonce bitmap created in the settlement; 218,624 compute units) on a local validator set up as above. `fixtures/devnet_v2_seed23.json` is the fixture the rc.6 pairing settled on devnet: proven locally for the deployed build (arm-risc0 v2.0.0-rc.6) and settled as a v0 transaction through the settlement lookup table (signature `4MfhZYndMgfY96RSjkK1CuqJbTW5QWj7hpe3mVdcoLY3cxEzgH3dXkXbuwSnczBbnn2agpH4zNWBGMfxgT5Xgxpo`). `fixtures/devnet_v2_seed21.json` and `fixtures/devnet_v2_seed22.json` were settled by the previous pairing (seed 21 as a legacy transaction, seed 22 as a v0 transaction, signature `s84CCDa5hHCBgYKzeHAKZdVy6dVxfup3xKFHgUNhxZAxm9ybYHC4FREZ3ynSokbWjoHZXb2FEdfMJwnJWzXeQSS`) under the adapter's previous build. All three nullifiers are consumed on devnet, so the files serve as templates for the format. `--call-accounts` is the account segment of one external call (repeat it per call, in call order); the adapter's committed fixtures call the block-time forwarder with the forwarder and the clock sysvar. The fixture must have been proven for the deployed build: a fixture's proof binds the circuit image ids the deployment was built with.

The settle step is a v0 transaction against the deployment's settlement lookup table (`--lookup-table`, default `SETTLE_LOOKUP_TABLE`, the devnet table). The tool prints the wire size and how many keys the table absorbed; the adapter repo's `dev.sh lookup-table` command creates a deployment's table.

`fixtures/events_fixture.json`, the cross-package check of the event decoders, is regenerated from such a run by `node tools/capture-events-fixture.mjs <rpc url> <signature>...`: it records the adapter events of the listed transactions with the values @anchor-lang/core's BorshCoder decodes from them, and encodes one entry for each IDL event type the transactions did not emit.

## Release coupling

This package is paired with a specific `solana-protocol-adapter` commit (recorded in `PA_COMMIT.txt`). A PA release that changes the wire-level contracts in `REQUIREMENTS.md` §3 triggers a new release here, against the new PA commit. The Rust crate and TS package release together with matching SemVer versions.
