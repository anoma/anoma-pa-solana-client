# Release checklist

The Rust crate `anoma-pa-solana-client` and the npm package `@anomaorg/pa-solana-client` release together, at one version, as a git tag on `main`, and every release pairs with one adapter tag. The crate is published to crates.io at that version; the npm package is not published to a registry, and its consumers pin the tag.

1. On the branch to release: set the version in `Cargo.toml` (`[workspace.package]`) and in `ts/package.json` and `ts/package-lock.json` to the same value; record the adapter tag the release pairs with in `PA_TAG.txt`, and copy the adapter's `solana-pa-prototype/idl/protocol_adapter.json` and `solana-pa-prototype/idls/verifier_router.json` at that tag to `rust/idls/` (the `idl_at_tag` test checks they are the same bytes). CI's release-readiness job checks that the versions agree. `cargo publish --dry-run -p anoma-pa-solana-client` builds the crate as crates.io will serve it, from its packaged files alone.
2. Merge into `main` and tag the merge commit `bindings/vMAJOR.MINOR.PATCH`, as pa-evm tags its bindings.
3. From the tagged commit, `cargo publish -p anoma-pa-solana-client`. Rust consumers depend on the published version:

   ```toml
   anoma-pa-solana-client = "MAJOR.MINOR.PATCH"
   ```
