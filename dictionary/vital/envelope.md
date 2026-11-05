Facts checked on the open source of build 1.0.6 (commit 636ca0e) unless an entry says measured. Routing an envelope to a target belongs to the modulation layer.

## How envelopes work

Vital has six envelopes (source: synth_constants.h). Each runs per voice through delay, attack, hold, decay and sustain while the key is down, then release when it lifts (source: envelope.cpp).

Each time runs from 0 to 1 and the level then follows a curve set by the segment's power. A power of 0 gives a straight line (source: envelope.cpp, futils.h).

Attack rises from the level at note start to 1. Decay falls from 1 to the sustain level and stays there. Release falls from the level at key lift to 0 (source: envelope.cpp).

Times in seconds are the stored value to the fourth power. Attack, decay and release reach 32 s. Delay and hold reach 4 s (source: synth_parameters.cpp, envelope.cpp).

A delay or hold of 0 skips that segment (source: envelope.cpp).

Envelope 1 is the amplitude envelope. Voice gain is (envelope 1 x lerp(1, velocity, `velocity_track`) x `voice_amplitude`) squared (source: synth_voice_handler.cpp; measured on the open engine). An envelope at 0.5 gives a quarter of the full gain.

A voice ends when its gain is silent after the key lifts. Envelope 1 release therefore sets how long the tail lasts (source: voice_handler.cpp, synth_voice_handler.cpp).

## Control entries

### Delay and hold
- Keys: `env_N_delay`, `env_N_hold`
- Effect: Delay waits before the attack starts. Hold keeps the level at 1 between attack and decay (source: envelope.cpp). Both run 0 to 4 s.
- Words: [envelope (adsr)](../core/words-lists.md#technique-words)

### Attack
- Keys: `env_N_attack`, `env_N_attack_power`
- Effect: Attack sets the seconds the level takes to rise to 1. Its power bends the curve with the sign flipped, so a positive power rises fast first (source: envelope.cpp, futils.h).
- Words: [envelope (adsr)](../core/words-lists.md#technique-words)

### Decay
- Keys: `env_N_decay`, `env_N_decay_power`
- Effect: Decay sets the seconds the level takes to fall from 1 to the sustain level. A negative power, the default, falls fast first and slows near the sustain level (source: envelope.cpp, futils.h).
- Words: [envelope (adsr)](../core/words-lists.md#technique-words)

### Sustain
- Keys: `env_N_sustain`
- Effect: Sets the level held after the decay while the key stays down, from 0 to 1. It is a level, not a time (source: envelope.cpp).
- Words: [envelope (adsr)](../core/words-lists.md#technique-words)

### Release
- Keys: `env_N_release`, `env_N_release_power`
- Effect: Release sets the seconds the level takes to fall from its value at key lift to 0. It is the time to reach 0, whatever the starting level (source: envelope.cpp). A negative power, the default, falls fast first.
- Words: [envelope (adsr)](../core/words-lists.md#technique-words)

### Velocity track
- Keys: `velocity_track`
- Effect: Sets how much the key velocity scales the voice gain before the squaring. At 0 % velocity has no effect. At 100 % the factor equals the velocity (source: synth_voice_handler.cpp).
- Words: none in the word lists

### Voice amplitude
- Keys: `voice_amplitude`
- Effect: Scales the voice gain from 0 to 1 before the squaring, together with envelope 1 and the velocity factor (source: synth_voice_handler.cpp).
- Words: none in the word lists
