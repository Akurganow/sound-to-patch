# Agent notes

Facts learned while working on this repository.

- A `feat` or `fix` merge starts a release commit that bumps the version in
  `Cargo.lock` and `fuzz/Cargo.lock`. An open pull request that adds a
  workspace member then needs a rebase and a lock refresh. Other commit
  types release nothing.
- The `main` ruleset blocks a merge while any review thread is open,
  CodeRabbit threads included.
- `gh pr merge --match-head-commit` needs the full 40-character SHA.
- The macOS `test` job can wait in the runner queue for hours.
- A debug build runs the f0 track at about 40 s per second of audio. Use a
  release build for real stems and large signal sets.
- An estimator change that passed generated signals still gave false
  records on real stems. Check such a change on real audio as well, and
  keep that audio out of the repository.
