# Agent notes

Facts learned while working on this repository.

- A `feat` or `fix` merge starts a release commit that bumps the version in
  `Cargo.lock` and `fuzz/Cargo.lock`. An open pull request that changes
  either lock file then needs a rebase and a lock refresh. Other commit
  types release nothing.
- Every pull request gets reviews, from people or from tools. The `main`
  ruleset blocks a merge while any review thread is open.
- `gh pr merge --match-head-commit` needs the full 40-character SHA.
- CI jobs can wait in the runner queue for hours.
- A debug build runs the f0 track at about 40 s per second of audio. Use a
  release build for real stems and large signal sets.
- An estimator change that passed generated signals still gave false
  records on real stems. Check such a change on real audio as well, and
  keep that audio out of the repository.
- `vendor/pyin` is a copy of pyin 1.2.0 without the binary. The release
  pulled `zip` 0.5 through `ndarray-npy`, and no newer release exists.
  Cargo cannot switch off a feature of a transitive dependency. Keep the
  library source equal to upstream; `cargo xtask check-vendor` enforces it.
  Delete `vendor/pyin` once a pyin release after 1.2.0 drops `ndarray-npy`.
