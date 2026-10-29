# Reference recordings

Reference recordings check the automatic render against a render made
elsewhere. Each one renders a generated Vital preset and MIDI file in a
standard DAW host, on the retail Vital 1.0.7, exported as 32-bit float
stereo at 44.1 kHz with its tail. Our host draws 20 renders of the same
preset, MIDI file and tempo, in sequence on one fresh plug-in instance,
and the comparison asks whether the recording lies as close to those
renders as they lie to each other. The
[testing page](testing.md#reference-recordings) gives the method, and the
[render spread page](../reference/formats/render-spread.md) the outputs,
the tolerance and the floor.

The recordings and the renders stay local. This page and the two reports
next to it hold the preset ids, the hashes of the preset, the MIDI file
and the recording, and the metrics:

- [`report.json`](reference-recordings/report.json): the comparison as
  recorded.
- [`report-gain.json`](reference-recordings/report-gain.json): the same
  comparison with every recording raised by 2.16 dB, a diagnostic. Its
  verdicts are named `within_with_gain`. Each run starts every recording
  on a fresh instance, so both runs draw the same 20 renders and hold the
  same spread rows; only the recording's gain differs.

38 recordings cover isolated controls, musical presets of each role,
look-alike pairs and complex presets, at 120 and 140 BPM. For the 34 whose
DAW exported the state it played, the exported state matches the preset
on every control, at the recording's tempo.

## Result

No recording matches the automatic render on all four outputs, so the
acceptance of the comparison, every preset within the tolerance, is not
met. The ratio of each difference to its limit shows how far each output
lies off, in the tables below.

- As recorded, 0 of 38 recordings lie within the limit on all four outputs: the RMS level on 0, the peak on 1, the duration on 12 and the envelope on 0.
- With the recordings raised by 2.16 dB, 10 of 38 recordings lie within the limit on all four outputs: the RMS level on 31, the peak on 35, the duration on 14 and the envelope on 29.

Two differences account for most misses. Their cause is not known, on
either side: our host's gain staging and export are as much in question
as the DAW's.

- Level. On the 12 recordings whose duration lies within the limit, the
  recording is 1.98 to 2.20 dB quieter than the renders, with a median of
  2.16 dB. The 2.16 dB of the diagnostic run is that median, so it is
  fitted on the same 12 recordings that it then scores. Its verdicts on
  those 12 are not independent evidence of a match.
- Duration. A clip ends at its last sample above -96 dBFS.

- As recorded, 23 recordings end earlier than the renders, beyond the limit, and 3 end later.
- 15 of the earlier ones end 0.12 to 1.68 s early. The other 8 end 0.6 to 60.8 ms early.
- With the recordings raised by 2.16 dB, the duration lies within the limit on 1 of the 15 and on 1 of the 8.
- The later ones end `cplx_22__pad_chords_120bpm` by 42.8 ms, `mus_sub_bass_00__bass_octave_120bpm` by 40.6 ms, `cplx_17__arp_120bpm` by 21.7 ms late.

The recordings that end 0.12 s or more early are presets with a long
release, reverb or delay: their tail falls below -96 dBFS earlier than
the renders'. A quieter recording crosses -96 dBFS earlier on a
decaying tail, so part of an early end can come from the level offset.

Nothing was tuned to make a recording match.

## Calibration

A calibration recording settles where the level offset and the early
ends come from. It sends signals of known level through the DAW chain of
the references and through our host, and compares both with the source.

## Each recording

The RMS difference is the median over the renders of the level
difference over the common span, in dB. The duration difference is the
recording's duration minus the median of the renders, in ms. Each ratio
is the difference over its limit: above 1 lies beyond the limit. The
first four ratios are as recorded, the last four with the recording
raised by 2.16 dB.

| Id | BPM | RMS difference (dB) | Duration difference (ms) | RMS | Peak | Duration | Envelope | RMS, +2.16 dB | Peak, +2.16 dB | Duration, +2.16 dB | Envelope, +2.16 dB |
| :-- | --: | --: | --: | --: | --: | --: | --: | --: | --: | --: | --: |
| `iso_init__chord_long_C4maj_v102` | 120 | 2.20 | +0.2 | 45.48 | 4.12 | 0.51 | 1.83 | 0.81 | 0.94 | 0.51 | 0.78 |
| `iso_uni_v7_det40__chord_long_C4maj_v102` | 120 | 2.16 | -0.2 | 57.29 | 2.43 | 0.40 | 1.70 | 0.26 | 0.24 | 0.40 | 0.97 |
| `iso_chorus_mix100__chord_long_C4maj_v102` | 120 | 2.20 | -23.9 | 39.97 | 2.51 | 1.77 | 2.87 | 0.80 | 0.54 | 1.34 | 0.91 |
| `iso_amp_rel1__chord_short_C4maj_v102` | 120 | 2.34 | -5.2 | 7.69 | 1.75 | 1.99 | 1.98 | 0.60 | 0.39 | 0.28 | 1.04 |
| `iso_reverb_d1__chord_short_C4maj_v102` | 120 | 2.32 | -116.4 | 5.92 | 1.89 | 2.73 | 1.28 | 0.42 | 0.49 | 0.76 | 0.76 |
| `iso_lfo_amp_1_8__chord_long_C4maj_v102` | 120 | 2.17 | +0.2 | 32.23 | 216.65 | 0.54 | 1.73 | 0.31 | 0.65 | 0.46 | 0.85 |
| `iso_comp_fast__chord_long_C4maj_v102` | 120 | 2.17 | -2.3 | 40.06 | 4.02 | 0.99 | 3.58 | 0.29 | 0.87 | 0.93 | 0.96 |
| `iso_dist_soft_12db__chord_long_C4maj_v102` | 120 | 2.18 | -0.6 | 215.32 | 9.99 | 1.52 | 6.14 | 1.72 | 0.42 | 1.29 | 0.88 |
| `iso_delay_1_8_fb50__chord_short_C4maj_v102` | 120 | 2.43 | -1683.5 | 7.50 | 2.68 | 226.63 | 1.98 | 0.83 | 0.42 | 226.61 | 0.96 |
| `mus_pad_00__chord_long_C4maj_v102` | 120 | 2.00 | -1062.6 | 5.19 | 1.76 | 5.06 | 1.15 | 0.46 | 0.31 | 5.05 | 1.06 |
| `mus_supersaw_00__chord_long_C4maj_v102` | 120 | 2.10 | -711.3 | 23.68 | 2.76 | 5.76 | 2.33 | 0.68 | 0.47 | 5.73 | 0.95 |
| `mus_keys_00__phrase_pad_chords_120bpm_v102` | 120 | 2.18 | -45.4 | 6.03 | 1.75 | 0.42 | 1.68 | 0.58 | 0.30 | 0.42 | 0.94 |
| `mus_stab_00__chord_short_C4maj_v102` | 120 | 2.14 | -246.5 | 6.13 | 3.16 | 4.82 | 1.67 | 0.36 | 0.37 | 4.67 | 0.93 |
| `mus_pluck_00__strum_120bpm` | 120 | 2.15 | -612.7 | 14.02 | 2.11 | 76.58 | 2.50 | 0.38 | 0.53 | 75.75 | 0.99 |
| `iso_porta_force_0.1__chord_long_C4maj_v102` | 120 | 2.16 | +0.4 | 216.10 | 138.76 | 0.54 | 14.93 | 0.10 | 2.53 | 0.59 | 3.01 |
| `mus_pad_00__pad_chords_120bpm` | 120 | 2.18 | -1206.5 | 4.53 | 1.75 | 5.23 | 1.28 | 0.16 | 0.54 | 5.21 | 1.06 |
| `mus_arp_00__arp_120bpm` | 120 | 2.20 | -786.1 | 17.19 | 1.32 | 63.59 | 3.32 | 0.41 | 0.54 | 63.30 | 1.08 |
| `mus_lead_00__lead_legato_120bpm` | 120 | 2.16 | -394.2 | 83.30 | 3.81 | 22.65 | 1.14 | 0.39 | 0.46 | 22.32 | 0.83 |
| `mus_reese_bass_00__bass_octave_120bpm` | 120 | 2.05 | -72.7 | 5.00 | 132.92 | 0.85 | 1.48 | 0.58 | 0.36 | 0.80 | 0.88 |
| `iso_porta_0.1__lead_glide_120bpm` | 120 | 2.13 | +0.1 | 78.46 | 1.15 | 0.45 | 10.10 | 1.16 | 0.54 | 0.43 | 0.99 |
| `mus_fx_00__chord_long_C4maj_v102` | 120 | 2.18 | -1259.7 | 13.86 | 1.76 | 7.10 | 2.00 | 0.43 | 0.44 | 7.07 | 1.15 |
| `conf_cutoff_vs_wavetable_v1_a__chord_long_C4maj_v102` | 120 | 2.15 | +0.1 | 78.48 | 17.61 | 0.32 | 1.55 | 0.51 | 0.47 | 0.34 | 0.95 |
| `conf_cutoff_vs_wavetable_v1_b__chord_long_C4maj_v102` | 120 | 2.17 | -0.2 | 217.37 | 112.30 | 0.29 | 1.35 | 1.37 | 0.31 | 0.24 | 0.95 |
| `conf_phaser_flanger_chorus_v1_a__chord_long_C4maj_v102` | 120 | 2.17 | -4.9 | 65.49 | 2.82 | 2.47 | 2.76 | 0.42 | 0.47 | 1.42 | 0.99 |
| `conf_phaser_flanger_chorus_v1_b__chord_long_C4maj_v102` | 120 | 2.13 | -3.4 | 63.04 | 5.20 | 2.62 | 2.38 | 1.02 | 1.09 | 2.41 | 0.90 |
| `conf_phaser_flanger_chorus_v1_c__chord_long_C4maj_v102` | 120 | 2.26 | -22.4 | 52.73 | 1.75 | 1.94 | 2.69 | 2.35 | 0.43 | 1.68 | 1.02 |
| `conf_spread_vs_stereo_fx_v1_a__chord_long_C4maj_v102` | 120 | 2.17 | -0.9 | 23.83 | 2.91 | 1.49 | 1.58 | 0.43 | 0.48 | 1.37 | 0.83 |
| `conf_spread_vs_stereo_fx_v1_b__chord_long_C4maj_v102` | 120 | 2.18 | -60.8 | 31.09 | 3.01 | 4.15 | 2.32 | 0.64 | 0.20 | 3.67 | 0.83 |
| `mus_pad_05__chord_long_C2maj_v102` | 120 | 1.99 | -867.6 | 7.07 | 2.78 | 4.08 | 1.18 | 0.61 | 0.36 | 4.05 | 0.99 |
| `mus_keys_04__chord_short_C5maj_v102` | 120 | 2.39 | -220.6 | 9.42 | 2.35 | 2.25 | 1.30 | 0.91 | 0.28 | 2.22 | 0.87 |
| `mus_bell_00__pad_chords_120bpm` | 120 | 2.12 | -477.1 | 10.56 | 2.80 | 2.90 | 1.52 | 0.46 | 0.68 | 2.86 | 0.94 |
| `cplx_22__pad_chords_120bpm` | 120 | 2.17 | +42.8 | 39.43 | 2.26 | 43.06 | 1.11 | 0.38 | 0.75 | 44.45 | 0.95 |
| `mus_808_00__bass_glide_120bpm` | 120 | 2.13 | -2.8 | 29.38 | 5.38 | 0.88 | 4.07 | 0.51 | 0.27 | 0.74 | 0.89 |
| `mus_reese_bass_05__bass_glide_120bpm` | 120 | 2.06 | -0.4 | 4.03 | 4.77 | 0.65 | 1.22 | 0.25 | 0.27 | 0.43 | 0.95 |
| `mus_sub_bass_00__bass_octave_120bpm` | 120 | 2.20 | +40.6 | 189.41 | 47.20 | 48.37 | 4.11 | 3.76 | 0.25 | 48.69 | 1.09 |
| `cplx_17__arp_120bpm` | 120 | 2.21 | +21.7 | 79.54 | 5.91 | 72.78 | 4.16 | 1.97 | 1.11 | 72.93 | 1.01 |
| `mus_arp_02__arp_140bpm` | 140 | 1.91 | -353.6 | 7.27 | 0.64 | 4.04 | 1.89 | 0.97 | 0.80 | 3.96 | 0.97 |
| `mus_lead_03__lead_glide_140bpm` | 140 | 2.23 | -387.4 | 16.29 | 1.62 | 7.59 | 1.17 | 0.51 | 0.56 | 7.48 | 0.90 |
