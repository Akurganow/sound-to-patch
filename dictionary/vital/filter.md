Facts checked on the open source of build 1.0.6 (commit 636ca0e) unless an entry says measured. A filter removes part of the spectrum of a sound. Its cutoff marks the edge of that part.

## How the filter layer works

Vital has two voice filters and one FX filter. Each voice filter runs once per voice. The FX filter sits in the effect chain, runs once for the whole sound, and has its own switch. The chain order can change (source: filters_module.cpp, reorderable_effect_chain.cpp, filter_module.cpp).

An oscillator or sample destination menu offers Filter 1, Filter 2, Filter 1+2, Effects and Direct Out. A source sent to an off filter skips it and goes to the effects. A source sent to both filters skips them only when both are off (source: producers_module.cpp, synth_strings.h).

By default the two voice filters run in parallel and their outputs add. A filter input switch chains them instead (source: filters_module.cpp).

Each filter has one of 8 models. The stored index follows this order: Analog 0, Dirty 1, Ladder 2, Digital 3, Diode 4, Formant 5, Comb 6, Phaser 7. The model sets the algorithm and what Blend does (source: synth_constants.h, filter_module.cpp).

Each model has its own style list, stored from 0. Analog, Dirty, Ladder and Digital share 12dB, 24dB, Notch Blend, Notch Spread and B/P/N. Diode has Low Shelf and Low Cut. Formant has AOIE and AIUO. Comb has Low High Comb, Low High Flange+, Low High Flange-, Band Spread Comb, Band Spread Flange+ and Band Spread Flange-. Phaser has Positive and Negative (source: filter_section.cpp, synth_strings.h, comb_filter.h).

The cutoff is a note number. Hz = 8.1757989156 x 2^(stored/12), so 60 is 261.63 Hz and 12 more doubles the frequency. The plugin shows stored minus 60 semitones (measured on 1.0.7 for `filter_1_cutoff`) (source: common.h, utils.h).

Key tracking adds key track x (note - 60) semitones to the cutoff. The note is the played note after pitch bend (source: filter_module.cpp, synth_voice_handler.cpp).

Resonance maps to a different internal value in each model. Analog uses the square root of the knob. Digital and Diode use its cube. Ladder uses the knob on 12dB and a sine curve on the other styles. Phaser uses the knob linearly. These internal values are not decibels, so the peak height in dB is not known (source: sallen_key_filter.cpp, digital_svf.cpp, diode_filter.cpp, ladder_filter.cpp, phaser_filter.cpp).

The editor hides some controls per model. Formant hides cutoff, resonance, key track and drive. Comb hides drive. Only Comb shows the blend offset (source: filter_section.cpp).

## Control entries

### Filter switch
- Keys: `filter_N_on`, `filter_fx_on`
- Effect: Turns a filter on. An off voice filter passes no sound, so its sources go to the effects instead. The FX switch is a control of the effect chain (source: filter_module.cpp, producers_module.cpp, reorderable_effect_chain.cpp)
- Words: none in the word lists

### Filter input
- Keys: `filter_N_filter_input`
- Effect: Chains the filters. With `filter_2_filter_input` on, filter 2 receives the filter 1 output plus its own sources, and only filter 2 reaches the output. `filter_1_filter_input` does the reverse. The chain works only while the receiving filter is on; otherwise the filters run in parallel (source: filters_module.cpp).
- Words: none in the word lists
- Tuning: When both are set, filter 1 wins. The editor clears the other switch when one turns on. Its button reads the number of the filter that feeds it (source: filters_module.cpp, synthesis_interface.cpp, filter_section.cpp).

### Model and style
- Keys: `filter_N_model`, `filter_N_style`, `filter_fx_model`, `filter_fx_style`
- Effect: Model picks the filter algorithm, and style picks a variant of it. Both change what the other controls do. The style lists are in the section above (source: filter_module.cpp, filter_section.cpp).
- Words: none in the word lists

