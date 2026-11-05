Facts checked on the open source of build 1.0.6 (commit 636ca0e) unless an entry says measured. Distortion bends the waveform and adds harmonics. Drive sets how hard the bend is.

## How the distortion effect works

The distortion effect sits in the effect chain, runs once for the whole sound, and has its own switch. The chain order can change (source: distortion_module.cpp, reorderable_effect_chain.cpp).

The effect has 6 types: Soft Clip, Hard Clip, Linear Fold, Sine Fold, Bit Crush and Down Sample. Drive runs from -30 dB to 30 dB. Drive shows in decibels (measured on 1.0.7) (source: distortion.h, synth_strings.h).

Soft Clip multiplies the signal by the drive gain and rounds the peaks smoothly. Hard Clip cuts the peaks flat at full level (source: distortion.cpp).

Linear Fold and Sine Fold multiply the signal by the drive gain and fold the result back instead of cutting it (source: distortion.cpp).

Bit Crush rounds the signal to coarse steps. The step size grows with the square of the drive position across the range, up to a full-scale step at 30 dB (source: distortion.cpp, distortion.h).

Down Sample holds each sample for a time that grows as drive rises. At -30 dB the hold time is 1/88200 s (source: distortion.cpp, distortion.h).

The effect has its own filter. It is a basic 12 dB filter of the Digital kind. Its order setting runs it before the distortion, after it, or not at all. The cutoff follows the same note law as the voice filters, and this filter has no key tracking (source: distortion_module.cpp, digital_svf.cpp, filter_module.cpp).

The mix control blends the original sound with the result of the filter and the distortion together (source: distortion_module.cpp).

Drive of the voice filters is a different control: a 0 to 20 dB gain inside each filter, set in the filter layer. See [Filter](filter.md). The oscillator shapers also differ. See [Oscillator source](oscillator.md).

## Control entries

### Distortion switch
- Keys: `distortion_on`
- Effect: Turns the distortion effect on. The effect chain makes this switch (source: reorderable_effect_chain.cpp).
- Words: none in the word lists

### Distortion type
- Keys: `distortion_type`
- Effect: Picks the shape that bends the waveform. The six types are listed in the section above. Drive means something different in Bit Crush and Down Sample (source: distortion.cpp, distortion.h).
- Words: [distortion (drive)](../core/words-lists.md#technique-words), [bitcrush (downsample)](../core/words-lists.md#technique-words)

### Distortion drive
- Keys: `distortion_drive`
- Effect: Sets how hard the effect bends the sound. Drive is in decibels, from -30 dB to 30 dB (measured on 1.0.7). Soft Clip, Hard Clip and the folds treat it as gain. Bit Crush and Down Sample read it as a position along the range (source: distortion.h, distortion.cpp).
- Words: [distortion (drive)](../core/words-lists.md#technique-words)

### Distortion mix
- Keys: `distortion_mix`
- Effect: Crossfades between the sound entering the effect and the processed sound. The value is held between 0 and 1 (source: distortion_module.cpp).
- Words: none in the word lists

### Distortion filter order
- Keys: `distortion_filter_order`
- Effect: None skips the filter. Pre filters the sound before it reaches the distortion. Post filters the distorted sound (source: distortion_module.cpp, synth_strings.h).
- Words: none in the word lists

### Distortion filter cutoff
- Keys: `distortion_filter_cutoff`
- Effect: Sets the edge frequency of the effect filter. Hz = 8.1757989156 x 2^(stored/12), and the default 80 is 830.61 Hz. The value box shows the stored note number itself, in semitones, with no offset (source: common.h, utils.h, synth_parameters.cpp).
- Words: [filter (cutoff)](../core/words-lists.md#technique-words)

### Distortion filter resonance
- Keys: `distortion_filter_resonance`
- Effect: Boosts the sound near the cutoff. The knob maps to the internal value by its cube, as in the Digital voice filter (source: digital_svf.cpp).
- Words: [resonance](../core/words-lists.md#technique-words)

### Distortion filter blend
- Keys: `distortion_filter_blend`
- Effect: Morphs the filter response. At 0 it is low pass, at 1 band pass and at 2 high pass (source: digital_svf.cpp, distortion_module.cpp).
- Words: none in the word lists
