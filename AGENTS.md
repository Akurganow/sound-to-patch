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
- A debug build runs the f0 track at about half a second per second of
  audio, and a release build about 25 times faster. Use a release build
  for real stems and large signal sets.
- An estimator change that passed generated signals still gave false
  records on real stems. Check such a change on real audio as well, and
  keep that audio out of the repository.
- Judge a real-audio check on instrument stems. Vocals are not a project
  goal: a vocal stem only shows false records, blocks no merge, and gets no
  estimator rule of its own.

## Plan before work

Plan every step for yourself before its first action, inside the decomposition
the tracker already holds. The plan needs no approval. A small step gets a few
lines with the same points.

1. Read the issue, its epic, dependencies, comments and code.
2. Check whether the thing already exists.
3. State the request in your own words, apart from your guesses.
4. Treat no opinion as a decision.
5. Name one result and its check, from the issue's acceptance criteria.
6. List what is in scope and out of scope.
7. Give a rule the issue did not ask for its own issue.
8. Name the risks and the cheapest test of each.
9. Measure a small sample before writing code.
10. Name every comparison's base by SHA.
11. Ask for the user's decisions before the work, with background and figures.
12. List the steps in order: who runs each, result, dependencies, parallel work.
13. Name each costly run, how many times it runs, and when to stop and ask.
14. List the issues, epic and documents to update at the end.

When facts stop matching the plan, stop and revise the plan.
