# The Vital preset file

This page states what is known about the `.vital` preset file. No official
format document exists. The facts come from the open-source snapshot of
Vital, from renders of its open engine and from sampled presets.

Every row names a build, a status and a source.

- **Builds.** `source 1.0.6` is the open-source snapshot at commit
  `636ca0e`. `open engine` is that engine compiled into the `vita` 0.1.0
  Python binding, which reports itself as `99999.9.9`. A retail build is
  named by its version, such as `1.0.7`.
- **Statuses.**
  - `checked`: read in the source.
  - `verified`: confirmed by a probe or a load check of ours on the named
    build.
  - `reported`: an outside source says so.
  - `open`: unknown.

A row verified on the open engine is not verified on retail. The pinned
retail build is stable 1.0.7. On 2026-10-26 the download page offered
stable 1.0.7 and early access 1.6.4. The checks still open are listed under
[Open items](#open-items).

## File and top-level keys

| Fact | Build | Status | Source |
| :-- | :-- | :-- | :-- |
| A preset is one compact JSON text. | source 1.0.6 | checked | `load_save.cpp` |
| The top level holds `synth_version`, `preset_name`, `author`, `comments`, `preset_style`, `macro1` to `macro4` and `settings`. | open engine, 1.0.7 | verified | probe: saved default states |
| `settings` holds 772 numeric controls and four structured parts: `sample`, `modulations`, `wavetables` and `lfos`. | open engine, 1.0.7 | verified | probe: saved default states |
| The app saves the keys of each object in alphabetical order. | 1.0.7 | verified | load check: the re-save of `Bass` |
| A re-save keeps every control within one 32-bit float step, rewrites the `version` of each wavetable to the app's build, and delays the sample by 4 frames with some values one step off. | 1.0.7 | verified | load check: the re-save of `Bass`; the open engine keeps the sample |
| A file without `synth_version`, `settings` or `settings.sample`, or a sample without `length`, `sample_rate` or `samples`, makes the app throw. | source 1.0.6 | checked | `load_save.cpp` |
| A missing control takes its default. | open engine | verified | probe: [`minimal.vital`](#fixtures) reads back 772 controls, each at the fresh engine's default |
| An unknown key loads and is dropped on save, in `settings` and at the top level. | open engine | verified | load check: the unknown-keys variant |
| A file without `preset_name` and `author` loads. | open engine | verified | load check: the no-name variant |
| An out-of-range control is applied as it is. The loader does not clamp. | source 1.0.6 | checked | `load_save.cpp` |
| A text in a control fails the load. A bool loads. The writer of this project writes neither. | open engine | verified | probe: `filter_1_cutoff` set to `"x"` and to `true` |
| The app overwrites `preset_name` with the file name on open. | source 1.0.6 | checked | `synth_base.cpp` |
| The writer of this project prints each number as the shortest text that reads back to the same value. `1E5` becomes `100000.0`. Values stay bit for bit. | this project | verified | unit test `number_text_is_canonical_and_values_are_kept` |

## Version gate

The app compares the `major.minor` part of `synth_version` with its own
build. The patch number is ignored.

| Case | Build | Status | Source |
| :-- | :-- | :-- | :-- |
| A file whose major and minor are newer is rejected with a dialog. | source 1.0.6 | checked | `load_save.cpp` |
| A file one patch newer loads. | source 1.0.6 | checked | `load_save.cpp` |
| An older file runs about 30 upgrade steps before it loads. Files from 1.0.0 on need none. | source 1.0.6 | checked | `load_save.cpp` |
| The plugin state path ignores a rejected file. The patch keeps its last sound, and no dialog appears. | source 1.0.6 | checked | `synth_plugin.cpp` |
| The stamps `1.0.6`, `1.0.9`, `1.1.0`, `1.5.5` and `0.9.0` load. `100000.0.0` and a text that is not a version are rejected. | open engine | verified | probe 5 and the stamp fixtures |
| A file stamped `1.0.6` loads, with every edited control in place. | 1.0.7 | verified | load check: `Bass` |
| Files stamped `1.5.5`, `1.6.0` and `1.7.0` are refused with the dialog "Preset was created with a newer version". | 1.0.7 | verified | load check: the stamp variants |
| The app writes its own build, `1.0.7`, as the stamp on save. | 1.0.7 | verified | load check: the re-save of `Bass` |
| Presets saved by 1.6.x do not open in 1.5.5. | 1.5.5 | reported | Vital forum |
| Which stamps the early-access 1.6.4 accepts. | 1.6.4 | open | optional forward check |

**Stamp rule.** Files built from the built-in template carry `1.0.6`, the
open-source build, as `DEFAULT_STAMP`. Stable 1.0.7 loads them: the gate
compares only major and minor, and files from 1.0.0 on run no upgrade step.
`preset apply` keeps the stamp of its template and never lowers it. A
template older than 1.0.0, or one that holds `sub_octave`, is refused. For
the early-access builds, pass a template saved by your own app with
`--template`.

## Retail-only keys

Retail builds write keys that the open source does not know.

| Key | Type | First seen | Default | Status | Source |
| :-- | :-- | :-- | :-- | :-- | :-- |
| `custom_warps` | list of line-generator states, one per oscillator | 1.5.3 | open | checked | 8 of 24 sampled presets |
| `random_values` | list of `{seed}` objects, one per random LFO | 1.5.3 | open | checked | the same sample |
| `osc_1_spectral_morph_phase` to `osc_3_` | number | 1.5.3 | open | checked | the same sample |

Stable 1.0.7 writes none of them. It loads a file that holds them, and so
does the open engine, which drops them on save (verified, load check: the
retail-keys variant). `preset inspect` marks them. Whether builds 1.6.x add
more keys is open.

## Template

| Fact | Build | Status | Source |
| :-- | :-- | :-- | :-- |
| The built-in template holds all 772 controls at their registry defaults, 64 empty modulation slots, three sawtooth wavetables, eight triangle LFO shapes and a seeded noise sample. | this project | checked | `src/preset/vital/template.rs` |
| Its key set and every default match a fresh open engine and the saved default state of 1.0.7. | open engine, 1.0.7 | verified | probe 8: 0 keys differ |
| It loads, and every control reads back unchanged. | open engine | verified | load check: the template variant |
| The built-in template is not the app's Init preset. Its wavetables and sample are our own. | this project | checked | `src/preset/vital/template.rs` |

## Parameters and laws

The registry in
[`src/preset/vital/registry.json`](https://github.com/Akurganow/sound-to-patch/blob/main/src/preset/vital/registry.json)
holds 794 controls. 22 are legacy keys that no module creates. Our own tool
extracts the names, ranges, defaults, engine scales and labels from the
source; the units, laws and provenance are ours.

| Fact | Build | Status | Source |
| :-- | :-- | :-- | :-- |
| The open engine and 1.0.7 create exactly the 772 non-legacy controls. | open engine, 1.0.7 | verified | probe: key sets |
| Ranges, defaults and the build that added each control match for 763 controls. These carry `verified_on = 1.0.7`. | open engine, 1.0.7 | verified | probe: control details and the 1.0.7 default state |
| A filter cutoff follows $f = 8.1758 \cdot 2^{s/12}$ Hz: stored 66.86 shows about 390 Hz. | 1.0.7 | verified | probe: a UI edit read back from the saved file |
| An LFO tempo menu stores the index of its label: `2/1` is 5 and `1/4` is 8. | 1.0.7 | verified | probe: a UI edit read back from the saved file |
| The macOS bundles of 1.0.7 are x86_64 only, checked with `lipo -archs` on 2026-10-26. A native arm64 plugin host cannot load them; the standalone app runs under Rosetta. | 1.0.7 | verified | probe 1 |
| `delay_tempo` and `delay_aux_tempo` show the menu label at the stored value itself. Value 4 is `4/1`. The `vita` binding counts from the range start instead and shows `Freeze`. | source 1.0.6 | checked | `synth_slider.cpp`; the binding differs |
| `filter_*_style` has values 0 to 9 and five labels. | source 1.0.6 and open engine | verified | probe: control details |
| `osc_*_destination` and `sample_destination` allow 0 to 14, but only 0 to 13 have labels. The open engine reports 13 as the top. | source 1.0.6 | checked | `synth_parameters.cpp`; the binding differs |
| An oscillator level is the square of its stored value: $g = s^2$. | open engine | verified | render: halving the stored level quarters the amplitude |
| The outer unison offset is `detune_range` times `unison_detune` squared, in cents. | open engine | verified | render: range 2 and detune 10 give 200 cents |
| Unison voice offsets follow `laws::unison_offsets` for 2, 3, 4 and 8 voices at powers 0 and 1.5, within 0.02 cents. | open engine | verified | render, unit test `offsets_match_the_render` |
| Unison gains follow `laws::unison_gains`. A gain belongs to a pair of voices, which share it equally. | open engine | verified | render, unit test `gains_match_the_render` |
| The voice gain is $(e \cdot \mathrm{lerp}(1, v, t) \cdot a)^2$, with envelope $e$, velocity $v$, velocity track $t$ and `voice_amplitude` $a$. | open engine | verified | render, unit test `the_amp_gain_squares_like_the_render` |
| Laws tagged `interim` in the registry: resonance, drive, filter shape, distortion, compressor, EQ, effect feedback, depth and mix, reverb, oscillator warp, curve powers, and the controls with no display unit. | source 1.0.6 | checked | `src/preset/vital/registry.rs`; each carries a tuning note |

The interim laws wait for the render calibration. They are not settled
here.

## Modulation, LFO shapes, wavetables and names

| Fact | Build | Status | Source |
| :-- | :-- | :-- | :-- |
| `modulations` holds 64 `{source, destination}` entries. A slot's amount, bipolar flag, power, stereo and bypass are the controls `modulation_N_*`. | open engine | verified | probe: a fresh engine's saved state |
| The 32 source names. A route from each of them loads. | open engine | verified | load check: the every-source variant |
| A route amount is a fraction of the destination's stored range: $\Delta = (x - 0.5 b) \cdot \mathrm{amount} \cdot (\max - \min)$. | source 1.0.6 | checked | `laws::modulation_delta` |
| A Wave Source holds `name`, `author`, `version`, `remove_all_dc`, `full_normalize` and `groups`. A component holds `type`, `interpolation`, `interpolation_style` and `keyframes`. A keyframe holds `position` and `wave_data`. | open engine | verified | probe 9: the engine's saved state has the same fields as ours |
| `wave_data` is base64 of 2048 little-endian 32-bit floats: 10924 characters. Positions run from 0 to 256. | source 1.0.6 | checked | `wave_source.cpp` |
| Tables of 1 and of 257 keyframes load. | open engine | verified | load check: the wavetable variants |
| An LFO shape holds `name`, `num_points`, `points`, `powers` and `smooth`, with one power per point. A shape of 100 points loads. | open engine | verified | probe 9 and the 100-point variant |
| How `effect_chain_order` encodes a permutation. The writer refuses every code but 0. | 1.0.7 | open | probe 10, in the follow-up check |

## Fixtures

The fixtures in `tests/fixtures/vital/` are generated by
`cargo xtask vital-fixtures` and listed with their SHA-256 in
`tests/fixtures/PROVENANCE.md`. `cargo xtask vital-fixtures --check`
checks them.

| File | Holds | Tests |
| :-- | :-- | :-- |
| `minimal.vital` | the stamp and the three sample fields | `every_fixture_meets_the_format_contract` |
| `marks.vital` | every value `preset inspect` marks | the `preset-inspect` command cases |
| `routes.vital` | three routes, an LFO shape, a two-frame wavetable | `the_routes_fixture_holds_its_generated_values`, `the_routes_wavetable_has_two_full_frames` |
| `stamps/*.vital` | the version-gate set | `the_stamps_pass_the_version_gate` |

The round trip read, write, read is checked for every fixture by
`every_fixture_meets_the_format_contract`.

## Limits

| Fact | Build | Status | Source |
| :-- | :-- | :-- | :-- |
| The reader refuses files over 64 MiB and nesting over 128 levels. | this project | verified | unit tests `the_size_cap_applies_to_both_reads`, `deep_nesting_names_the_depth_limit` |
| Three tables of 257 keyframes come to 8,422,404 bytes of base64. A sample adds more. | source 1.0.6 | checked | `wave_source.cpp` |
| Parsing takes about 3 times the file size for long texts and up to about 38 times for short numbers. | this project | verified | measured in a release build |
| Ten minutes of fuzzing per target found no crash. | this project | verified | `fuzz/`: 5.4 million reads, 4.0 million round trips, 95 thousand writes |

## Builds seen

| Build | Seen how |
| :-- | :-- |
| source 1.0.6 | read, commit `636ca0e` |
| open engine (`vita` 0.1.0) | probes and load checks of ours |
| stable 1.0.7 | probes and load checks of ours |
| 1.5.3, 1.5.5 | sampled presets |
| early access 1.6.4 | offered on the download page, not checked |

## Terms

Factory presets, samples and wavetables are not redistributable, and no
file here holds them. The source is GPL-3.0-or-later. This page states
facts in our own words and copies no code, comment or table.

## Open items

Stable 1.0.7 loaded a written preset with its edits, refused the newer
stamps and re-saved the preset with every control kept. The ignored test
`a_preset_saved_by_vital_keeps_the_edits` reads that re-save from
`VITAL_SAVED_PRESET` and passes. The rest is tracked in
[issue 233](https://github.com/Akurganow/sound-to-patch/issues/233):

- whether a bare preset JSON loads as plugin state in an x86_64 host;
- how `effect_chain_order` encodes a permutation;
- the load of the route, LFO-shape, wavetable, unknown-key and no-name
  variants in 1.0.7;
- optionally, the stamps and the retail-only keys of early access 1.6.4.

## Sources

- Vital source, <https://github.com/mtytel/vital>, GPL-3.0-or-later, facts
  only, commit `636ca0e`.
