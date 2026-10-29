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
  - `untestable by rule`: a check that needs a step the project never
    takes, such as running the app.

A row verified on the open engine is not verified on retail. The pinned
retail build is stable 1.0.7. The early-access build is 1.6.4 (reported).
The checks still open are listed under
[Open items](#open-items).

## File and top-level keys

| Fact | Build | Status | Source |
| :-- | :-- | :-- | :-- |
| A preset is one compact JSON text. | source 1.0.6 | checked | `load_save.cpp` |
| The top level holds `synth_version`, `preset_name`, `author`, `comments`, `preset_style`, `macro1` to `macro4` and `settings`. | open engine, 1.0.7 | verified | probe: saved default states |
| `settings` holds 772 numeric controls and four structured parts: `sample`, `modulations`, `wavetables` and `lfos`. | open engine, 1.0.7 | verified | probe: saved default states |
| The app saves the keys of each object in alphabetical order. | 1.0.7 | verified | load check: the re-save of `Bass` |
| A re-save keeps every control within one 32-bit float step and rewrites the `version` of each wavetable to the app's build. A load and save rewrites the sample: our noise sample came back 4 frames later with some values one step off, so compare a re-saved sample by its name, length and rate, not its bytes. | 1.0.7 | verified | load check: the re-save of `Bass`; the open engine keeps the sample |
| A file without `synth_version`, `settings` or `settings.sample`, or a sample without `length`, `sample_rate` or `samples`, makes the app throw. | source 1.0.6 | checked | `load_save.cpp` |
| A missing control takes its default. | open engine | verified | probe: [`minimal.vital`](#fixtures) reads back 772 controls, each at the fresh engine's default |
| An unknown key loads and is dropped on save, in `settings` and at the top level. | open engine | verified | load check: the unknown-keys variant |
| A file without `preset_name` and `author` loads. | open engine | verified | load check: the no-name variant |
| An out-of-range control is applied as it is. The loader does not clamp. | source 1.0.6 | checked | `load_save.cpp` |
| Factory presets hold values outside the table range, such as a dry/wet above its maximum or the 32-bit float form of a bound. `preset apply` warns about such a value in a key no edit touched and refuses it in an edited key. | 1.0.x | verified | observed in factory presets of Vital 1.0.x |
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
| The 1.0.7 default state holds all 772 controls at the registry default. These carry `verified_on = 1.0.7`, which confirms the key and the default only. | 1.0.7 | verified | load check: the 1.0.7 default state |
| The open engine reports the same range, default and first build as the registry for 763 controls; the other 9 are listed below. The registry evidence says which. | open engine | verified | probe: control details |
| A filter cutoff follows $f = 8.1758 \cdot 2^{s/12}$ Hz: stored 66.86 shows about 390 Hz. | 1.0.7 | verified | probe: a UI edit read back from the saved file |
| Display laws read and typed in the value box, each matching the registry law: `portamento_time` is $2^s$ seconds (0.5 s stores -1), `osc_1_unison_detune` is $s^2$ percent (25 % stores 5), `filter_1_cutoff` shows $s - 60$ semitones (12 stores 72), `distortion_drive` is $s$ dB (12 stores 12). The registry tags these four `measured`, with `verified_on = 1.0.7`. | 1.0.7 | verified | probe: values typed in the app and read back from the saved file |
| A route amount shows in the destination's units, as the amount times the destination's stored span. On `filter_1_cutoff`, span 128, 64 semitones stores 0.5. `preset apply` converts a semitone amount the same way. | 1.0.7 | verified | probe: a value typed in the app; unit tests of the edit list |
| An LFO tempo menu stores the index of its label: `2/1` is 5 and `1/4` is 8. | 1.0.7 | verified | probe: a UI edit read back from the saved file |
| The macOS bundles of 1.0.7 are x86_64 only, checked with `lipo -archs` on 2026-10-26. A plugin host running natively on arm64 cannot load them. On Apple silicon an x86_64 app runs under Rosetta. | 1.0.7 | verified | probe 1; Rosetta is a platform fact |
| `delay_tempo` and `delay_aux_tempo` show the menu label at the stored value itself. Value 4 is `4/1`. The `vita` binding counts from the range start instead and shows `Freeze`. | source 1.0.6 | checked | `synth_slider.cpp`; the binding differs |
| `filter_*_style` has values 0 to 9 and five labels. | source 1.0.6 and open engine | verified | probe: control details |
| `osc_*_destination` and `sample_destination` allow 0 to 14, but only 0 to 13 have labels. The open engine reports 13 as the top. | source 1.0.6 | checked | `synth_parameters.cpp`; the binding differs |
| An oscillator level is the square of its stored value: $g = s^2$. | open engine | verified | render: halving the stored level quarters the amplitude |
| The outer unison offset is `detune_range` times `unison_detune` squared, in cents. | open engine | verified | render: range 2 and detune 10 give 200 cents |
| Unison voice offsets follow `laws::unison_offsets` for 4 and 8 voices at powers 0 and 1.5, within 0.03 cents. Odd counts are not rendered. | open engine | verified | render, unit test `offsets_match_the_render` |
| Unison gains follow `laws::unison_gains` for 3, 4, 5 and 8 voices, within 0.001 of the single-voice amplitude. A gain belongs to a pair of voices, which share it equally. | open engine | verified | render, unit test `gains_match_the_render` |
| The voice gain is $(e \cdot \mathrm{lerp}(1, v, t) \cdot a)^2$, with envelope $e$, velocity $v$, velocity track $t$ and `voice_amplitude` $a$. | open engine | verified | render, unit test `the_amp_gain_squares_like_the_render` |
| Laws tagged `interim` in the registry: resonance, filter drive, filter shape, distortion except `distortion_drive`, compressor, EQ, effect feedback, depth and mix, reverb, oscillator warp, curve powers, and the controls with no display unit. | source 1.0.6 | checked | `src/preset/vital/registry.rs`; each carries a tuning note |

The interim laws wait for the render calibration. They are not settled
here.

## Modulation, LFO shapes, wavetables and names

| Fact | Build | Status | Source |
| :-- | :-- | :-- | :-- |
| `modulations` holds 64 `{source, destination}` entries. A slot's amount, bipolar flag, power, stereo and bypass are the controls `modulation_N_*`. | open engine | verified | probe: a fresh engine's saved state |
| The 32 source names. A route from each of them loads. | open engine, 1.0.7 | verified | load check: the every-source variant; in 1.0.7 the matrix shows 32 rows, and the value box reads 12.8 for amount 0.1 into the cutoff |
| A route amount is a fraction of the destination's stored range: $\Delta = (x - 0.5 b) \cdot \mathrm{amount} \cdot (\max - \min)$. | source 1.0.6 | checked | `laws::modulation_delta` |
| Routes target numbers, counts, menus such as `osc_1_transpose` and `lfo_1_tempo`, and the amount or power of another slot, never a switch. `preset apply` refuses a switch and the bipolar, stereo and bypass flags of a slot. | 1.0.x | verified | observed in factory presets of Vital 1.0.x |
| A Wave Source holds `name`, `author`, `version`, `remove_all_dc`, `full_normalize` and `groups`. A component holds `type`, `interpolation`, `interpolation_style` and `keyframes`. A keyframe holds `position` and `wave_data`. | open engine | verified | probe 9: the engine's saved state has the same fields as ours |
| `wave_data` is base64 of 2048 little-endian 32-bit floats: 10924 characters. Positions run from 0 to 256. | source 1.0.6 | checked | `wave_source.cpp` |
| Tables of 1 and of 257 keyframes load. One keyframe shows a single frame; with 257 the frame position sweeps the wave. | open engine, 1.0.7 | verified | load check: the wavetable variants |
| An LFO shape holds `name`, `num_points`, `points`, `powers` and `smooth`, with one power per point. A shape of 100 points loads. | open engine, 1.0.7 | verified | probe 9 and the 100-point variant |
| A stored point y is drawn upside down: the output is $1 - y$, so a stored 0 is the top. The default triangle stores $(0, 1), (0.5, 0), (1, 1)$ and peaks at the centre. `LfoShape` takes outputs and stores $1 - y$. | 1.0.7 | verified | load check: a shape of rising stored values showed falling saws |
| `effect_chain_order` codes the effect list, top to bottom. Code 0 is Chorus, Compressor, Delay, Distortion, EQ, Filter, Flanger, Phaser, Reverb. Number the effects in that order. Each effect after the first adds a mixed-radix digit, the count of effects above it with a higher number, in base its position plus one. The first digit is the most significant: for $i$ from 1 to 8, $\mathrm{code} = \mathrm{code} \cdot (i + 1) + \mathrm{digit}_i$. The codes run from 0 to $9! - 1$. | 1.0.7 | verified | probe 10: lists rearranged in the app saved 260650, 8 and 181440; unit tests `the_effect_order_codes_match_vital` and `every_effect_order_code_round_trips` |

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
| early access 1.6.4 | reported, not checked |

## Terms

Factory presets, samples and wavetables are not redistributable, and no
file here holds them. The source is GPL-3.0-or-later. This page states
facts in our own words and copies no code, comment or table.

## Plugin state

A host loads a preset into the VST3 plugin through its state, not through a
file. The rows below come from the host spike ([issue 244](https://github.com/Akurganow/sound-to-patch/issues/244)):
the retail plugin loaded headless as a library in an x86_64 host process
through `vst3-host` 0.10.0, with no editor window. The probe output is in
[issue 10](https://github.com/Akurganow/sound-to-patch/issues/10).

| Fact | Build | Status | Source |
| :-- | :-- | :-- | :-- |
| The plugin state is the preset JSON plus a `tuning` object, followed by a NUL. | source 1.0.6 | checked | `synth_plugin.cpp` |
| The macOS bundles hold only an x86_64 binary, so a host process must run as x86_64. | 1.0.7 | verified | `lipo -archs` of the bundle |
| The plugin loads and renders with no editor window. The probe process wrote nothing to standard error and exited with code 0. | 1.0.7 | verified | host spike: `minimal.vital` named `SpikeProbe` with `env_1_attack` 1, `filter_1_on` 1 and `filter_1_cutoff` 50, MIDI 60 at velocity 100 |
| The single audio output bus has 2 channels and reports itself inactive after the load. With the bus left inactive, the render was silent; after the host activated it, the render sounded. | 1.0.7 | verified | host spike: `minimal.vital` named `SpikeProbe` with `env_1_attack` 1, `filter_1_on` 1 and `filter_1_cutoff` 50, MIDI 60 at velocity 100; default patch |
| The saved component state starts with `VstW`, then `CcnK` at byte 16. The preset JSON starts at byte 176 and is followed by a NUL, zero bytes and the text `JUCEPrivateData`. The JSON holds `tuning` and the app's stamp `1.0.7`. The plugin saves no controller state. | 1.0.7 | verified | host spike: `minimal.vital` named `SpikeProbe` with `env_1_attack` 1, `filter_1_on` 1 and `filter_1_cutoff` 50, MIDI 60 at velocity 100; state read-back |
| A bare preset JSON passed to `IComponent::setState` loads, with or without a trailing NUL. The saved state then holds the name `SpikeProbe` and the key `filter_1_cutoff`. | 1.0.7 | verified | host spike: `minimal.vital` named `SpikeProbe` with `env_1_attack` 1, `filter_1_on` 1 and `filter_1_cutoff` 50, MIDI 60 at velocity 100; state read-back |
| A host needs no `VC2!` envelope, the form some Python hosts save: the bare JSON loads. | 1.0.7 | verified | host spike: `minimal.vital` named `SpikeProbe` with `env_1_attack` 1, `filter_1_on` 1 and `filter_1_cutoff` 50, MIDI 60 at velocity 100 |
| A state without `tuning` keeps the instance's current tuning: `setStateInformation` loads `tuning` only when the key is there. The app reads no `tuning` from a preset file. | source 1.0.6 | checked | `synth_plugin.cpp`, `synth_base.cpp` |
| `Tuning::jsonToState` reads `scale_start_midi_note`, `reference_midi_note`, `tuning_name`, `mapping_name` and `scale` without a check, and `default` and `mapping` when present. A missing key or a wrong type throws, and the plugin then shows a dialog box. The default tuning starts the scale at MIDI 60 with reference 0 and the scale 0 to 12. | source 1.0.6 | checked | `tuning.cpp`, `synth_plugin.cpp` |
| In one plugin instance, a preset with `reference_midi_note` 2 played MIDI 60 at 293.66 Hz. A preset without `tuning` loaded next still played at 293.66 Hz. With the default tuning added to its state, it played at 261.63 Hz. | 1.0.7 | verified | `render_vital::a_tuning_does_not_carry_over_to_the_next_preset`: `minimal.vital`, the lead material at 120 BPM |
| In a host, the plugin sets the engine's tempo from the host's tempo on every block, so `beats_per_minute`, in beats per second, follows the host whatever the preset holds. `SoundEngine::setBpm` divides the tempo by 60 in 32-bit floats. | source 1.0.6 | checked | `synth_plugin.cpp`, `sound_engine.cpp` `SoundEngine::setBpm` |
| The plugin stores `f32(bpm) * f32(1 / 60)`, a multiply by the reciprocal: at 140 BPM it saves 2.3333334922790527, and at 100.3 BPM a value two `f32` steps from the correctly rounded 100.3 / 60. A preset with `beats_per_minute` 2 rendered at another tempo saves that value. | 1.0.7 | verified | `render_vital::a_render_at_another_tempo_passes_the_load_check` at 140 and 100.3 BPM, and `the_saved_tempo_follows_the_model_over_a_sweep`: every whole tempo 4 to 1000, every tenth 4.0 to 400.0 and 600 random tempos on 0.01 and 0.001 steps |
| A loaded preset renders unlike the default patch: RMS 0.017 for the probe preset against 0.118 for the default patch. | 1.0.7 | verified | host spike: `minimal.vital` named `SpikeProbe` with `env_1_attack` 1, `filter_1_on` 1 and `filter_1_cutoff` 50, MIDI 60 at velocity 100; default patch |
| Two renders of one patch differ sample by sample. | 1.0.7 | verified | host spike: `minimal.vital` named `SpikeProbe` with `env_1_attack` 1, `filter_1_on` 1 and `filter_1_cutoff` 50, MIDI 60 at velocity 100; default patch |
| In one probe, offline against realtime processing changed the render by less than two repeat renders differ: a difference RMS of 0.0100 against 0.0132. | 1.0.7 | verified | host spike: `minimal.vital` named `SpikeProbe` with `env_1_attack` 1, `filter_1_on` 1 and `filter_1_cutoff` 50, MIDI 60 at velocity 100 |
| Whether the block size changes the render by more than repeat renders differ. In one probe, blocks of 64 against 512 samples gave a difference RMS of 0.0235, against 0.0132 between two repeat renders. The 64-sample render ran in a second instance. | 1.0.7 | open | host spike: `minimal.vital` named `SpikeProbe` with `env_1_attack` 1, `filter_1_on` 1 and `filter_1_cutoff` 50, MIDI 60 at velocity 100 |
| `IMidiMapping` returns a parameter for CC 1, CC 7, CC 64, channel aftertouch and pitch bend. A full bend up raised MIDI 60 by 2 semitones. | 1.0.7 | verified | host spike: default patch, MIDI 60 with and without a full bend; test `a_full_pitch_bend_raises_the_note_two_semitones` |
| CC 1 from the host moves a route from `mod_wheel`: with the route at -1 on `osc_1_level`, a full wheel lowered the level. | 1.0.7 | verified | test `the_mod_wheel_reaches_a_route` |
| A state stamped `1.7.0` loads nothing. The plug-in keeps its init patch and saves the init patch back. | 1.0.7 | verified | test `a_newer_stamp_falls_back_to_init` |
| A `settings` key that Vital does not know is gone from the state it saves after the load. | 1.0.7 | verified | test `a_key_vital_drops_is_a_readback_mismatch` |
| After a load, the saved state holds every control and the name as sent, a re-encoded `sample.samples` text and a new `version` in each wavetable. | 1.0.7 | verified | `render` of `routes.vital` |
| In one probe, a note right after the state load differed from a note after 0.5 s of silence by less than two repeat renders differ. | 1.0.7 | verified | host spike: `minimal.vital` named `SpikeProbe` with `env_1_attack` 1, `filter_1_on` 1 and `filter_1_cutoff` 50, MIDI 60 at velocity 100 |
| A second instance in the same process, with blocks of 64 samples, loaded the probe preset and rendered it. | 1.0.7 | verified | host spike: `minimal.vital` named `SpikeProbe` with `env_1_attack` 1, `filter_1_on` 1 and `filter_1_cutoff` 50, MIDI 60 at velocity 100 |
| The standalone app's `--headless --render` flags. | 1.0.7 | untestable by rule | the project never runs the app |

## Open items

Stable 1.0.7 loaded a written preset with its edits, refused the newer
stamps and re-saved the preset with every control kept. The ignored test
`a_preset_saved_by_vital_keeps_the_edits` reads that re-save from
`VITAL_SAVED_PRESET` and passes. The unknown-key, no-name, every-source,
LFO-shape and wavetable variants loaded in 1.0.7 with no dialog.

A bare preset JSON loads as plugin state in an x86_64 host (see
[Plugin state](#plugin-state)).

Still open:

- optionally, the stamps and the retail-only keys of early access 1.6.4.
- whether the block size changes a render by more than repeat renders
  differ. The recipe fixes blocks of 512 samples, so renders compare at one
  block size.

## Sources

- Vital source, <https://github.com/mtytel/vital>, GPL-3.0-or-later, facts
  only, commit `636ca0e`.
