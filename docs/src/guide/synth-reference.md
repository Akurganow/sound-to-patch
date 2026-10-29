# Using the synth reference

The synth reference is an agent plugin, `sound-to-patch-reference`. An
agent reads it when it writes an edit list for `sound-to-patch preset
apply`, writes notes from a producer's words, or explains a preset. It
lists the controls of each target synth by layer, with the on-screen
label, the preset key, the unit, the range and how to tune each control by
ear. It also holds the music dictionary and the word dictionary: what
words such as warm or plucky mean, and how far listeners agree on them.

## Install it

Add the repository as a marketplace, then install the plugin:

```sh
claude plugin marketplace add Akurganow/sound-to-patch
claude plugin install sound-to-patch-reference@sound-to-patch
```

Each release also ships the plugin as `sound-to-patch-reference-vX.Y.Z.zip`.
Pass the unpacked folder or the zip to a session with `--plugin-dir`.

A session opened on a clone of this repository needs no install: it finds
the same skill under `.claude/skills/`.

On Windows, let git create symbolic links before you install from GitHub
or clone the repository. Enable Developer Mode (or run the shell
elevated), then run `git config --global core.symlinks true`. Without
them the plugin installs with no reference. The
[agent reference page](../development/agent-reference.md#plugin-loading)
lists which install paths carry the reference.

## Use it

Ask the agent for the sound in your own words. The skill loads when the
request needs a control, a note rule or a sound word, so the agent names
each control by its on-screen label and checks its edit list with
`sound-to-patch preset apply` before it hands you the preset.

Vital and Serum 2 are trademarks of their owners. This project is not
affiliated with or endorsed by them.
