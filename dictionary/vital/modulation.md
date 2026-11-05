Facts checked on the open source of build 1.0.6 (commit 636ca0e) unless an entry says measured.
A source is a signal that moves a control by itself: an LFO, a random LFO, an envelope, a macro or a performance input.

## How modulation works

A route joins one source to one destination control. The matrix holds 64 routes, and route k owns the keys `modulation_k_*` (source: synth_constants.h, modulation_connection_processor.cpp).

The engine offers 32 sources: 8 LFOs, 4 random LFOs, 6 envelopes, 4 macros, `note`, `note_in_octave`, `aftertouch`, `velocity`, `slide`, `lift`, `mod_wheel`, `pitch_wheel`, `random` and `stereo` (source: synth_voice_handler.cpp).

A route moves its destination by source x amount x (destination max - min), in the destination's stored units. Amount 0.25 on a control that spans 128 stored units moves it by 32, for example `filter_1_cutoff` (source: synth_base.cpp, synth_parameters.cpp). The amount field shows this product (measured on 1.0.7).

The modulation adds to the stored value first. The control's own scale, such as 2 to the power of the stored value, applies afterwards (source: synth_module.cpp).

The 8 drawable LFOs each hold a shape of up to 100 points. The shape is stored upside down, so the output is 1 - y (source: line_generator.cpp). The default shape is a triangle that starts at 0 and peaks at 1 mid-cycle (source: line_generator.cpp, synth_voice_handler.cpp).

The 4 random LFOs output values from 0 to 1 in four styles: Perlin, Sample and Hold, Sine Interpolate and Lorenz (source: random_lfo.cpp).

Each voice runs its own copy of every LFO. The rate comes from one of five sync modes (source: synth_lfo.cpp, operators.cpp).

## Controls

### LFO rate
- Keys: `lfo_N_frequency`
- Effect: Sets the cycles per second in Seconds mode. Hz is 2 to the power of the stored value, and the plugin shows the period (source: synth_parameters.cpp).
- Words: [lfo (modulation)](../core/words-lists.md#technique-words)

### LFO sync mode and tempo
- Keys: `lfo_N_sync`, `lfo_N_tempo`
- Effect: Sync picks the rate source: Seconds uses the rate control, Tempo, Dotted and Triplets follow the host tempo, and Keytrack follows the note (source: operators.cpp). Tempo menu index 0 (Freeze) stops the LFO. Index i runs 2^(i - 8) cycles per beat, and the menu stores the index of its label (measured on 1.0.7; source: synth_constants.h). Dotted runs at 2/3 of that rate and Triplets at 3/2 (source: operators.cpp).
- Words: [lfo (modulation)](../core/words-lists.md#technique-words)

### Keytrack (LFO and random LFO)
- Keys: `lfo_N_keytrack_transpose`, `lfo_N_keytrack_tune`, `random_N_keytrack_transpose`, `random_N_keytrack_tune`
- Effect: Used only in Keytrack mode. The LFO runs at the frequency of the played note plus Transpose (semitones) plus Tune, so a held key sets the LFO speed (source: operators.cpp).
- Words: [lfo (modulation)](../core/words-lists.md#technique-words)

### LFO sync type and phase
- Keys: `lfo_N_sync_type`, `lfo_N_phase`
- Effect: Trigger restarts the shape on each note and loops it. Sync restarts it at the point the host clock has reached. Envelope plays it once and holds the end. Sustain Envelope pauses at the Phase point while the key is held. Loop Point jumps back to the Phase point at the end. Loop Hold loops from the start to the Phase point while held, then plays on and stops. In the other types, Phase shifts the start (source: synth_lfo.cpp).
- Words: [lfo (modulation)](../core/words-lists.md#technique-words)

### LFO stereo
- Keys: `lfo_N_stereo`
- Effect: Shifts the left channel's phase by half the value and the right channel's phase by minus half, so the two sides move out of step (source: synth_lfo.cpp).
- Words: [lfo (modulation)](../core/words-lists.md#technique-words)

### LFO delay and fade in
- Keys: `lfo_N_delay_time`, `lfo_N_fade_time`
- Effect: Delay holds the LFO at its start for the set seconds after a note starts. Fade In eases the LFO from its start value to full swing over the set seconds (source: synth_lfo.cpp).
- Words: [lfo (modulation)](../core/words-lists.md#technique-words)

### LFO smoothing
- Keys: `lfo_N_smooth_mode`, `lfo_N_smooth_time`
- Effect: The panel button swaps Fade In for Smooth. With Smooth on, the output follows the shape through a low-pass and Fade In has no effect. Time is 2 to the power of the stored value, and the low-pass half-life is 0.2 of it (source: synth_lfo.cpp, synth_parameters.cpp, synth_strings.h).
- Words: [lfo (modulation)](../core/words-lists.md#technique-words)

### Random LFO style
- Keys: `random_N_style`
- Effect: Perlin glides between random targets. Sample and Hold jumps between them in steps. Sine Interpolate eases from one random value to the next along half a sine cycle. Lorenz follows a chaotic attractor (source: random_lfo.cpp, futils.h).
- Words: [lfo (modulation)](../core/words-lists.md#technique-words)

### Random LFO rate
- Keys: `random_N_frequency`, `random_N_sync`, `random_N_tempo`
- Effect: Sets how often the random LFO picks a new target. Sync and Tempo work as on the LFO (source: random_lfo_module.cpp, operators.cpp).
- Words: [lfo (modulation)](../core/words-lists.md#technique-words)

### Random LFO stereo and sync type
- Keys: `random_N_stereo`, `random_N_sync_type`
- Effect: Stereo off gives both channels the same random values and on gives each its own. Sync Type on locks one shared random stream to the host clock, and notes no longer reseed it (source: random_lfo.cpp).
- Words: [lfo (modulation)](../core/words-lists.md#technique-words)

### Macros and mod wheel
- Keys: `macro_control_1`, `macro_control_2`, `macro_control_3`, `macro_control_4`, `mod_wheel`
- Effect: Each is a source set by hand, from 0 to 1. One macro can drive any number of routes. MIDI controller 1 sets the mod wheel (source: synth_voice_handler.cpp, midi_manager.cpp).
- Words: none in the word lists

### Route amount
- Keys: `modulation_N_amount`
- Effect: Scales how far the route moves its destination, as a share of the destination's full span. A negative amount flips the direction. The value is clamped to -1 to 1 (source: modulation_connection_processor.cpp).
- Words: none in the word lists

### Route bipolar
- Keys: `modulation_N_bipolar`
- Effect: Off, the source adds from 0 up to its full value. On, the source subtracts 0.5 first, so it pushes both up and down around the set value. The editor turns it on for LFO, stereo, random and pitch sources (source: modulation_connection_processor.cpp, synth_types.cpp).
- Words: none in the word lists

### Route power
- Keys: `modulation_N_power`
- Effect: Bends the response. 0 is a straight line. A positive stored value rises early and flattens near the top, and a negative one rises late. The bend mirrors around the centre in bipolar routes (source: modulation_connection_processor.cpp, futils.h).
- Words: none in the word lists

### Route stereo and bypass
- Keys: `modulation_N_stereo`, `modulation_N_bypass`
- Effect: Stereo on inverts the route's amount on the right channel only. Bypass on mutes the route and keeps its settings (source: modulation_connection_processor.cpp).
- Words: none in the word lists
