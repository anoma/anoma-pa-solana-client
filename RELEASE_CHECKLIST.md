# Release checklist

The Rust crate `anoma-pa-solana-client` and the npm package `@anomaorg/pa-solana-client` release together, at one version, as a git tag on `main`, and every release pairs with one adapter commit. Neither is published to a registry: consumers pin the tag.

1. On the branch to release: set the version in `Cargo.toml` (`[workspace.package]`) and in `ts/package.json` and `ts/package-lock.json` to the same value; record the adapter commit the release pairs with in `PA_COMMIT.txt`, with the IDL files regenerated from it. CI's release-readiness job checks that the versions agree.
2. Merge into `main` and tag the merge commit `bindings/vMAJOR.MINOR.PATCH`, as pa-evm tags its bindings.
3. Consumers pin the tag, every one with the same string (Cargo treats a `rev` and a `tag` of one commit as two crates):

   ```toml
   anoma-pa-solana-client = { git = "https://github.com/anoma/anoma-pa-solana-client", tag = "bindings/vMAJOR.MINOR.PATCH" }
   ```
