Facts checked on the open source of build 1.0.6 (commit 636ca0e) unless an entry says measured. Distortion Spread and Frequency Morph Spread sit in the oscillator file, with the other warp controls.

## How unison works

Unison stacks 1 to 16 copies of the oscillator, each slightly out of tune. The result is a thicker, wider sound. The stored voice count is rounded and limited to 1 to 16 (source: synth_oscillator.cpp).
Voices come in pairs. Each pair raises its left-channel voice and lowers its right-channel voice by the same cents, or the reverse. The order alternates from pair to pair (source: synth_oscillator.cpp).
An odd count has a centre voice, which plays the same phase in both channels. With Phase Randomization above 0%, every other voice starts at its own random phase (source: synth_oscillator.cpp).
The outer pair sits at an offset of Detune Range x Unison Detune squared, in cents (measured on the open engine). The on-screen percent is the stored value squared (measured on 1.0.7).
The defaults give 2 x 20 = 40 cents. The maximum is 48 x 100 = 4800 cents.
Pair i of n voices sits at position t = (2i + bump) / max(1, n - 1) of the outer offset. Bump is 1 for an even count and 0 for an odd one. Detune Power bends t (source: synth_oscillator.cpp, futils.h).
At the default power 1.5, 4 voices sit at 0.186 and 1.00 of the outer offset. 8 voices sit at 0.069, 0.259, 0.551 and 1.00 (law measured on the open engine for 4 and 8 voices).

## Stack style ratios

A stack style multiplies the pitch of each pair, innermost pair first. The detune comes on top (source: synth_oscillator.cpp).
- Unison: every pair x1. Center Drop 12 and Center Drop 24: the first pair x1/2 and x1/4, the others x1.
- Octave: x1 and x2, alternating. 2x Octave: x1, x2 and x3, repeating.
- Power Chord: x1, x1.498 and x2, repeating. 2x Power Chord: x1, x1.498, x2, x2.997, x4, then x1, x1.498, x2.
- Major Chord: x1, x1.260, x1.498 and x2, repeating. Minor Chord: x1, x1.189, x1.498 and x2, repeating.
- Harmonics: x1 up to x8, one step per pair. Odd Harmonics: x1, x3, x5 up to x15.

## Controls

### Voices
- Keys: `osc_N_unison_voices`
- Effect: Sets how many copies play, from 1 to 16. One voice gives a single centred copy; 2 voices give one detuned pair (source: synth_oscillator.cpp).
- Words: [unison (detune)](../core/words-lists.md#technique-words)

### Detune
- Keys: `osc_N_unison_detune`, `osc_N_detune_range`, `osc_N_detune_power`
- Effect: Unison Detune scales the spread of the voices inside Detune Range, in cents. Detune Power above 0 packs the inner pairs near the centre; below 0 spreads them out; at 0 spaces the voices evenly (source: synth_oscillator.cpp, futils.h).
- Words: [unison (detune)](../core/words-lists.md#technique-words)

### Stack style
- Keys: `osc_N_stack_style`
- Effect: Tunes the pairs to the ratios of a chord, an octave stack or a harmonic series. The ratios are listed above (source: synth_oscillator.cpp).
- Words: none in the word lists

### Blend
- Keys: `osc_N_unison_blend`
- Effect: With 3 voices or more, sets the level of the outer pairs against the innermost pair. At 0% only the innermost pair sounds. At 100% an outer pair has 1.5 times its gain. The total power stays constant (source: synth_oscillator.cpp).
- Words: none in the word lists

### Spectral unison and frame spread
- Keys: `osc_N_spectral_unison`, `osc_N_frame_spread`
- Effect: With Spectral Unison on, each pair reads its own frame and morph value. Frame Spread moves the frame from no shift at the first pair to the full Frame Spread (-128 to 128 frames) at the last. With it off, all pairs read the same wave. Needs 3 voices or more (source: synth_oscillator.cpp).
- Words: none in the word lists

### Stereo spread
- Keys: `osc_N_stereo_spread`
- Effect: At 100% each channel keeps its own voices. Lower values mix the channels with an equal-power fade, and 0% sends the same mono sum to both (source: synth_oscillator.cpp, futils.h).
- Words: none in the word lists

### Stereo mode and routing
- Keys: `stereo_mode`, `stereo_routing`
- Effect: These act on the final stereo output, after the effects and before master volume. SPREAD at 100% keeps the channels apart; 0% sums them to one mono signal. ROTATE turns the pair through a full circle across 0% to 100%, so both ends match and 50% inverts the polarity (source: sound_engine.cpp, operators.cpp).
- Words: none in the word lists
