# Reference recordings

Reference recordings check the automatic render against a render made
elsewhere. Each one renders a generated Vital preset and MIDI file in a
standard DAW host, on the retail Vital 1.0.7, exported as 32-bit float
stereo at 44.1 kHz with its tail. Our host renders the same preset, MIDI
file and tempo 20 times, and the comparison asks whether the recording
lies within the spread of those renders. The
[testing page](testing.md#reference-recordings) gives the method, and the
[render spread page](../reference/formats/render-spread.md) the outputs.

The recordings and the renders stay local. This page and the two reports
next to it hold the preset ids, the hashes of the preset, the MIDI file
and the recording, and the metrics:

- [`report.json`](reference-recordings/report.json): the comparison as
  recorded.
- [`report-gain.json`](reference-recordings/report-gain.json): the same
  comparison with every recording raised by 2.16 dB, a diagnostic. The
  renders of the two runs are separate sets of 20.

38 recordings cover isolated controls, musical presets of each role,
look-alike pairs and complex presets, at 120 and 140 BPM. For the 34 whose
DAW exported the state it played, the exported state matches the preset
on every control, at the recording's tempo.

## Result

No recording matches the automatic render on all four outputs. The
acceptance of the comparison, every preset within the spread, is not
met. Two differences explain most of it, and neither is a difference of
the synth's sound:

- Level. On the 12 recordings whose duration lies within the spread, the
  recording is 1.73 to 2.20 dB quieter, with a median of 2.16 dB. The
  offset is the same across presets of every kind, so it points to a
  gain in the DAW's signal chain, such as a channel volume or a pan law.
  It is not verified.
- Tail. On presets with a long release, reverb or delay, the recording
  ends earlier: by 0.2 to 1.3 s on musical presets with a long release,
  and by 1.7 s on the delay preset. A clip ends at its last sample above
  -96 dBFS, so the export cut the tail at a higher level. A shorter tail
  also raises the clip's RMS level, which runs from the onset to the end.

With every recording raised by 2.16 dB, the peak and the envelope lie
within the spread on 35 of 38 recordings, the RMS level on 21 and the
duration on 16. 11 recordings match on all four outputs. 22 of the 27
others end earlier than the renders. The other five miss by at most
0.04 dB, on outputs whose spread is 0.04 dB or less, and one of them,
the forced portamento, also on the envelope: 0.45 dB against a spread of
0.16 dB.

Nothing was tuned to make a recording match. The next step is to check
the DAW's channel gain and its export setting for tails, and to record
again.

## Each recording

The level offset is the recording's RMS level minus the median of the
renders, and the duration difference the recording's duration minus the
median of the renders, both as recorded. The last two columns name the
outputs beyond the spread, as recorded and with the recording raised by
2.16 dB.

| Id | BPM | Level offset (dB) | Duration difference (s) | Beyond, as recorded | Beyond, +2.16 dB |
| :-- | --: | --: | --: | :-- | :-- |
| `iso_init__chord_long_C4maj_v102` | 120 | -2.20 | +0.000 | RMS, peak, envelope | none |
| `iso_uni_v7_det40__chord_long_C4maj_v102` | 120 | -2.17 | -0.000 | RMS, peak, envelope | none |
| `iso_chorus_mix100__chord_long_C4maj_v102` | 120 | -2.13 | -0.023 | RMS, peak, duration, envelope | duration |
| `iso_amp_rel1__chord_short_C4maj_v102` | 120 | -2.29 | -0.005 | RMS, peak, duration, envelope | none |
| `iso_reverb_d1__chord_short_C4maj_v102` | 120 | -2.01 | -0.114 | RMS, peak, duration, envelope | none |
| `iso_lfo_amp_1_8__chord_long_C4maj_v102` | 120 | -2.17 | +0.000 | RMS, peak, envelope | peak |
| `iso_comp_fast__chord_long_C4maj_v102` | 120 | -2.17 | -0.003 | RMS, peak, envelope | none |
| `iso_dist_soft_12db__chord_long_C4maj_v102` | 120 | -2.18 | -0.001 | RMS, peak, duration, envelope | RMS |
| `iso_delay_1_8_fb50__chord_short_C4maj_v102` | 120 | +0.11 | -1.683 | peak, duration, envelope | RMS, duration |
| `mus_pad_00__chord_long_C4maj_v102` | 120 | -1.51 | -1.050 | RMS, peak, duration, envelope | RMS, duration |
| `mus_supersaw_00__chord_long_C4maj_v102` | 120 | -1.61 | -0.717 | RMS, peak, duration, envelope | RMS, duration |
| `mus_keys_00__phrase_pad_chords_120bpm_v102` | 120 | -2.15 | -0.075 | RMS, peak, envelope | none |
| `mus_stab_00__chord_short_C4maj_v102` | 120 | -1.68 | -0.255 | RMS, peak, duration, envelope | duration |
| `mus_pluck_00__strum_120bpm` | 120 | -1.51 | -0.612 | RMS, peak, duration, envelope | RMS, duration |
| `iso_porta_force_0.1__chord_long_C4maj_v102` | 120 | -2.16 | +0.001 | RMS, peak, envelope | peak, envelope |
| `mus_pad_00__pad_chords_120bpm` | 120 | -1.79 | -1.283 | RMS, peak, duration, envelope | RMS, duration |
| `mus_arp_00__arp_120bpm` | 120 | -1.59 | -0.787 | RMS, peak, duration, envelope | RMS, duration, envelope |
| `mus_lead_00__lead_legato_120bpm` | 120 | -2.01 | -0.386 | RMS, peak, duration, envelope | RMS, duration |
| `mus_reese_bass_00__bass_octave_120bpm` | 120 | -1.89 | -0.072 | RMS, peak, envelope | none |
| `iso_porta_0.1__lead_glide_120bpm` | 120 | -2.12 | +0.000 | RMS, envelope | RMS |
| `mus_fx_00__chord_long_C4maj_v102` | 120 | -1.81 | -1.143 | RMS, peak, duration, envelope | RMS, peak, duration, envelope |
| `conf_cutoff_vs_wavetable_v1_a__chord_long_C4maj_v102` | 120 | -2.15 | -0.000 | RMS, peak, envelope | none |
| `conf_cutoff_vs_wavetable_v1_b__chord_long_C4maj_v102` | 120 | -2.17 | -0.000 | RMS, peak, envelope | RMS |
| `conf_phaser_flanger_chorus_v1_a__chord_long_C4maj_v102` | 120 | -2.15 | -0.004 | RMS, peak, duration, envelope | duration |
| `conf_phaser_flanger_chorus_v1_b__chord_long_C4maj_v102` | 120 | -2.12 | -0.003 | RMS, peak, duration, envelope | RMS, duration |
| `conf_phaser_flanger_chorus_v1_c__chord_long_C4maj_v102` | 120 | -2.23 | -0.024 | RMS, peak, duration, envelope | none |
| `conf_spread_vs_stereo_fx_v1_a__chord_long_C4maj_v102` | 120 | -2.18 | -0.001 | RMS, peak, duration, envelope | duration |
| `conf_spread_vs_stereo_fx_v1_b__chord_long_C4maj_v102` | 120 | -2.08 | -0.059 | RMS, peak, duration, envelope | duration |
| `mus_pad_05__chord_long_C2maj_v102` | 120 | -1.54 | -0.843 | RMS, peak, duration, envelope | RMS, duration |
| `mus_keys_04__chord_short_C5maj_v102` | 120 | -1.96 | -0.203 | RMS, peak, duration, envelope | duration |
| `mus_bell_00__pad_chords_120bpm` | 120 | -2.08 | -0.468 | RMS, peak, duration, envelope | duration |
| `cplx_22__pad_chords_120bpm` | 120 | -2.20 | +0.043 | RMS, peak, duration, envelope | duration |
| `mus_808_00__bass_glide_120bpm` | 120 | -2.14 | -0.002 | RMS, peak, envelope | none |
| `mus_reese_bass_05__bass_glide_120bpm` | 120 | -1.73 | -0.000 | RMS, peak, envelope | none |
| `mus_sub_bass_00__bass_octave_120bpm` | 120 | -2.25 | +0.041 | RMS, peak, duration, envelope | RMS, duration |
| `cplx_17__arp_120bpm` | 120 | -2.24 | +0.021 | RMS, peak, duration, envelope | RMS, duration |
| `mus_arp_02__arp_140bpm` | 140 | -1.67 | -0.353 | RMS, duration, envelope | RMS, duration |
| `mus_lead_03__lead_glide_140bpm` | 140 | -2.00 | -0.357 | RMS, peak, duration, envelope | RMS, duration |
