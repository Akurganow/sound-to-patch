# Release

Releases happen on their own when a pull request merges into `main`. No
one tags or uploads by hand.

## What a merge does

1. The CI workflow runs every check on the merged commit.
2. If `fmt`, `clippy`, `test`, `doc`, `licenses` and `docs` pass, the
   `release` job runs `cog bump --auto`. It reads the commit subjects since
   the last tag:
   - a `feat` commit bumps the minor version;
   - a `fix` commit bumps the patch version;
   - a breaking change (`!` after the type or scope, or a `BREAKING CHANGE:`
     footer) bumps the major version, and the minor version while the
     project is below 1.0;
   - any other type alone bumps nothing. The job ends green and the change
     ships with the next release.
3. A bump runs `cargo xtask set-version`, which writes the version into
   `Cargo.toml` and `Cargo.lock`, updates `CHANGELOG.md` and creates the
   commit `chore(version): vX.Y.Z` with the tag `vX.Y.Z`.
4. The job pushes the commit and the tag in one atomic push with the
   release deploy key. The main ruleset lets only that key push to `main`
   directly. The private key is a secret of the `release` environment,
   which only `main` may deploy to, so a workflow on another branch cannot
   read it.
5. The tag starts the publish workflow. It stops unless the tag is
   `vX.Y.Z` and its commit is on `main`. It builds the macOS arm64 binary,
   the man page, the shell completions and `THIRD-PARTY-LICENSES.html`,
   checks that the binary prints the tag's version, packs each folder in
   `plugins/` as its own zip archive, and publishes a release with the
   changelog section as its notes.

The version commit starts the CI workflow again. The `release` job skips a
commit whose subject starts with `chore(version)`, so the loop stops there.

## The release archive

`sound-to-patch-vX.Y.Z-aarch64-apple-darwin.tar.gz` holds:

- `sound-to-patch`, the binary;
- `man/sound-to-patch.1`;
- `completions/` for bash, zsh and fish;
- `THIRD-PARTY-LICENSES.html`;
- `LICENSE`.

`THIRD-PARTY-LICENSES.html` is also a release asset of its own. Each agent
plugin ships as `<plugin>-vX.Y.Z.zip`, its folder under `plugins/`.

## Releases are immutable

Once a release is published, its tag and assets cannot change. The title
and notes stay editable. A deleted release's tag can never be reused.
The tags ruleset covers every tag: only the release deploy key creates
one, and no one deletes or moves one, including a tag whose release
failed.

## When something fails

- **A check fails on `main`.** The `release` job does not run. Fix the
  problem in a new pull request; its merge releases everything since the
  last tag.
- **The publish workflow fails.** The tag exists and no later merge bumps
  it again. Fix the cause on `main` if needed, then re-run the failed
  publish run, or start the publish workflow by hand with the tag name.
  It skips a release that already exists. Never delete and re-create a
  tag.
- **The push of the version commit fails** because `main` moved on. The
  run of the newer commit releases both.
