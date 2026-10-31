# Layer report

`analyze` writes `layers.json`, the report that an agent reads before it
writes an edit list. This page lists the fields of version `layers/1`. The
committed JSON Schema,
[`schemas/layers.schema.json`](https://github.com/Akurganow/sound-to-patch/blob/main/schemas/layers.schema.json),
holds the same contract, and the test suite validates every generated
report against it.

## Top level

| Field | Value |
| :-- | :-- |
| `schema` | `"layers/1"`. |
| `source` | The file name of the stem, without its folder. A run of another stem into the same folder stops unless `--force` is given. |
| `synth` | The `--synth` value, `"vital"` or `"serum2"`. Later mappers pick the synth from it; the layer fields stay synth-neutral. |
| `stem` | The stem group below. |
| `transcription` | The transcriber's model and settings, the group below. `null` with `--notes`, which skips the transcriber. |
| `pitch` | The pitch layer group below: glides, pitch drops and vibrato. |
| `envelope` | The envelope layer group below: the amplitude envelope of the notes and the sidechain pump. |
| `conventions` | The convention of every number in `stem` and of every record of the layer groups, keyed by its field name, plus `frame_size`, `statistics`, `spectral` and `confidence`. |

The top level takes no other field. Each layer adds one group with
`values`, `confidence`, `flags`, `needs_render` and `suggested_edits`, and
extends the schema in the same change. A group with too little evidence
writes `values: null` and says why in its confidence. `values` holds plain
numbers with the unit in the field name. `suggested_edits` is a list of
records in the edit-list format, each a `param` or a `route` with unit
strings, so a route keeps its source, amount and polarity; the mapping
step fills it in a later release, and until then the list is empty.

The pitch group breaks reports of `layers/1` from releases up to 0.23:
`suggested_edits` changed from an object of numbers to a list of records,
`pitch` is a new required top-level key, and `conventions` requires the
keys `pitch`, `glides`, `drops`, `sub_band_line` and `vibrato`. A reader of
older reports keeps their schema.

The envelope group breaks reports of `layers/1` from releases up to 0.25:
`envelope` is a new required top-level key, and `conventions` requires the
keys `envelope`, `note_off`, `level_spread_db` and `pump`.

## The transcription group

The group records what produced the notes, so a note that flips between
two machines can be traced to its settings.

| Field | Value |
| :-- | :-- |
| `model_sha256` | The SHA-256 of the bundled model weights, 64 lowercase hex digits. |
| `crate_version` | The version of the transcription crate, `"0.1.0"`. |
| `profile` | One of `"bass"`, `"lead"`, `"pad"` and `"detected"`, the last when the stem chose the settings. |
| `note_params` | Every decoder parameter by the crate's own name: `onset_threshold`, `frame_threshold`, `minimum_note_length_ms`, `minimum_frequency`, `maximum_frequency` (both in Hz), `infer_onsets`, `melodia_trick`, `include_pitch_bends`, `energy_tolerance` and `bend_offset`. Numbers and booleans only. |

## Naming

- Field names are snake_case ASCII.
- A field with a unit carries it as a suffix: `_hz`, `_s`, `_ms`, `_db`,
  `_dbfs`, `_deg`, `_st` (semitones) or `_ct` (cents). A field without a
  suffix is a count, a ratio, a share from 0 to 1, a power, a flag or a
  tempo in BPM.
- Integers are written as integers; every other number is a float.

## Value objects

A single estimate is `{"value": ..., "confidence": ...}`. A statistic over
active frames is `{"median": ..., "iqr": ..., "confidence": ...}`, the
median and the interquartile range. The value, or the median and the IQR,
is `null` exactly when the confidence states no estimate.

The confidence object follows the [units page](../units.md#confidence):

```json
{"level": "high", "reasons": [{"code": "active_frames", "value": 412}], "spread": 310.0, "estimate": true}
```

- `level` is `low`, `medium` or `high`.
- `reasons` holds at least one `{code, value}` pair. The value is a count,
  a number or a flag.
- `spread` is optional, in the unit of the value: the IQR of a statistic,
  one bin width for the band edge.
- `"estimate": false` means no value. Such an object has reasons and
  neither a level nor a spread.
- An unknown field is an error.

## The stem group

Every statistic over frames, `band_share` and `band_edge_hz` need at
least 16 active frames; with fewer they give no estimate with the reason
`too_few_frames`. The ranges are those of the unit types and the schema. Each convention is
the text that `conventions` holds under the same key; the
[pipeline page](../../analysis/pipeline.md) explains the rules behind
them.

| Field | Unit | Range | Shape | Convention |
| :-- | :-- | :-- | :-- | :-- |
| `bpm` | beats per minute | 4 to 1000 | number | The tempo of the beat grid and of every later step: `bpm_refined` when its confidence is high, else `bpm_given`. |
| `bpm_given` | beats per minute | 4 to 1000 | number | Given by `--bpm`. |
| `bpm_refined` | beats per minute | 4 to 1000 | value | The tempo refined from the onsets, from 2.91 % below to 3.09 % above `bpm_given`, in 0.01 BPM steps; see the [tempo section](../../analysis/pipeline.md#tempo). Its reasons give `tempo_peak`, `beat_periodicity`, `sparse_periodicity`, `onset_sharpness`, `onsets_per_beat` and the whole beats of the stem as `cycles_in_span`. No value with `--bpm-exact` (`bpm_exact`), for a stem of fewer than 8 beats or for one without onsets. |
| `duration_s` | seconds | 0 or more | number | Decoded frames over the sample rate. |
| `sample_rate_hz` | hertz | 1 or more | integer | The native rate of the file; no resampling. |
| `band_edge_hz` | hertz | 0 to Nyquist | value | The steepest fall of at least 25 dB per semitone above 2 kHz on a plateau, else the highest bin 30 dB above the floor at low confidence. |
| `hard_limited` | flag | | boolean | Flat runs of at least 1 ms within 1e-5 of the channel peak, entered and left from at least half the peak, cover 0.1 % of a channel. |
| `peak_dbfs` | dB re full scale 1.0 | 0 or less for unclipped audio | value | Peak of the mono mix over the whole stem. |
| `rms_dbfs` | dB re full scale 1.0 | below `peak_dbfs` | value | RMS of the mono mix over the whole stem; a full-scale sine reads -3.01 dB. |
| `crest_db` | dB | 0 or more | value | `peak_dbfs - rms_dbfs`. |
| `centroid_hz` | hertz | 0 to Nyquist | statistic | Magnitude-weighted mean frequency per active frame. |
| `centroid_over_f0` | ratio | 0 or more | statistic or `null` | The centroid over the median f0 of the note list; `null` without `--notes`. |
| `spread_hz` | hertz | 0 or more | statistic | Magnitude-weighted standard deviation around the centroid. |
| `rolloff85_hz` | hertz | 0 to Nyquist | statistic | Frequency below which 85 % of the frame's magnitude lies. |
| `rolloff95_hz` | hertz | 0 to Nyquist | statistic | The same at 95 %. |
| `flatness` | ratio | 0 to 1 | statistic | Geometric over arithmetic mean of the magnitude, 100 Hz to 10 kHz, floored at 1e-5. |
| `band_share` | share | each 0 to 1, sum 1 | value with a list | Power per band over active frames divided by the power in the bands reported; each item is `{lo_hz, hi_hz, share}` for the bands 20, 60, 250 Hz, 1, 4, 10 and 20 kHz. A band at or above the Nyquist frequency is left out, and one that crosses it ends there. |
| `stereo` | | | object | See below. |

`stereo` holds `full`, `sub` (below 120 Hz), `low_mid` (120 Hz to 1 kHz)
and `high` (above 1 kHz), each `{rho, sm_db, ipd_deg}`, and one
confidence for the screen:

| Field | Unit | Range | Convention |
| :-- | :-- | :-- | :-- |
| `rho` | correlation | -1 to 1 | Pearson correlation of L and R at zero lag; per band from the 8192-point STFT. |
| `sm_db` | dB | any | `20 log10(rms(S) / rms(M))` with `M = (L + R) / 2`, `S = (L - R) / 2`. |
| `ipd_deg` | degrees | 0 to 180 | The magnitude-weighted phase difference of L and R at harmonics 1 to 20 of the median f0, averaged over active frames; `null` without `--notes`. |

A stereo value is `null` when it does not exist: a one-channel file, a
silent channel, or a zero mid or side.

## The pitch group

The pitch layer of a monophonic line; the
[pitch layer page](../../analysis/pitch.md) explains each method. Every
record carries its own confidence, always an estimate. The group's own
confidence is medium, or low when fewer than half of the track frames
inside the notes are voiced within 100 cents of a sounding note, as under
noise or DC. Its reasons are `voiced_frames`, the voiced frames of the f0
track, and `note_frames`, that share.

`values` is `null` when the line plays chords (reason `polyphonic_line`)
or the stem has no notes (reason `no_note_list`). Otherwise it holds three
lists, each in time order and each empty when nothing was found:
`glides`, `drops` and `vibrato`. Every time is audio time from the start
of the stem.

A glide:

| Field | Unit | Range | Value |
| :-- | :-- | :-- | :-- |
| `start_s` | seconds | 0 or more | The fitted start of the motion. |
| `interval_st` | semitones | any | From the start pitch to the end pitch; negative for a glide down. |
| `time_s` | seconds | 0.09 or more | The full 0 to 100 % time. |
| `curve` | power | above 0.21, at most 5 | `p` of `s(x) = 1 - (1 - x)^p`: above 1 starts fast, 1 is linear in pitch. A fit within 5 % of the lower bound 0.2 is no glide; one within 5 % of the upper bound 5, such as an exponential portamento, needs a detected span of at most 2.5 times its fitted time, and reads low with `curve_bound`. |
| `from_silence` | flag | | The glide leads into the first stable pitch of a voiced run whose sound starts up to 100 ms before it. That stable pitch has a median level within 40 dB of the loudest frame, and the pitch does not swing back half the lead within 167 ms after it, unless the run's next stable pitch lies 100 cents or more away and matches another note, and the pitch, once past half the way to it, stays there up to that stable pitch and to the window's end. |

Its reasons are `motion_span` (seconds), `fit_residual` (cents),
`pitch_jitter` (cents) and `curve_bound` (flag). A glide whose
`motion_span` is under 0.09 reads low.

A drop:

| Field | Unit | Range | Value |
| :-- | :-- | :-- | :-- |
| `onset_s` | seconds | 0 or more | The onset of the strike from the band envelope. |
| `f_end_hz` | hertz | above 0 | The settled frequency. |
| `df_st` | semitones | above 2, at most 36 | The drop at the onset, `12 log2((f_end + df) / f_end)`. |
| `df_hz` | hertz | above 0 | The same in hertz above `f_end_hz`. |
| `tau_s` | seconds | above 0.003 | The time constant of `f_end + df exp(-t / tau)`, above the 3.04 ms floor of the search. |
| `residual` | share | 0 to 0.15 | The RMS residual of the fit over the fall it shows from 7 ms on, `df_hz exp(-7 ms / tau_s)`; with `tau_s` of 0.01 or more, of the error averaged over half a period of `f_end_hz`, or over a full period when `tau_s` spans 1.5 periods or more. |

Its reasons are `fit_residual` and `attack_time` (seconds): the time the
band envelope of the strike takes from 10 % to 90 % of its peak. A drop
with an attack over 30 ms reads low, and one with `tau_s` under 0.01 or an
attack over 12 ms at most medium.

A vibrato:

| Field | Unit | Range | Value |
| :-- | :-- | :-- | :-- |
| `start_s`, `end_s` | seconds | 0 or more | The first and last frame read. |
| `rate_hz` | hertz | 2.97 to 12.12 | The rate, 3 to 12 Hz with 1 % slack. |
| `extent_ct` | cents | 10 or more | Half the peak-to-peak swing, after the correction. |
| `extent_raw_ct` | cents | 0 or more | The same as the f0 track reads it. |
| `extent_factor` | ratio | above 0 | The correction for the track's smoothing at this rate; `extent_ct = extent_raw_ct * extent_factor`. |
| `share` | share | 0.5 to 1 | The power the fitted sine explains over the power left after the line alone. |

Its reasons are `sine_share`, `cycles_in_span` and `contour_kurtosis`. A
vibrato holds 2.5 cycles or more; from 4 it reads high. A kurtosis below
1.25 reads low, since the contour jumps between two levels, as a trill
does, and so does a vibrato on a pitch below 100 Hz.

`flags` holds four booleans: `chord_skipped`, `glide_too_short` (a motion
between two notes lasted 35 to 90 ms over its fitted time, or over its
detected span when the fit failed), `weak_fundamental` (a low note's
second harmonic exceeded its fundamental by 6 dB, so its drop was not
reported) and `no_notes`. At a sample rate under about 4 times a note's
pitch the second harmonic cannot be measured, and that note is not
tested for `weak_fundamental`.
`needs_render` is `false`.

## The envelope group

The amplitude envelope over the pooled notes, each read note's record, and
the sidechain pump; the
[envelope layer page](../../analysis/envelope.md) explains each method.
`values` is `null` when no note was read: the stem has no notes (reason
`no_note_list`) or every note was skipped (reason `too_few_notes`). The
group's own confidence is medium with 3 or more pooled notes, else low.

| Field | Unit | Range | Value |
| :-- | :-- | :-- | :-- |
| `attack_s` | seconds | 0 or more | Statistic: 1.25 times the 10 to 90 % rise of each pooled note to the attack's top, its full time for a linear ramp. An upper bound when `attack_is_upper_bound` is set. |
| `hold_s` | seconds | 0 or more | Statistic: the time at the peak before the decay, 0 under 10 ms. |
| `decay_s` | seconds | 0 or more | Statistic: the fall from the peak to the sustain level, from the 10 and 90 % shares of the fall in dB. |
| `sustain_db` | dB | 0 or less | Statistic: the level of the last 30 % of the held part, from the peak. |
| `note_off_offset_s` | seconds | any | Statistic: the note's end in the note list minus its note-off knee, over the pooled notes with a knee. |
| `level_spread_db` | dB | 0 or more | Value: the 90th minus the 10th percentile of the pooled peak levels; always low. |
| `ripple_db` | dB | 0 or more | The median ripple of the sustain window after its line; `null` without a sustain. |
| `fit_residual_db` | dB | 0 or more | The median RMS of the held parts against the hold, decay and sustain model. |
| `n_notes_used` | count | 0 or more | The pooled notes. |
| `overlap_fraction` | share | 0 to 1 | The share of the notes in the list that overlap another. |
| `pump_depth_db` | dB | 0 or more | Minus the minimum of the pump curve; `null` with fewer than 4 counted beats. |
| `pump_recovery_s` | seconds | 0 or more | From the curve's minimum back to within 1.5 dB, over the beat's wrap; `null` when it does not within a beat. |
| `pump_tau_ms` | milliseconds | 0 or more | `pump_recovery_s` over `ln(pump_depth_db / 1.5)`, the one time field in milliseconds; `null` unless the pump locks to the beat. |
| `pump_locked_to` | | | `beat`, `note`, `free` or `none`; `null` without a pump. |
| `pump_beats` | count | 0 or more | The beats behind the curve. |
| `pump_bpm` | BPM | above 0 | The tempo the curve was read at: `bpm` for a read that stays on the grid, or a fitted tempo up to about 2.7 % from it when that read replaced it. A sidechain's rate follows a fitted tempo closely. A read that stays on the grid says only that the duck locks there: up to 2.56 % off on generated ducks, as [the envelope page](../../analysis/envelope.md#limits) says. `null` without a pump. |
| `notes` | | | One record per read note, in note order. |

A statistic reads low over fewer than 3 notes or with an interquartile
range above half its median. Its reasons are `too_few_notes` and
`relative_spread`; `attack_s` adds `upper_bound_share`, the share of the
pooled attacks that are upper bounds. A median of 0 under an interquartile
range above 0 has no ratio, so the statistic reads low and writes
`zero_median`, that range in the statistic's unit, in place of
`relative_spread`.

A note record:

| Field | Unit | Range | Value |
| :-- | :-- | :-- | :-- |
| `onset_s` | seconds | 0 or more | The onset in the note list. |
| `peak_db` | dB | any | The peak power of the note's amp proxy, the one harmonic it reads; two notes compare directly when they read the same harmonic. |
| `attack_s` | seconds | 0 or more | The note's attack, as above; `null` when the rise is not read. |
| `attack_upper_bound` | flag | | The note's attack is an upper bound. |
| `knee_s` | seconds | 0 or more | The note-off knee; `null` for a note that decays at one rate. |
| `pooled` | flag | | The pool holds the note. |

`flags` holds three booleans: `no_notes`, `attack_is_upper_bound` (more
than half the pooled attacks are upper bounds, so a preset should not
take the attack) and `pump_stripped` (the pump locks to the beat over 8
counted beats or more, and its curve was taken out of the note tracks).
`needs_render` is `false`.

## Conventions

| Key | Holds |
| :-- | :-- |
| `frame_size` | The STFT frame length of the descriptors and the band edge: 8192 when the note list has a median f0 below 100 Hz, else 4096, and 4096 without a note list. |
| `statistics` | Median and IQR over active frames, with linear-interpolation quantiles; the active-frame gate. |
| `spectral` | Magnitude weights for shape, power for band shares; the window and the hop. |
| `confidence` | What the level, the reasons and the spread mean. |
| every `stem` field | The convention of that field, as in the tables above. |
| `pitch` | What the pitch layer reads, when `values` is `null`, and the time base. |
| `glides`, `drops`, `vibrato` | The method and the rule of each list of the pitch group. |
| `sub_band_line` | The line the drops read. |
| `envelope` | The tracks, the amp proxy, the stages and the pool of the envelope group. |
| `note_off` | The note-off knee and `note_off_offset_s`. |
| `level_spread_db` | The level spread. |
| `pump` | The pump's track, beats, curve, fields and lock, and when it is stripped. |
