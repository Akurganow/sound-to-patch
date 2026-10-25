<!--
Title: a Conventional Commit, `type(scope): subject`, written for users.
The squash merge turns the title into the commit subject and the changelog
entry, and this description into the commit body. The `commits` check reads
both. See CONTRIBUTING.md.
-->

## Summary

<!-- What changed and why. One `Closes #N` line per issue this closes. -->

## Authorship

<!-- Check exactly one box. -->

- [ ] A person wrote this change.
- [ ] An agent wrote this change. The maintainer reviews it before merge.

## Verification

<!-- verification -->
<!-- Paste the commands you ran and their output. -->

## Checklist

- [ ] `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings` and `cargo nextest run --workspace` pass.
- [ ] New public items have rustdoc. Changed behaviour has a test.
- [ ] New fixtures have a row in `tests/fixtures/PROVENANCE.md`.
- [ ] New dependencies have their license in `about.toml`.
- [ ] A new cited work has a credit entry.
- [ ] Docs and the `--help` snapshot match the change.
