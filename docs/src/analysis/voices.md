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
  envelope layer reads as silent holds none. A held part stops at the stem end,
  so a note listed past the stem holds none either. A note under 25 ms holds none
  without a knee [fact]. The issue asks
  for the first note to lie within 3 dB of its sustain level. This reads the
  knee instead and has no level test [fact].
- Pitches within 60 cents form one voice, so seven detuned copies of one pitch
  count once [fact, proposal for 60 cents].
- `max_polyphony` is the most voices that have started by any onset and are held
  50 ms after it, so a held overlap under 50 ms does not count [fact, proposal for
  50 ms]. `mono` is true when it is 1, so one overlap of 50 ms or more makes a
  line poly, with no tolerance [fact].
- `mean_polyphony` is the mean count over the time any note is held [fact]. An
  overlap under 50 ms counts here and not in `max_polyphony`, so the mean can
  exceed the maximum [fact].
- The confidence is the share of that time whose count agrees with the verdict:
  one voice on a mono line, two or more otherwise. It reads high from 0.9 and
  medium from 0.7 [fact, proposal for both].

## Joins

Consecutive notes of a mono line join when the later onset lies under 30 ms past
the earlier end in the note list, starts after the earlier onset, and lies less
than 50 ms inside the earlier held part. A note stacked on another joins nothing
and leaves the earlier note in place unless it ends sooner, and a silent note
leaves it in place too [fact]. `gap_ms` is
negative when they overlap in the list. The split join of the notes step merges a same-key repeat with no
attack of its own, so that repeat reaches no join [fact].

- `dip_db` is the level of the quieter note minus the lowest level from 25 ms
  before the onset, or from the earlier held end when that comes first, to 25 ms
  after it, on an RMS over one period of the lower note [fact]. A transcribed onset
  can land after the silence it follows, which the held end still sees [synthetic]. Each note's
  level is the 10th percentile over up to 100 ms beside the join, because that
  window still ripples on the higher note [fact].
- `centroid_change` is the centroid over f0 after the join over the same before
  it, minus 1, over 20 ms or two periods each side, and null when either side holds
  no signal or runs past either end of the stem [fact]. The verdict does not read
  it, because it gained nothing on the set below [synthetic].
- `legato` is true when `dip_db` is under 1 [fact, proposal for 1 dB]. A 12 dB dip
  that rises over 5 ms at 110 Hz reads 1.35 dB [synthetic].
- `dip_db` and `legato` are null, with no estimate, when neither note has a level
  above the -120 dB silence floor beside the join, as with notes of about 25 ms or
  notes listed past the stem [fact].
- A join reads low when its verdict is legato, as a retrigger with an instant
  attack keeps its level too; when no attack was read; when the 25 ms beside the
  join and one period more do not fit in the stem, as the later note's level is then
  unread or short; or when the median attack of the pool is over 100 ms, because a long attack hides a retrigger [fact, proposal
  for 100 ms]. Otherwise it reads medium [fact].

The set: 4000 joins sampled over 41 to 440 Hz, steps of -12 to +12 semitones,
four timbres, dips of 3 to 60 dB, attacks of 1 to 50 ms, level changes of up to
6 dB, onsets off by up to 20 ms and gaps of -20 to 10 ms, of which 3669 reach a
join. At 1 dB, 91 % of the legato joins read legato [synthetic]. Of the
retriggers of 12 dB or more [synthetic]:

| Attack | Read as a retrigger |
| :-- | :-- |
| 1 ms | 100 of 274 |
| 5 ms | 188 of 288 |
| 20 ms | 253 of 271 |
| 50 ms | 292 of 296 |

## Limits

- A window of one period of the lower note smears a short new attack, so a
  retrigger with an attack of 5 ms or less often reads legato, as the table shows.
  No level read shorter than a period holds still on a periodic wave [fact].
- A retrigger with an instant attack keeps its level, so on generated lines with
  exact note lists it mostly reads legato: 232 of 270 joins of a retriggered 808,
  106 of 127 of a soft retrigger and 81 of 131 of a stepped one, against 150 of 174
  of a legato 808. A legato line with a dip reads legato in 46 of 73 joins
  [synthetic].
- The count reads the note list as it is, so a transcriber's wrong overlap counts,
  and one overlap of 50 ms or more makes a line poly. On 889 generated mono lines
  the chord gate calls monophonic, 136 read poly from transcribed notes, 131 of them
  at low confidence. With exact note lists, 463 of 463 read mono [synthetic]. On 22
  real instrument stems, 1 read mono and none reported a join, at mostly low
  confidence. The other 21 read a maximum of 3 to 8 and a mean of 1.06 to 2.52
  [fact]. So a mapper must not take `mono: false` or missing joins as evidence at low confidence.
- The verdict depends on where the join falls in the period, and the acceptance
  test sits on a whole-cycle position. Moving a join from 1.0 s in 0.31 ms steps over
  40 positions, between the same saws, read these counts [synthetic]:

  | Hz | 0 dB join read legato | 12 dB, 5 ms retrigger found |
  | :-- | :-- | :-- |
  | 110, 165 | 31 | 24 |
  | 220, 330 | 34 | 40 |
  | 110, 146.83 | 37 | 24 |
  | 82.41, 123.47 | 29 | 24 |

- With under 30 ms of audio after a join, the later note has no level, so a legato
  join reads as a retrigger: 682 of 960 over four pairs, against 17 of 160 at each
  length from 40 ms. A 12 dB, 20 ms retrigger reads as one in all but 27 of 960, so
  the verdict stays and the join reads low, rather than losing those [synthetic]. At the stem start there is
  no such excess past 5 ms: 45 and 47 of 160 at leads of 3 and 5 ms, and 18 to 40 from
  10 ms on, against 29 from 1.0 s [synthetic].
- `dip_db` reaches 100 to 190 dB on exact digital zeros, since the -200 dB floor stands
  in for a level. The verdict does not change [synthetic].
- A filter that restarts with no dip in level reads legato. In the set, 228 of 433
  filter joins read as a retrigger, against 289 of 436 to 358 of 476 for the other
  timbres [synthetic].
- A long attack hides a retrigger, and the amplitude envelope may not restart from
  zero, which makes a dip shallower [fact].
- A pad's count is a lower bound, because pads are the weakest class of the
  transcriber [fact].
- Glides and legato inside chords have no method here [fact].
