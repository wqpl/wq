# Releasing wq

All Cargo workspace members and the npm `wq-wasm` package share one version and
one `vVERSION` Git tag.

Cargo 1.90 or newer is required because it can publish interdependent workspace
packages in dependency order.

## Git remotes

List every release destination in the local Git remote group:

```sh
git config remotes.publish "github codeberg"
```

The release script pushes each non-GitHub mirror first and the GitHub repository
last. Each remote receives the release commit and tag in one atomic push. The
GitHub tag push starts the crates.io and npm publishing workflows only after the
mirrors accept the release.

## Cut a release

Run the release command from `wq-wasm`:

```sh
npm run release
```

Without an argument, it increments a trailing prerelease number or a stable
patch version. Pass an explicit version when needed:

```sh
npm run release -- 0.10.0-preview1
```

The command verifies a clean worktree, updates Cargo and npm versions, runs the
release checks, creates a commit and annotated tag, then asks before pushing.
Use the manual dispatch for `publish-crates.yml` to run a crates.io dry run
without publishing.

Cargo workspace publishing is not atomic. If part of a crates.io release
succeeds before a later package fails, manually dispatch `publish-crates.yml`
from the original release tag. Select one remaining package, enable `publish`,
and recover packages in dependency order. The workflow rejects publishing from
a branch or from a tag that does not match the workspace version.
