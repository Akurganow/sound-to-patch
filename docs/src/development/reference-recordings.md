# Reference recordings

Reference recordings check the automatic render against a render made
elsewhere. Each one renders a generated Vital preset and MIDI file in a
standard DAW host, on the retail Vital 1.0.7, exported as 32-bit float
stereo at 44.1 kHz with its tail. Our host draws 20 renders of the same
preset, MIDI file and tempo, in sequence on one fresh plug-in instance.
One gain per recording matches its level to those renders. The
comparison then asks whether the recording's shape lies as close to the
renders as they lie to each other: its peak-to-RMS ratio, its duration
to the tail floor and its envelope. The
[testing page](testing.md#reference-recordings) gives the method, and the
[render spread page](../reference/formats/render-spread.md) the outputs,
the gain fit, the tolerance and the floor.

The recordings and the renders stay local. This page and the report next
to it hold the preset ids, the hashes of the preset, the MIDI file and
the recording, and the metrics.
[`report.json`](reference-recordings/report.json) holds, per recording,
the fitted gain, the comparison after it and the comparison as recorded.

38 recordings cover isolated controls, musical presets of each role,
look-alike pairs and complex presets, at 120 and 140 BPM. For the 34 whose
DAW exported the state it played, the exported state matches the preset
on every control, at the recording's tempo.

## Result

The acceptance of #256 asks every recording to match after its gain, so
it is not met. A level difference within ±3 dB is no fault: `compare`
matches the loudness of two sounds before its distances. The comparison
as recorded is for information only.

- After the gain fit, 12 of 38 recordings match the renders on all three
  outputs of the sound's shape: the peak-to-RMS ratio on 37, the
  duration on 14 and the envelope on 31.
- As recorded, for information, 0 of 38 lie within the limit on all five
  outputs: the RMS level on 0, the peak on 1, the peak-to-RMS ratio on
  37, the duration on 12 and the envelope on 0.
- The fitted gains lie from +1.91 to +2.43 dB, with a median of +2.17
  dB. None lies beyond ±3 dB.

Every gain is positive: each recording is quieter than the renders, by
the amounts above. The cause is not known, on either side: our host's
gain staging and export are as much in question as the DAW's.

## What remains unexplained

Nothing was tuned to make a recording match.

- Duration. A clip ends at its last sample above -96 dBFS. The gain fit
  raises each recording before its end is measured, so the level offset
  does not explain an early end.
  - 21 recordings end earlier than the renders, beyond the limit, and 3
    end later.
  - 14 of the earlier ones end 0.1 s or more early, from 0.22 to 1.68 s.
    The other 7 end 0.5 to 53.8 ms early.
  - The later ones end late by 44.2 ms (`cplx_22__pad_chords_120bpm`),
    40.9 ms (`mus_sub_bass_00__bass_octave_120bpm`) and 21.7 ms
    (`cplx_17__arp_120bpm`).
- 19 recordings miss on the duration alone.
- 7 recordings lie beyond the limit on the envelope, at ratios from 1.02
  to 2.96. 5 of them also lie beyond it on the duration.
- The peak-to-RMS ratio lies beyond the limit on 1:
  `iso_porta_force_0.1__chord_long_C4maj_v102`, at a ratio of 1.98, and
  2.96 on the envelope.

In the recordings that end 0.1 s or more early, the tail falls below -96
dBFS sooner than in the renders, and the cause is open. A calibration
recording, signals of known level and decay sent through the DAW chain
of the references and through our host, can tell whether the DAW chain
cuts or shapes a tail.

## The draws

The renders of each recording also check the tolerance and the draws.

- Each render, held out in turn and compared with the other 19 as a
  clip, lies within their limit at these rates over the 760 cases: 95.1
  % on the RMS level, 95.1 % on the peak, 94.6 % on the peak-to-RMS
  ratio, 95.0 % on the duration and 95.5 % on the envelope. The rank of
  19 renders covers 19 in 20, 95.0 %.
- The lag-1 autocorrelation of each set of 20 has a median of -0.17 on
  the RMS level (-0.41 to 0.69), -0.05 on the peak (-0.49 to 0.48),
  -0.05 on the peak-to-RMS ratio (-0.48 to 0.48) and -0.06 on the
  duration (-0.50 to 0.40).
- The adjacent ratio has a median of 1.09 on the RMS level (0.44 to
  1.66), 1.01 on the peak (0.52 to 1.36), 1.03 on the peak-to-RMS ratio
  (0.50 to 1.53), 0.97 on the duration (0.28 to 1.45) and 1.01 on the
  envelope (0.89 to 1.30).

The lag-1 autocorrelation of independent draws lies near -0.05, within
about ±0.45 for 20 renders, so these medians show no drift over a
sequence.

## Each recording

The gain is the fitted gain in dB. The duration difference is the
recording's duration after the gain minus the median of the renders, in
ms. Each ratio is the difference over its limit: above 1 lies beyond the
limit. The first three ratios are the verdict after the gain, the last
five the comparison as recorded.

| Id | BPM | Gain (dB) | Duration difference (ms) | Peak-to-RMS | Duration | Envelope | RMS, as recorded | Peak, as recorded | Peak-to-RMS, as recorded | Duration, as recorded | Envelope, as recorded |
| :-- | --: | --: | --: | --: | --: | --: | --: | --: | --: | --: | --: |
| `iso_init__chord_long_C4maj_v102` | 120 | +2.20 | +0.3 | 0.80 | 0.44 | 0.77 | 41.72 | 3.91 | 0.80 | 0.44 | 1.82 |
| `iso_uni_v7_det40__chord_long_C4maj_v102` | 120 | +2.16 | -0.1 | 0.21 | 0.36 | 0.95 | 52.55 | 2.06 | 0.21 | 0.36 | 1.67 |
| `iso_chorus_mix100__chord_long_C4maj_v102` | 120 | +2.20 | -18.1 | 0.41 | 1.33 | 0.89 | 35.56 | 2.06 | 0.41 | 1.76 | 2.81 |
| `iso_amp_rel1__chord_short_C4maj_v102` | 120 | +2.34 | +2.6 | 0.39 | 0.93 | 1.03 | 7.39 | 1.64 | 0.39 | 1.88 | 1.96 |
| `iso_reverb_d1__chord_short_C4maj_v102` | 120 | +2.32 | -22.0 | 0.36 | 0.50 | 0.70 | 4.64 | 1.65 | 0.36 | 2.63 | 1.11 |
| `iso_lfo_amp_1_8__chord_long_C4maj_v102` | 120 | +2.17 | +0.2 | 0.24 | 0.25 | 0.84 | 28.80 | 15.47 | 0.24 | 0.30 | 1.72 |
| `iso_comp_fast__chord_long_C4maj_v102` | 120 | +2.17 | -2.2 | 0.66 | 0.73 | 0.93 | 38.68 | 2.97 | 0.66 | 0.77 | 3.47 |
| `iso_dist_soft_12db__chord_long_C4maj_v102` | 120 | +2.18 | -0.5 | 0.38 | 1.16 | 0.85 | 183.43 | 8.38 | 0.38 | 1.37 | 5.97 |
| `iso_delay_1_8_fb50__chord_short_C4maj_v102` | 120 | +2.43 | -1683.4 | 0.43 | 187.46 | 0.88 | 7.49 | 1.94 | 0.43 | 187.48 | 1.91 |
| `mus_pad_00__chord_long_C4maj_v102` | 120 | +2.00 | -1062.4 | 0.34 | 4.22 | 1.02 | 3.24 | 1.41 | 0.34 | 4.22 | 1.11 |
| `mus_supersaw_00__chord_long_C4maj_v102` | 120 | +2.10 | -707.3 | 0.48 | 5.53 | 0.96 | 19.80 | 2.58 | 0.48 | 5.56 | 2.32 |
| `mus_keys_00__phrase_pad_chords_120bpm_v102` | 120 | +2.18 | -40.4 | 0.24 | 0.34 | 0.94 | 4.78 | 1.29 | 0.24 | 0.34 | 1.68 |
| `mus_stab_00__chord_short_C4maj_v102` | 120 | +2.14 | -238.8 | 0.46 | 4.05 | 0.75 | 5.85 | 2.75 | 0.46 | 4.18 | 1.35 |
| `mus_pluck_00__strum_120bpm` | 120 | +2.15 | -606.0 | 0.34 | 53.34 | 0.98 | 10.28 | 1.34 | 0.34 | 53.93 | 2.47 |
| `iso_porta_force_0.1__chord_long_C4maj_v102` | 120 | +2.16 | +0.6 | 1.98 | 0.54 | 2.96 | 216.10 | 104.93 | 1.98 | 0.50 | 14.68 |
| `mus_pad_00__pad_chords_120bpm` | 120 | +2.18 | -1202.2 | 0.46 | 5.10 | 1.03 | 4.49 | 1.55 | 0.46 | 5.12 | 1.24 |
| `mus_arp_00__arp_120bpm` | 120 | +2.20 | -782.5 | 0.44 | 5.96 | 1.04 | 11.27 | 1.10 | 0.44 | 5.98 | 3.22 |
| `mus_lead_00__lead_legato_120bpm` | 120 | +2.16 | -388.4 | 0.38 | 19.18 | 0.75 | 78.48 | 3.33 | 0.38 | 19.47 | 1.03 |
| `mus_reese_bass_00__bass_octave_120bpm` | 120 | +2.05 | -65.6 | 0.47 | 0.50 | 0.87 | 3.93 | 94.14 | 0.47 | 0.53 | 1.48 |
| `iso_porta_0.1__lead_glide_120bpm` | 120 | +2.13 | +0.2 | 0.52 | 0.38 | 0.99 | 54.97 | 1.14 | 0.52 | 0.40 | 9.91 |
| `mus_fx_00__chord_long_C4maj_v102` | 120 | +2.18 | -1253.9 | 0.38 | 5.49 | 1.13 | 9.79 | 1.70 | 0.38 | 5.52 | 1.96 |
| `conf_cutoff_vs_wavetable_v1_a__chord_long_C4maj_v102` | 120 | +2.15 | +0.2 | 0.47 | 0.32 | 0.94 | 72.93 | 16.82 | 0.47 | 0.30 | 1.54 |
| `conf_cutoff_vs_wavetable_v1_b__chord_long_C4maj_v102` | 120 | +2.17 | -0.2 | 0.59 | 0.21 | 0.95 | 217.37 | 109.93 | 0.59 | 0.26 | 1.35 |
| `conf_phaser_flanger_chorus_v1_a__chord_long_C4maj_v102` | 120 | +2.17 | -2.8 | 0.34 | 1.39 | 0.99 | 49.78 | 2.18 | 0.34 | 2.42 | 2.75 |
| `conf_phaser_flanger_chorus_v1_b__chord_long_C4maj_v102` | 120 | +2.13 | -3.2 | 0.96 | 1.96 | 0.87 | 62.04 | 4.39 | 0.96 | 2.13 | 2.32 |
| `conf_phaser_flanger_chorus_v1_c__chord_long_C4maj_v102` | 120 | +2.26 | -17.7 | 0.33 | 1.43 | 0.98 | 44.42 | 1.45 | 0.33 | 1.81 | 2.60 |
| `conf_spread_vs_stereo_fx_v1_a__chord_long_C4maj_v102` | 120 | +2.17 | -0.8 | 0.31 | 1.28 | 0.82 | 21.18 | 1.91 | 0.31 | 1.39 | 1.57 |
| `conf_spread_vs_stereo_fx_v1_b__chord_long_C4maj_v102` | 120 | +2.18 | -53.8 | 0.19 | 3.12 | 0.80 | 27.90 | 2.41 | 0.19 | 3.53 | 2.24 |
| `mus_pad_05__chord_long_C2maj_v102` | 120 | +1.99 | -860.0 | 0.50 | 2.83 | 0.96 | 5.93 | 2.63 | 0.50 | 2.86 | 1.15 |
| `mus_keys_04__chord_short_C5maj_v102` | 120 | +2.39 | -217.6 | 0.26 | 2.19 | 0.73 | 8.91 | 1.64 | 0.26 | 2.22 | 1.08 |
| `mus_bell_00__pad_chords_120bpm` | 120 | +2.12 | -470.0 | 0.72 | 1.94 | 0.91 | 5.57 | 2.36 | 0.72 | 1.97 | 1.47 |
| `cplx_22__pad_chords_120bpm` | 120 | +2.17 | +44.2 | 0.54 | 24.67 | 0.95 | 38.62 | 1.56 | 0.54 | 23.90 | 1.10 |
| `mus_808_00__bass_glide_120bpm` | 120 | +2.13 | -2.4 | 0.25 | 0.73 | 0.89 | 23.62 | 4.09 | 0.25 | 0.87 | 4.02 |
| `mus_reese_bass_05__bass_glide_120bpm` | 120 | +2.06 | +0.4 | 0.32 | 0.25 | 0.94 | 3.46 | 3.33 | 0.32 | 0.37 | 1.22 |
| `mus_sub_bass_00__bass_octave_120bpm` | 120 | +2.20 | +40.9 | 0.82 | 47.47 | 1.09 | 157.96 | 46.02 | 0.82 | 47.16 | 4.10 |
| `cplx_17__arp_120bpm` | 120 | +2.21 | +21.7 | 0.82 | 59.94 | 0.97 | 64.42 | 4.85 | 0.82 | 59.81 | 4.03 |
| `mus_arp_02__arp_140bpm` | 140 | +1.91 | -349.1 | 0.46 | 3.90 | 0.97 | 6.28 | 0.54 | 0.46 | 3.95 | 1.88 |
| `mus_lead_03__lead_glide_140bpm` | 140 | +2.23 | -381.6 | 0.64 | 7.05 | 0.90 | 14.60 | 1.54 | 0.64 | 7.16 | 1.17 |
