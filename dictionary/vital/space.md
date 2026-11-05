Facts checked on the open source of build 1.0.6 (commit 636ca0e) unless an entry says measured. Stored values of cutoff and center keys are semitones.

## How the effects work

Vital has nine effects, one instance each. Each has a switch named `<effect>_on`. An effect that is off passes the sound through unchanged. (source: reorderable_effect_chain.cpp)

The effects run in series. Each takes the output of the effect above it, so it only hears what the effects above made. (source: reorderable_effect_chain.cpp)

The default order is Chorus, Compressor, Delay, [Distortion](distortion.md), EQ, [Filter](filter.md), Flanger, Phaser, Reverb. (source: synth_strings.h)

Rule of this reference, not a source fact: build and check the dry sound first, then switch on reverb and delay last.

Sync menus pick the rate source. Seconds uses the free Frequency key, where Hz = 2^stored. (source: synth_module.cpp, operators.cpp)

Tempo, Tempo Dotted and Tempo Triplets make one cycle last one note value of the Tempo menu at the song tempo. Dotted lasts 1.5 times as long, Triplets two thirds as long. Freeze stops the motion. (source: operators.cpp, synth_constants.h)

Cutoff, center and filter keys follow the note law: Hz = 8.1757989156 x 2^(stored/12). (source: utils.h, delay.cpp, digital_svf.cpp, flanger_module.cpp, phaser_filter.h, reverb.cpp)

Chorus, delay, flanger and reverb mixes are equal-power crossfades of dry and wet. Phaser and compressor mixes are straight blends. (source: chorus_module.cpp, delay.cpp, reverb.cpp, phaser.cpp, compressor.cpp)

## Controls

### Effect order
- Keys: `effect_chain_order`
- Effect: Codes any order of the nine effects as one number; 0 keeps the default order. Number the effects Chorus 0, Compressor 1, Delay 2, Distortion 3, EQ 4, Filter 5, Flanger 6, Phaser 7, Reverb 8. With p the numbers from top to bottom, start at code 0 and for each i from 1 to 8 set code = code x (i + 1) + idx, where idx counts the effects above p[i] with a higher number. Reverb first and the rest in default order gives 260650, computed from this rule. (source: utils.cpp; measured on 1.0.7)
- Words: none in the word lists

