# Release checklist

The Rust crate `anoma-pa-solana-client` (crates.io) and the npm package `@anomaorg/pa-solana-client` release together, at one version, from a commit on `main`, and every release pairs with one adapter commit.

1. On the branch to release: set the version in `Cargo.toml` (`[workspace.package]`) and in `ts/package.json` and `ts/package-lock.json` to the same value; record the adapter commit the release pairs with in `PA_COMMIT.txt`, with the IDL files regenerated from it. CI's release-readiness job checks that the versions agree and that both packages pack.
2. Merge into `main` and tag the merge commit `vMAJOR.MINOR.PATCH`.
3. Publish, from that commit, with the organization's registry credentials:

   ```sh
   cargo publish -p anoma-pa-solana-client --dry-run
   cargo publish -p anoma-pa-solana-client
   cd ts && npm publish --dry-run && npm publish
   ```

   The crate has no git dependencies, so nothing has to be published ahead of it. Publish it before any crate that depends on it (the AnomaPay Solana resource pins it).
