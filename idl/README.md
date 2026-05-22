# IDL files

Anchor IDL files extracted from the `solana-protocol-adapter` programs at the commit recorded in `PA_COMMIT.txt`.

Expected files (populated at v0.1):

- `protocol_adapter.json` — IDL for the PA Anchor program.
- `spl_token_forwarder.json` — IDL for the SPL Token Forwarder Anchor program.

Regenerated on every release. Consumed by:

- The Rust crate and TS package, for code generation.
- External integrators that need event decoding without depending on a language package (e.g. the Elixir indexer consumes these directly).