### Cutoff
- Keys: `filter_N_cutoff`, `filter_fx_cutoff`
- Effect: Sets the edge frequency of the filter. In Comb it sets the delay length, one period of the cutoff frequency. The note law is in the section above. The value box shows stored minus 60, in semitones (measured on 1.0.7 for `filter_1_cutoff`; source: synth_parameters.cpp, comb_filter.cpp, common.h).
- Words: [filter (cutoff)](../core/words-lists.md#technique-words)

### Resonance
- Keys: `filter_N_resonance`, `filter_fx_resonance`
- Effect: Boosts the sound near the cutoff in most models, and sets the feedback in Comb. In Comb styles the feedback runs from -1 to 1, so 50 percent is zero feedback. In Flange styles it runs from 0 to 1 (source: comb_filter.cpp).
- Words: [resonance](../core/words-lists.md#technique-words)

### Key track
- Keys: `filter_N_keytrack`, `filter_fx_keytrack`
- Effect: Moves the cutoff with the played note. At 100 percent the cutoff rises one semitone per semitone above note 60. Negative values move it the other way (source: filter_module.cpp).
- Words: none in the word lists

### Blend
- Keys: `filter_N_blend`, `filter_fx_blend`
- Effect: Morphs the filter response between two ends. The stored range is 0 to 2 and the middle is 1. The editor icons show the ends. 12dB, 24dB and Notch Blend show low pass and high pass. Notch Spread and B/P/N show band pass and notch (source: filter_section.cpp).
- Words: none in the word lists
- Tuning: Digital and Ladder in 12dB and 24dB give low pass at 0, band pass at 1, high pass at 2. Diode Low Shelf raises the low cut amount with blend. Diode Low Cut moves the low cut corner up toward the cutoff. Phaser picks the 1, 3 or 5 peak output at 0, 1 or 2. Comb Low High weights a low and a high feedback path. Comb Band Spread widens the band to 8 octaves each side of its centre at 2 (source: digital_svf.cpp, ladder_filter.cpp, diode_filter.cpp, phaser_filter.cpp, comb_filter.cpp).

### Drive
- Keys: `filter_N_drive`, `filter_fx_drive`
- Effect: Adds gain in decibels before the filter's own saturation. Digital and Diode lower their output by the square root of the drive gain. Comb and Formant ignore drive (source: synth_filter.cpp, digital_svf.cpp, diode_filter.cpp, filter_module.cpp).
- Words: none in the word lists

### Mix
- Keys: `filter_N_mix`, `filter_fx_mix`
- Effect: Crossfades between the sound entering the filter and the filtered sound. The value is held between 0 and 1 (source: filter_module.cpp).
- Words: none in the word lists

### Comb blend offset
- Keys: `filter_N_blend_transpose`, `filter_fx_blend_transpose`
- Effect: Comb only. Moves the feedback filter above the cutoff by this many semitones. In Band Spread it moves the centre of the band. The Phaser model receives the value and does not use it (source: comb_filter.cpp, phaser_filter.cpp, filter_section.cpp).
- Words: none in the word lists

### Formant position
- Keys: `filter_N_formant_x`, `filter_N_formant_y`, `filter_fx_formant_x`, `filter_fx_formant_y`
- Effect: Formant only. Moves across a square whose four corners hold stored vowel shapes. Each point mixes the corners and sets the peaks of the formants (source: formant_filter.cpp).
- Words: [formant (vowel)](../core/words-lists.md#technique-words)

### Formant shape
- Keys: `filter_N_formant_transpose`, `filter_N_formant_resonance`, `filter_N_formant_spread`, `filter_fx_formant_transpose`, `filter_fx_formant_resonance`, `filter_fx_formant_spread`
- Effect: Transpose shifts all formant peaks by semitones. Resonance scales how sharp each peak is. Spread moves each peak toward a shared centre note, 80 (source: formant_filter.cpp).
- Words: [formant (vowel)](../core/words-lists.md#technique-words)