### Chorus time and motion
- Keys: `chorus_on`, `chorus_voices`, `chorus_delay_1`, `chorus_delay_2`, `chorus_mod_depth`, `chorus_frequency`, `chorus_sync`, `chorus_tempo`
- Effect: Chorus Voices sets how many pairs of delayed copies run, 1 to 4. Each pair mixes one copy at Chorus Delay 1 and one at Chorus Delay 2, where delay ms = 1000 x 2^stored. A sine sweep moves the delays; Chorus Mod Depth scales its span up to 30 ms. Chorus Frequency, Chorus Sync and Chorus Tempo set the sweep rate. (source: chorus_module.cpp)
- Words: [chorus (ensemble)](../core/words-lists.md#technique-words)

### Chorus feedback, filter and mix
- Keys: `chorus_feedback`, `chorus_cutoff`, `chorus_spread`, `chorus_dry_wet`
- Effect: Chorus Feedback returns part of each copy into its delay line; a negative value flips polarity on each repeat. Chorus Filter Cutoff and Chorus Filter Spread band-limit what enters the delay lines. The band centres on the cutoff and reaches Spread x 8 octaves to each side. (source: delay.cpp, chorus_module.cpp)
- Words: [chorus (ensemble)](../core/words-lists.md#technique-words)

### Delay time and style
- Keys: `delay_on`, `delay_style`, `delay_frequency`, `delay_sync`, `delay_tempo`, `delay_aux_frequency`, `delay_aux_sync`, `delay_aux_tempo`
- Effect: Delay time in seconds = 2^(-stored), up to 4 s. In a Tempo mode one echo lasts one note value. Style Mono times both sides with Delay Frequency. Stereo, Ping Pong and Mid Ping Pong time the right side with Delay Frequency 2, Delay Sync 2 and Delay Tempo 2. Ping Pong and Mid Ping Pong swap sides on each repeat. Ping Pong sums the input to mono first. (source: delay.cpp, delay_module.cpp, synth_module.cpp)
- Words: [delay (echo)](../core/words-lists.md#technique-words)

### Delay feedback, filter and mix
- Keys: `delay_feedback`, `delay_filter_cutoff`, `delay_filter_spread`, `delay_dry_wet`
- Effect: Delay Feedback returns part of each echo into the line; negative values flip polarity on each repeat. A clipper in the loop caps the level of the repeats. Delay Filter Cutoff and Delay Filter Spread band-limit what enters the line, with the same band rule as the chorus. (source: delay.cpp)
- Words: [delay (echo)](../core/words-lists.md#technique-words)

### Flanger
- Keys: `flanger_on`, `flanger_center`, `flanger_dry_wet`, `flanger_feedback`, `flanger_frequency`, `flanger_mod_depth`, `flanger_phase_offset`, `flanger_sync`, `flanger_tempo`
- Effect: A short delay swept by a triangle wave and fed back on itself. Flanger Center sets the base delay as one cycle of its frequency, so a higher center is a shorter delay. Flanger Mod Depth sets how far the delay swings around it. Flanger Phase Offset shifts the sweep of the right side by a fraction of a cycle. (source: flanger_module.cpp)
- Words: [phaser (flanger)](../core/words-lists.md#technique-words)

### Phaser
- Keys: `phaser_on`, `phaser_blend`, `phaser_center`, `phaser_dry_wet`, `phaser_feedback`, `phaser_frequency`, `phaser_mod_depth`, `phaser_phase_offset`, `phaser_sync`, `phaser_tempo`
- Effect: A filter whose cutoff sweeps on a triangle wave around Phaser Center by plus and minus Phaser Mod Depth semitones. Phaser Feedback drives the filter resonance and Phaser Blend its pass blend. Phaser Phase Offset shifts the sweep between the two sides. The wet sound blends with the dry. (source: phaser.cpp, phaser_module.cpp)
- Words: [phaser (flanger)](../core/words-lists.md#technique-words)

### Reverb decay and size
- Keys: `reverb_on`, `reverb_decay_time`, `reverb_size`, `reverb_delay`, `reverb_chorus_amount`, `reverb_chorus_frequency`
- Effect: Reverb Decay Time sets when the tail has fallen to 0.001 of its level (60 dB). Seconds = 2^stored, and the engine holds it between 0.1 s and 100 s. Reverb Size scales the lengths of the internal delay lines. Reverb Delay holds the wet sound back by 0 to 0.3 s. Reverb Chorus Amount and Reverb Chorus Frequency wobble the delay-line lengths: how far and how fast. (source: reverb.cpp, reverb.h)
- Words: [reverb](../core/words-lists.md#technique-words)

### Reverb tone and mix
- Keys: `reverb_pre_low_cutoff`, `reverb_pre_high_cutoff`, `reverb_low_shelf_cutoff`, `reverb_low_shelf_gain`, `reverb_high_shelf_cutoff`, `reverb_high_shelf_gain`, `reverb_dry_wet`
- Effect: The two Pre cutoffs band-limit the sound before it enters the reverb: nothing below the low one or above the high one feeds the tail. The Low and High shelves sit inside the feedback loop. Each pass cuts the lows below the low cutoff, or the highs above the high cutoff, by the shelf gain (0 to -6 dB). That region dies away faster. (source: reverb.cpp)
- Words: [reverb](../core/words-lists.md#technique-words)

### Compressor bands, timing and mix
- Keys: `compressor_on`, `compressor_enabled_bands`, `compressor_attack`, `compressor_release`, `compressor_low_gain`, `compressor_band_gain`, `compressor_high_gain`, `compressor_mix`
- Effect: Crossovers at 120 Hz and 2500 Hz give Low, Band and High. Multiband runs all three. Low Band splits at 120 Hz only, with the Low and Band settings. High Band splits at 2500 Hz only, with the Band and High settings. Single Band runs the whole sound on the Band settings. Attack and Release time = base ms x e^(8 x stored - 4), with bases Low 2.8 and 40, Band 1.4 and 28, High 0.7 and 15. Each gain adds -30 to 30 dB after compression. Compressor Mix blends the input with the compressed, gained sound. (source: compressor.cpp)
- Words: [compression (multiband compression)](../core/words-lists.md#technique-words)

### Compressor thresholds and ratios
- Keys: `compressor_low_upper_threshold`, `compressor_band_upper_threshold`, `compressor_high_upper_threshold`, `compressor_low_lower_threshold`, `compressor_band_lower_threshold`, `compressor_high_lower_threshold`, `compressor_low_upper_ratio`, `compressor_band_upper_ratio`, `compressor_high_upper_ratio`, `compressor_low_lower_ratio`, `compressor_band_lower_ratio`, `compressor_high_lower_ratio`
- Effect: Each band has two thresholds. Above the upper threshold the gain falls: upper ratio 0 leaves the level, 1 holds it at the threshold. Below the lower threshold, a lower ratio above 0 raises the level toward the threshold, and one below 0 lowers it. The gain change stops at 32 times. (source: compressor.cpp)
- Words: [compression (multiband compression)](../core/words-lists.md#technique-words)

### EQ
- Keys: `eq_on`, `eq_low_mode`, `eq_low_cutoff`, `eq_low_gain`, `eq_low_resonance`, `eq_band_mode`, `eq_band_cutoff`, `eq_band_gain`, `eq_band_resonance`, `eq_high_mode`, `eq_high_cutoff`, `eq_high_gain`, `eq_high_resonance`
- Effect: Three filters run in series: low, band, high. Each Mode picks the filter: low Shelf or High Pass, band Shelf or Notch, high Shelf or Low Pass. Cutoff and Resonance apply in both modes. Gain, within 15 dB either way, acts only in Shelf mode. High Pass, Notch and Low Pass ignore it. (source: equalizer_module.cpp, digital_svf.h)
- Words: [eq](../core/words-lists.md#technique-words)
