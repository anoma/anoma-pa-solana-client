# IDL files

Anchor IDL files extracted from the `solana-protocol-adapter` programs at the commit recorded in `PA_COMMIT.txt`.

Files:

- `protocol_adapter.json` — IDL for the PA Anchor program (production build).
- `block_time_forwarder.json` — IDL for the example forwarder the adapter's fixtures call.
- `test_forwarder.json` — IDL for the adapter repo's localnet test forwarder.

Regenerated on every release. Consumed by:

- The Rust crate and TS package, for code generation.
- External integrators that need event decoding without depending on a language package (e.g. the Elixir indexer consumes these directly).
