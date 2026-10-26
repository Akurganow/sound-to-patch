# Units

Every physical value in the library has a unit type. A value is checked
once, when it is built from a number, a JSON field or a unit string, and is
valid by type afterwards. Only the constructors and the ratio and gain
conversions take a bare float; every other public function takes a unit
type.

The code examples on this page come from `examples/units.rs`, which CI
compiles. `cargo run --example units` prints their results.

## Unit types

Each type holds an `f64` that is never NaN or infinite. Its constructor
returns an error that names the type and the accepted range, for example
`Normalized out of range: got 1.5, expected 0.0 to 1.0`. The rustdoc of each
type states its exact range.

| Type | Holds |
| :-- | :-- |
| [`Hz`](../api/sound_to_patch/units/struct.Hz.html) | a frequency, zero or more |
| [`Seconds`](../api/sound_to_patch/units/struct.Seconds.html) | a duration, zero or more |
| [`Semitones`](../api/sound_to_patch/units/struct.Semitones.html) | an interval, either sign |
| [`Cents`](../api/sound_to_patch/units/struct.Cents.html) | an interval in hundredths of a semitone, either sign |
| [`Octaves`](../api/sound_to_patch/units/struct.Octaves.html) | an interval, either sign |
| [`Decibels`](../api/sound_to_patch/units/struct.Decibels.html) | a level, either sign |
| [`Percent`](../api/sound_to_patch/units/struct.Percent.html) | a percentage of a unipolar or a bipolar control |
| [`Normalized`](../api/sound_to_patch/units/struct.Normalized.html) | a knob position |
| [`MidiPitch`](../api/sound_to_patch/units/struct.MidiPitch.html) | a fractional MIDI note |

`Percent` has two ranges, fixed by its type parameter.
`Percent<Unipolar>` is for controls that go one way from zero.
`Percent<Bipolar>` is for controls that go both ways, such as
a modulation amount. A unipolar value widens to a bipolar one; the reverse
is checked.

No milliseconds type exists. A value in milliseconds converts to `Seconds`
where it enters, with `Seconds::from_millis` or the `ms` suffix.

## Conversions

MIDI pitch and frequency use equal temperament with note 69, A4, at
440 Hz:

$$f = 440 \cdot 2^{(m - 69)/12} \qquad m = 69 + 12 \log_2 \frac{f}{440}$$

Every MIDI pitch has a frequency, so `Hz::from` a `MidiPitch` cannot fail.
The reverse fails for 0 Hz, which has no pitch, and for frequencies outside
notes 0 to 127, about 8.18 Hz to 12 544 Hz. A note number from a MIDI file
converts with `MidiPitch::try_from` a `u8`, which rejects numbers above 127.

Intervals convert to frequency ratios. One semitone is 100 cents, one
octave 1200 cents:

$$r = 2^{c/1200} \qquad c = 1200 \log_2 r \qquad r = 2^{s/12} \qquad r = 2^{o}$$

Decibels convert two ways. An amplitude gain scales a magnitude, such as a
sample or a spectral magnitude weight. A power ratio scales an energy,
such as the share of a frequency band:

$$g = 10^{\mathrm{dB}/20} \qquad \mathrm{dB} = 20 \log_{10} g \qquad p = 10^{\mathrm{dB}/10} \qquad \mathrm{dB} = 10 \log_{10} p$$

A gain or a ratio of zero has no decibel or cent value, so the inverse
conversions return an error for it. The forward conversions return an error
beyond a conservative cap, such as 1000 octaves, that keeps the ratio
finite and nonzero.

