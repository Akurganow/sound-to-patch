# Agent instructions

Follow `CONTRIBUTING.md`. Its rules apply to agents as well as people.
The rules below are for agents only.

## Stem input

Stems come from source separators of two kinds. A fixed split always
writes the same set of category stems. A custom split writes the
instruments the user picked. The tool supports both.

- A stem's label or file name never says what the stem holds. A fixed split
  writes every category, so a stem named for an instrument the song lacks
  still carries other material. A custom split names what the user asked
  for, not what the separator produced.
- Decide content, role and register from the audio only. In code, tests,
  docs, reviews and briefs, name a stem by its label ("the stem labelled
  Bass"), never by an instrument read from that label.
- A separator may regenerate a stem instead of cutting it from the mix, so
  its timbre and transients can differ from the original song. The stem is
  the target sound.
- A tempo in a stem's file or folder name is approximate. Estimate or refine
  the tempo from the audio before it drives notes, note lengths or synced
  rates.
