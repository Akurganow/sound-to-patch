Facts checked on the open source of build 1.0.6 (commit 636ca0e) unless an entry says measured. The sound of each warp and morph is not measured; the lists give the mechanism.

## How the oscillator works

Vital has 3 wavetable oscillators and one sample oscillator. A table holds 257 frames of 2048 samples. Wave Frame picks one, from position 0 to 256 (source: synth_constants.h, wave_frame.h).
The frequency morph reshapes the harmonics of the frame first. The phase distortion then warps how the oscillator reads that wave (source: synth_oscillator.cpp).
Each oscillator has one phase-distortion slot (13 types, with None) and one frequency-morph slot (12 types, with None) (source: synth_oscillator.h).
Level and Pan come after the warps, then Destination. An FM or RM source hands over its signal before its own Level and Pan (source: synth_oscillator.cpp, producers_module.cpp).
Destination is an index: 0 Filter 1, 1 Filter 2, 2 Filter 1+2, 3 Effects, 4 Direct Out (0 and 3 measured on 1.0.7; source: synth_strings.h). Direct Out skips the effects (source: sound_engine.cpp).
A source routed to a switched-off filter goes to the effects instead. Filter 1+2 does so only when both filters are off (source: producers_module.cpp).
The menu offers indices 0 to 4 only. The registry allows up to 14, but the producer code sends indices 5 and above to no output (source: oscillator_section.cpp, producers_module.cpp).

## Phase distortion types

Amount runs 0 to 100%. Spread changes it per unison pair (see Distortion Amount, Phase and Spread).
- None: leaves the read position alone.
- Sync and Formant: multiply the read speed by 2^(8 x (Amount - 0.5)). That is 1/16 at 0%, 1 at 50% and 16 at 100%. Formant also fades the wave with a half-sine window (source: synth_oscillator.cpp).
- Quantize: holds the read position in steps. A cycle has 2^(1 + 27.2 x (1 - Amount)^3) steps, so 100% gives 2 steps and the count grows steeply as Amount falls (source: synth_oscillator.cpp).
- Bend: reshapes the read position along a smooth curve. 50% changes nothing, 100% reads the early cycle faster, 0% reads it slower (source: synth_oscillator.cpp).
- Squeeze: compresses one part of the cycle and stretches the rest around a pivot. 50% changes nothing (source: synth_oscillator.cpp).
- Pulse: scales the read position by 1/(1 - Amount). The whole table then plays in a narrower part of the cycle, and a window mutes part of the rest. 0% changes nothing (source: synth_oscillator.cpp).
- FM (Osc A, Osc B, Sample): shifts the read position by the signal of the other source. Amount is squared first. Full Amount and a signal of amplitude 1 shift it by 6 table cycles (source: synth_oscillator.cpp).
- RM (Osc A, Osc B, Sample): multiplies the wave by a mix of 1 and the other signal. 0% changes nothing, 100% is full ring modulation (source: synth_oscillator.cpp).
- The menu names the source. Oscillator 1 offers oscillators 2 and 3. Oscillator 2 offers 1 and 3. Oscillator 3 offers 1 and 2 (source: oscillator_section.cpp, producers_module.h).

## Frequency morph types

Amount runs 0 to 100%. A morph moves or removes harmonics of the frame before it plays (source: synth_oscillator.cpp, spectral_morph.h).
- None: leaves the spectrum alone.
- Vocode and Formant Scale: harmonic n takes the amplitude of harmonic n x factor, so the spectral shape slides. Formant Scale runs from factor 4 at 0% to 1/4 at 100%. Vocode runs from 2 to 1/2 and also scales with the note frequency.
- Harmonic Stretch: harmonic n moves to (n - 1) x factor + 1, with factor 2^(8 x (Amount - 0.5)). 50% changes nothing.
- Inharmonic Stretch: the same move, but the shift grows with the harmonic number, so partials leave whole multiples. 50% changes nothing.
- Smear: mixes each harmonic with a running amplitude carried up from the lower harmonics. 0% changes nothing.
- Random Amplitudes: scales each harmonic by a fixed pseudo-random gain from a seeded table. Every note gets the same pattern.
- Low Pass and High Pass: keep the harmonics below (Low Pass) or above (High Pass) a cutoff harmonic of 2^(10 x Amount).
- Phase Disperse: shifts the phase of each harmonic by an amount that grows with the square of its distance from harmonic 12. 50% changes nothing.
- Shepard Tone: fades the odd harmonics out and gives each even harmonic the amplitude of the harmonic at half its number, an octave-up stretch. The read rate falls by up to an octave to match. Amount wraps at the top, so a rising sweep glides on.
- Spectral Time Skew: each harmonic reads a different frame, and higher harmonics shift further as Amount rises.

## Controls

