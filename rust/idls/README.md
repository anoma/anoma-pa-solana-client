# IDL files

Anchor IDL files of the `solana-protocol-adapter` programs. `protocol_adapter.json` is the file committed at the adapter tag recorded in `PA_TAG.txt` (`solana-pa-prototype/idl/protocol_adapter.json`), byte for byte; the `idl_at_tag` test checks it. Anchor's `declare_program!` reads IDLs from this directory, an `idls/` directory beside the crate's manifest, so the crate builds wherever it is fetched.

Files:

- `protocol_adapter.json` — IDL for the PA Anchor program (production build).
- `block_time_forwarder.json` — IDL for the example forwarder the adapter's fixtures call.
- `test_forwarder.json` — IDL for the adapter repo's localnet test forwarder.

Regenerated on every release. Consumed by:

- The Rust crate, whose `declare_program!` generates `generated::protocol_adapter` from `protocol_adapter.json`.
- The TS package's tests, which check its hand-written error, event, instruction and state-account names against it.
- External integrators that need event decoding without depending on a language package (e.g. the Elixir indexer consumes these directly).
