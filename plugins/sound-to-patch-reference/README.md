# sound-to-patch-reference

The synth reference of sound-to-patch, as an agent plugin. Its skill `synth-reference` lists the controls of each target synth by layer: the on-screen label, the preset key, the unit, the range, the default, how sure the value is and how to tune it by ear. An agent reads it while it writes an edit list for `sound-to-patch preset apply` or explains a preset.

Vital and Serum 2 are trademarks of their owners. This project is not affiliated with or endorsed by them.

## Install

Add the repository as a marketplace, then install the plugin:

```sh
claude plugin marketplace add Akurganow/sound-to-patch
claude plugin install sound-to-patch-reference@sound-to-patch
```

Each release also ships this folder as `sound-to-patch-reference-vX.Y.Z.zip`.

The plugin is the entry point for local sessions. A session opened on a clone of this repository also finds the same skill under `.claude/skills/`.

## Contents

- `skills/synth-reference/SKILL.md`: the rules to keep and a pointer into the reference.
- `skills/synth-reference/reference`: a link to the shared `reference/` folder of the repository. Installing the plugin copies the linked files, so the installed plugin carries them. Edit them in `reference/`, never here.

## License

MIT, as the rest of the repository.
