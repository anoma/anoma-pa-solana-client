# anoma-pa-solana-client

PA-side client bindings for the Solana Protocol Adapter and SPL Token Forwarder. One source of truth for instruction builders, account decoders, PDA helpers, event decoders, and constants — published as a Rust crate and a TypeScript/npm package from this repository.

**Status:** spec phase. See [REQUIREMENTS.md](REQUIREMENTS.md) for the full scope, surface, and open questions.

## Layout

| Path | Purpose |
|------|---------|
| `REQUIREMENTS.md` | Authoritative spec for what this package must provide. Implementation follows from it. |
| `rust/` | Rust crate (`Cargo.toml`, `src/`). Cargo target. |
| `ts/` | TypeScript / npm package (`package.json`, `src/`). npm target. |
| `idl/` | Anchor IDL files extracted from the PA and forwarder programs, regenerated per release. |
| `PA_COMMIT.txt` | The `solana-protocol-adapter` commit this release pairs with. Updated per release. |

## Integrators

- `anomapay-backend` (Rust): Cargo dependency on `rust/`.
- `pay-interface-app` (TypeScript): npm dependency on `ts/`.
- `galileo-indexer` (Elixir): consumes `idl/*.json` for event decoding via its hosted indexing service.

## Release coupling

This package is paired with a specific `solana-protocol-adapter` commit (recorded in `PA_COMMIT.txt`). A PA release that changes the wire-level contracts in `REQUIREMENTS.md` §3 triggers a new release here, against the new PA commit. The Rust crate and TS package release together with matching SemVer versions.