### Wave frame
- Keys: `osc_N_wave_frame`
- Effect: Picks which frame of the table sounds. Whole frames 0 to 256 each hold one cycle of 2048 samples (source: synth_constants.h, synth_oscillator.cpp).
- Words: [oscillator waveforms (saw, square, sine, pulse, triangle)](../core/words-lists.md#technique-words), [wavetable](../core/words-lists.md#technique-words)

### Switch and destination
- Keys: `osc_N_on`, `osc_N_destination`
- Effect: Switch turns the oscillator on or off and clears its output when off. Destination picks where the sound goes; the routing rules are above (source: oscillator_module.cpp).
- Words: none in the word lists

### Level and pan
- Keys: `osc_N_level`, `osc_N_pan`
- Effect: Level sets loudness; the engine squares the stored value to get gain (measured on the open engine). Pan is equal-power: centre gives gain 1 to both channels, a hard pan gives 1.41 (+3 dB) to one channel (source: futils.h).
- Words: none in the word lists

### Pitch of the oscillator
- Keys: `osc_N_transpose`, `osc_N_tune`, `osc_N_midi_track`
- Effect: Transpose shifts the pitch in semitones and Tune in cents. With Midi Track off, the oscillator ignores the key and plays MIDI note 48 plus both (source: synth_oscillator.cpp).
- Words: none in the word lists

### Transpose quantize
- Keys: `osc_N_transpose_quantize`
- Effect: A bit mask. Bits 0 to 11 mark allowed notes of an octave, and 0 turns snapping off. With bit 12 set, the snap acts on the played note plus Transpose, and bit 0 is C. Clear, only Transpose snaps, and bit n allows n semitones (source: poly_utils.h).
- Words: none in the word lists

### Phase
- Keys: `osc_N_phase`, `osc_N_random_phase`
- Effect: Phase offsets the point of the cycle each voice reads. Phase Randomization scatters the start point of each voice at every note; at 0% all notes start at the same point (source: synth_oscillator.cpp).
- Words: none in the word lists

### Phase distortion type
- Keys: `osc_N_distortion_type`
- Effect: Picks the warp of the read position. Each of the 13 types is described above under Phase distortion types.
- Words: [fm](../core/words-lists.md#technique-words), [sync (oscillator sync)](../core/words-lists.md#technique-words)

### Distortion Amount, Phase and Spread
- Keys: `osc_N_distortion_amount`, `osc_N_distortion_phase`, `osc_N_distortion_spread`
- Effect: Amount sets the strength of the warp. Phase moves the point of the cycle where Sync, Formant, Quantize, Bend, Squeeze and Pulse act; None, FM and RM ignore it. Spread moves Amount from no shift at the first unison pair to the Spread percent at the last, from 3 voices up (source: synth_oscillator.cpp).
- Words: none in the word lists

### Frequency morph type
- Keys: `osc_N_spectral_morph_type`
- Effect: Picks how the harmonics of the frame change. Each of the 12 types is described above under Frequency morph types.
- Words: none in the word lists

### Frequency morph Amount and Spread
- Keys: `osc_N_spectral_morph_amount`, `osc_N_spectral_morph_spread`
- Effect: Amount sets the strength of the morph. Spread moves it from no shift at the first unison pair to the Spread percent at the last. It works only with Spectral Unison on and 3 voices or more (source: synth_oscillator.cpp).
- Words: none in the word lists

### Wired-only and display controls
- Keys: `osc_N_smooth_interpolation`, `osc_N_view_2d`
- Effect: Smooth Interpolation (button HI-RES WAVETABLE) feeds the oscillator, but no code of build 1.0.6 reads it. View 2D picks the editor view of the table (0 = 3D, 1 = 2D, 2 = SP) and leaves the sound alone (source: oscillator_module.cpp, oscillator_advanced_section.cpp, synth_strings.h).
- Words: none in the word lists

### Sample oscillator
- Keys: `sample_on`, `sample_level`, `sample_pan`, `sample_destination`
- Effect: A fourth source beside the 3 oscillators. Its default sample is one second of white noise: random values between -0.9 and 0.9 at 44.1 kHz. Level squares like the oscillator Level; Pan and Destination work as above (source: sample_source.cpp, sample_module.cpp, producers_module.cpp).
- Words: [noise](../core/words-lists.md#technique-words)

### Sample playback
- Keys: `sample_loop`, `sample_bounce`, `sample_keytrack`, `sample_random_phase`
- Effect: With Loop off, the sample plays once. Bounce plays it forward, then backward. Keytrack shifts the pitch by the played note relative to note 60. Random Phase starts each note at a random point (source: sample_source.cpp, common.h).
- Words: none in the word lists

### Sample pitch
- Keys: `sample_transpose`, `sample_tune`, `sample_transpose_quantize`
- Effect: Transpose and Tune shift the pitch by changing the playback speed. The shift clamps at 96 semitones either way. Quantize uses the same bit mask as the oscillator (source: sample_source.cpp).
- Words: none in the word lists
