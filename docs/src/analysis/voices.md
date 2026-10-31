# Voices

The voices layer counts how many notes sound at once and, on a mono line,
reads how each note joins the next. `analyze` writes it to the `voices` group
of `layers.json`. It reads any line, chords too, so its fields sit in a group
of their own and the `pitch` group stays null on chords [fact].

`Voicing`, the chord gate of the transcriber and the pitch layer, does not read
this count [fact].

## The count

- A note holds a voice from its onset to its note-off knee, or to 25 ms before
  its end without a knee, so a release tail holds no voice, and a note the
  envelope layer reads as silent holds none [fact]. The issue asks
  for the first note to lie within 3 dB of its sustain level; the knee reads the
  same end and needs no second level read [fact].
- Pitches within 60 cents form one voice, so seven detuned copies of one pitch
  count once [fact, proposal for 60 cents].
- `max_polyphony` is the most voices 50 ms after any onset, so an overlap under
  50 ms does not count [fact, proposal for 50 ms]. `mono` is true when it is 1.
- `mean_polyphony` is the mean count over the time any note is held [fact].
- The confidence is the share of that time whose count agrees with the verdict:
  one voice on a mono line, two or more otherwise. It reads high from 0.9 and
  medium from 0.7 [fact, proposal for both].

## Joins

Consecutive notes of a mono line join when the later onset lies under 30 ms past
the earlier end in the note list [fact]. `gap_ms` is negative when they overlap
in the list. The split join of the notes step merges a same-key repeat with no
attack of its own, so that repeat reaches no join [fact].

- `dip_db` is the level of the quieter note minus the lowest level within 25 ms
  of the onset, on an RMS over one period of the lower note [fact]. Each note's
  level is the 10th percentile over up to 100 ms beside the join, because that
  window still ripples on the higher note [fact].
- `centroid_change` is the centroid over f0 after the join over the same before
  it, minus 1, over 20 ms or two periods each side, and null when either side holds
  no signal or runs past the stem [fact]. The verdict does not read it, because it
  gained nothing on the set below [synthetic].
- `legato` is true when `dip_db` is under 1 [fact, proposal for 1 dB]. A 12 dB dip
  that rises over 5 ms at 110 Hz reads 1.35 dB [synthetic].
- `dip_db` and `legato` are null, with no estimate, when neither note has a level
  beside the join, as with notes of about 25 ms [fact].
- A join reads low when the median attack of the pool is over 100 ms, because a
  long attack hides a retrigger, and medium otherwise [fact, proposal for 100 ms].

The set: 4000 joins sampled over 41 to 440 Hz, steps of -12 to +12 semitones,
four timbres, dips of 3 to 60 dB, attacks of 1 to 50 ms, level changes of up to
6 dB, onsets off by up to 20 ms and gaps of -20 to 10 ms, of which 3669 reach a
join. At 1 dB, 91 % of the legato joins read legato [synthetic]. Of the
retriggers of 12 dB or more [synthetic]:

| Attack | Read as a retrigger |
| :-- | :-- |
| 1 ms | 101 of 274 |
| 5 ms | 186 of 288 |
| 20 ms | 253 of 271 |
| 50 ms | 292 of 296 |

## Limits

- A window of one period of the lower note smears a short new attack, so a
  retrigger with an attack of 5 ms or less often reads legato, as the table shows.
  No level read shorter than a period holds still on a periodic wave [fact].
- A retrigger with an instant attack keeps its level, so on generated lines with
  exact note lists it mostly reads legato: 242 of 270 joins of a retriggered 808,
  107 of 127 of a soft retrigger and 97 of 131 of a stepped one, against 150 of 174
  of a legato 808. A legato line with a dip reads legato in 46 of 73 joins
  [synthetic].
- The count reads the note list as it is, so a transcriber's wrong overlap counts.
  On 889 generated mono lines the chord gate calls monophonic, 139 read poly from
  transcribed notes, 134 of them at low confidence. With exact note lists, 463 of
  463 read mono [synthetic]. Such a line reports no joins.
- A filter that restarts with no dip in level reads legato: 53 % of the filter
  joins in the set read as a retrigger, against 66 to 75 % for the other timbres
  [synthetic].
- A long attack hides a retrigger, and the amplitude envelope may not restart from
  zero, which makes a dip shallower [fact].
- A pad's count is a lower bound, because pads are the weakest class of the
  transcriber [fact].
- Glides and legato inside chords have no method here [fact].