The conversion methods are
[`Cents::to_ratio`](../api/sound_to_patch/units/struct.Cents.html#method.to_ratio),
[`Cents::from_ratio`](../api/sound_to_patch/units/struct.Cents.html#method.from_ratio),
[`Semitones::to_ratio`](../api/sound_to_patch/units/struct.Semitones.html#method.to_ratio),
[`Octaves::to_ratio`](../api/sound_to_patch/units/struct.Octaves.html#method.to_ratio),
[`Decibels::to_amplitude_gain`](../api/sound_to_patch/units/struct.Decibels.html#method.to_amplitude_gain),
[`Decibels::from_amplitude_gain`](../api/sound_to_patch/units/struct.Decibels.html#method.from_amplitude_gain),
[`Decibels::to_power_ratio`](../api/sound_to_patch/units/struct.Decibels.html#method.to_power_ratio)
and [`Decibels::from_power_ratio`](../api/sound_to_patch/units/struct.Decibels.html#method.from_power_ratio).

```rust,ignore
{{#include ../../../examples/units.rs:convert}}
```

## Unit strings

The agent's edit list writes each value as a unit string: a number,
optional whitespace and a suffix. The parser accepts these suffixes, and
only these, matched case-sensitively:

| Suffix | Unit | Type |
| :-- | :-- | :-- |
| `Hz` | hertz | `Hz` |
| `kHz` | kilohertz | `Hz` |
| `s` | seconds | `Seconds` |
| `ms` | milliseconds | `Seconds` |
| `ct` | cents | `Cents` |
| `st` | semitones | `Semitones` |
| `oct` | octaves | `Octaves` |
| `dB` | decibels | `Decibels` |
| `%` | percent | `Percent` |

A target type accepts only its own suffixes. A suffix of another unit, an
unknown suffix and a missing number are errors that list the accepted
suffixes. A bare number for a unit value is an error too; only enums,
counts and bools take bare values. The parsed value then passes the range
check of its type, so `"-5 Hz"` fails with the range of `Hz`, and `"-40%"`
parses for a bipolar control but fails for a unipolar one.

```rust,ignore
{{#include ../../../examples/units.rs:parse}}
```

Each type prints in a form that parses back to the same value, such as
`1200 Hz` or `-40%`.

## JSON forms

Values reach the library in two JSON forms, and both parse through the
same range checks:

- `layers.json` holds plain numbers with the unit in the field name, such
  as `"cutoff_hz": 410` or `"attack_s": 0.02`. An integer is accepted for a
  float field. A string in a number field fails with the parser's type
  message.
- The agent's edit list holds unit strings, such as `"value": "410 Hz"`.
  The edit-list contract
  ([#94](https://github.com/Akurganow/sound-to-patch/issues/94)) documents
  where each string goes.

A value outside its range fails with the type's message, and the JSON
reader adds the line and column. NaN and infinity never reach a file,
because no unit type can hold them.

## Confidence

Every estimate in the layer report carries a
[`Confidence`](../api/sound_to_patch/confidence/struct.Confidence.html):

- a [level](../api/sound_to_patch/confidence/enum.Level.html): `low`, `medium` or `high`. The estimator stores it from its own
  signals; the type does not derive it;
- one or more reasons. Each pairs a stable
  [code](../api/sound_to_patch/confidence/enum.ReasonCode.html), such as `active_frames`, with the
  [value](../api/sound_to_patch/confidence/enum.Signal.html) of the signal behind it: a count, a number
  or a bool;
- an optional spread, never negative, in the unit the type parameter
  names. A cutoff is a frequency, but its spread is in octaves, so its
  confidence is a `Confidence<Octaves>`. An estimate without a spread, such
  as a menu choice, uses `Confidence<NoSpread>`;
- or a "no estimate" state. A layer with too little evidence reports no
  value, no level and no spread, and at least one reason.

The report writes one object per confidence. `spread` is left out when
there is none:

```json
{"level": "medium", "reasons": [{"code": "active_frames", "value": 412}], "spread": 0.3, "estimate": true}
{"reasons": [{"code": "too_few_notes", "value": 2}], "estimate": false}
```

Reading a report applies the same rules: a level on a "no estimate" state,
an estimate without a level, an empty reason list, a negative spread and an
unknown field or code are errors.

```rust,ignore
{{#include ../../../examples/units.rs:confidence}}
```

## Unit traps

Some Serum 2 controls store a knob position where a reader expects a
physical unit. Such a control takes a `Normalized` value. No conversion
from `Normalized` to `Hz` or `Cents` exists, because the law from knob to
unit is not measured yet. A cutoff in Hz therefore has no unit type for its
knob: the preset writer will map it through the interim law of the
parameter registry
([#107](https://github.com/Akurganow/sound-to-patch/issues/107)) and mark
the value uncalibrated.

Each claim below has a build column and a status: `verified` in a
Serum 2.1.5 session, `reported` by an outside source, or `open`. The
outside source records no build, so every row says `not recorded` and is
`reported`. The probe presets of
[#42](https://github.com/Akurganow/sound-to-patch/issues/42) will check
each claim on build 2.1.5 and fill in the build.

| Control | Stored as | Unit type | Build | Status |
| :-- | :-- | :-- | :-- | :-- |
| Voice filter cutoff | knob position 0 to 1 | `Normalized` | not recorded | `reported` |
| FX Distortion frequency | knob position 0 to 1 | `Normalized` | not recorded | `reported` |
| FX Filter frequency | knob position 0 to 1 | `Normalized` | not recorded | `reported` |
| EQ, Phaser and Chorus frequencies | hertz | `Hz` | not recorded | `reported` |
| Unison detune | knob position 0 to 1 | `Normalized` | not recorded | `reported` |
| Reverb pre-delay | milliseconds | `Seconds`, converted | not recorded | `reported` |
| Delay time | seconds | `Seconds` | not recorded | `reported` |
| Modulation amount | percent, -100 to 100 | `Percent<Bipolar>` | not recorded | `reported` |

## Sources

- Preset parameter tables: [serum-mcp](https://github.com/Celian-mrc/serum-mcp).
