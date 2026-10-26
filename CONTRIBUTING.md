# Contributing

## Ways to contribute

- Report a bug with the bug form. Include the command, the input kind and
  the full output.
- Pick an open task issue. Each epic issue lists its tasks; a task names its
  scope, its acceptance cases and the tests it needs.
- Improve the docs in `docs/`, the README or the rustdoc.

Out of scope for now: training models, rendering presets to audio, tempo
detection, drums and running the toolkit in the cloud.

## Setup

1. Install Rust 1.91 or newer with rustup.
2. Install the tools CI uses: `cargo install --locked cargo-nextest cocogitto`
   and `cargo install --locked --features cli cargo-about`.
3. Clone the repository and run `cog install-hook commit-msg` once in it.

The hook checks each local commit message. It lives in `.git/hooks`, so
every clone installs it once.

The checks CI runs, in the order a pull request should pass them:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --workspace
cargo test --workspace --doc
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items
cargo about generate --locked --fail -c about.toml about.hbs -o THIRD-PARTY-LICENSES.html
```

The book check needs mdBook, its KaTeX preprocessor and lychee:

```sh
cargo install --locked mdbook@0.5.4 mdbook-katex@0.10.0 lychee
mdbook build docs
cargo doc --no-deps
cp -R target/doc docs/book/api
lychee --offline --no-progress --include-fragments --exclude-path docs/book/api --exclude-path docs/book/404.html docs/book
cargo xtask check-architecture
cargo xtask check-credits
cargo xtask check-rust-version
```

The `commits` check also runs `cargo xtask check-pr-body`, which reads the
pull request description and title from the `PR_BODY` and `PR_TITLE`
variables. They become the squash commit, so the check also rejects the
markers that make GitHub skip workflows, such as the bracketed "skip ci":

```sh
PR_BODY="$(cat description.md)" PR_TITLE="fix: the title" cargo xtask check-pr-body
```

`cargo xtask` lists every development task.

## Quality bar

- Lints live in `[workspace.lints]` in `Cargo.toml` and deny Clippy's
  pedantic group, `unwrap`, `expect` and `panic` outside tests, `unsafe`
  code and missing docs. Do not lower a lint to land a change; fix the code.
- Public APIs take unit newtypes, never a bare float.
- Every public item has rustdoc. Changed behaviour has a test. The testing
  guide in `docs/src/development/testing.md` says which test goes where.
- Expected test values never come from an AI. Take them from a file saved
  by Serum 2 or from an independent reference.
- An epic closes with its docs and the checklist in its issue.

## Commits and pull requests

- Commit messages follow Conventional Commits: `type(scope): subject`. The
  scope is optional; the allowed scopes are in `cog.toml`. Write the subject
  for users: `fix(notes): keep pitch bends within two semitones`.
- `feat` releases a minor version, `fix` a patch. A breaking change uses `!`
  after the scope or a `BREAKING CHANGE:` footer. Other types release
  nothing on their own.
- `main` takes pull requests only, and squash is the only merge method. The
  pull request title becomes the commit subject and the changelog entry, and
  the description becomes the commit body. The `commits` check verifies the
  title with `cog verify` and the description against the template.
- Never edit `CHANGELOG.md`. The release job writes it.

## AI assistance

Agents may write changes. Such a pull request follows these rules:

- The description checks the agent box under Authorship.
- The Verification section shows the commands that were run and their
  output.
- The maintainer reviews every agent-written pull request before it
  merges. An agent never merges, approves or enables auto-merge on a pull
  request.
- Agent instructions in the repository point to this file instead of
  repeating its rules.

## Licensing

The project is MIT-licensed, and contributions are accepted under the same
license.

- Code or weights under MIT, Apache-2.0, BSD, ISC, CC0 or CC BY may be
  reused with attribution.
- GPL, LGPL, AGPL and MPL-2.0 projects, non-commercial weights and code
  without a license serve as sources of facts only. A GPL-family program may
  run as a separate process. An unmodified MPL-2.0 crate may be a
  dependency.
- The README credits list every studied project, paper and dataset with its
  name, a link and its license. Works under a non-permissive license, or
  with none, carry "Inspiration only.": we read them for facts and ideas and
  copy no code, text or data. A pull request that cites a new work adds its
  credit entry; `cargo xtask check-credits` enforces it.
- Crates we depend on are not credits. Their notices ship in
  `THIRD-PARTY-LICENSES.html`. A new dependency adds its license to
  `about.toml` when the license is new to the tree.
- Docs name no third-party brands except Serum and Vital. A credit is not
  a brand mention.
- Vital is GPL-3.0, so we use its facts only, in our own words, and `vita`
  runs only as a separate process.
- Committed test fixtures have a row in `tests/fixtures/PROVENANCE.md`.

## Security

Report vulnerabilities privately, as `SECURITY.md` describes. Do not open a
public issue for them.
