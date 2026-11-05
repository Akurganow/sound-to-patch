# Suggested edits

The mapping step turns the `pitch`, `voices` and `envelope` groups of
`layers.json` into edits for one synth. It runs in two parts. A
synth-neutral plan decides what the patch should hold, in physical units.
The synth's adapter then writes each setting to its own controls with its
own laws. This page states the plan.

The output holds the edits, an `unrepresentable` list for values the synth
cannot reach, and an `unwritten` list for measured fields the plan leaves
out on purpose. `analyze` writes the edits and the `unrepresentable` entries
into each group of `layers.json` (see [Layer report](../reference/formats/layers.md));
the `unwritten` list stays in the library. Each `unwritten` entry names the field by its path in the
group's `values`, such as `attack_s` or `glides.curve`, and gives one reason:

| Reason | Meaning |
| :-- | :-- |
| `low_confidence` | The field has no estimate, or a low one. |
| `upper_bound` | The field is an upper bound, such as an attack faster than the track can tell. |
| `not_identifiable` | The report cannot separate the value from what hides it, such as a release under a heavy tail. |
| `curve_not_set` | The curve stays at its default; tune it by ear. |
| `unprobed` | The control exists, but no probe has shown what it does. |

## Voicing

- Mono, legato and the voice count need a voices verdict of medium
  confidence or higher. A `voices` group at low confidence writes nothing,
  and lists `mono` or `max_polyphony`; set them by ear. A low count is no
  evidence of voicing (see [Voices](voices.md)).
- A mono stem writes mono on and no voice count.
- Legato is on when a join of medium or high confidence reads legato. It is
  off when every such join retriggers. A synth's legato acts only where notes
  overlap, so one legato join needs it on. Joins with no confident verdict
  write nothing and list `joins.legato`, so legato keeps the template's
  setting; set it by ear.
- A poly stem writes mono off, legato off and a voice count of
  `max_polyphony`, times 1 unison voice and 1 oscillator. The unison step
  sets the final count once it knows those numbers.

## Glide and pitch drop

- A record of low confidence counts for nothing.
- The glide time is the median `time_s` of the confident glides: the full 0
  to 100 % time. A time beyond the synth's maximum lands on the maximum and is
  listed in `unrepresentable`. Always-glide is on when one of those glides
  starts from silence.
- No law maps the fitted glide shape `p` to a synth's glide curve, so the
  curve stays at its default and `glides.curve` reads `curve_not_set`.
- The pitch drop is the median `df_st` and the median `tau_s` of the
  confident drops. The adapter writes a unipolar envelope route into
  oscillator pitch. A drop beyond the destination's range lands on the range,
  and the rest goes to `unrepresentable`. The route amount and the drop
  envelope are interim laws, marked `uncalibrated`, until the calibration set
  measures them.
- Vibrato writes no route here.

## Amplitude envelope

- A stage of low confidence writes nothing. An edit's confidence is the
  lowest level of the records behind it, capped by the group's level.
- An attack flagged `attack_is_upper_bound` writes nothing and reads
  `upper_bound`.
- A hold is written from 10 ms, the plateau the envelope layer reads.
- The decay runs from the peak to the sustain level. The sustain is the held
  level from the peak in dB; the adapter's law turns it into its stored
  value.
- A release is written only when `release_confidence` is medium or high:
  the envelope layer leaves no release under a tail heavier than 0.5 or with
  a residual above 3 dB. Otherwise `release_s` reads `not_identifiable`.
- The attack, decay and release curves stay at their defaults and read
  `curve_not_set`.
- The tail and the pump have no envelope control. They belong to the space
  and modulation layers.
- A value equal to the control's default writes no edit.

## Velocity

Velocity gets a path to the level when `level_spread_db` has a value, read
over at least 3 pooled notes, and exceeds 3 dB. The edit is always low
confidence, as the spread is.
