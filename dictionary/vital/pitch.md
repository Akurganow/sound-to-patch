Facts checked on the open source of build 1.0.6 (commit 636ca0e) unless an entry says measured. Oscillator tuning and unison detune sit in other layers.

## How pitch and glide work

Each voice builds one pitch in semitones from five parts: the glided note, the pitch wheel times the bend range, a per-note bend, the voice tune and the voice transpose (source: synth_voice_handler.cpp).

The glided note slides from the previous note to the new note. The slide time comes from Portamento Time, and Portamento Slope bends its speed curve (source: portamento_slope.cpp).

A glide shorter than 0.001 seconds is skipped and the pitch jumps (source: portamento_slope.cpp, portamento_slope.h). The default Portamento Time is below that limit, so glide is off by default.

By default a note glides only when another key is still held. Portamento Force removes that condition (source: portamento_slope.cpp).

Polyphony sets how many notes sound together, 1 to 32 (source: synth_constants.h, voice_handler.cpp). Priority, override and legato act when held notes reach that count.

## Control entries

### Polyphony
- Keys: `polyphony`
- Effect: Sets the number of voices, 1 to 32. Lowering it while notes sound kills the excess voices, released ones first (source: voice_handler.cpp, synth_constants.h).
- Words: none in the word lists

### Legato
- Keys: `legato`
- Effect: A new note that takes over a voice still held does not restart that voice's envelopes and LFOs (source: voice_handler.cpp). It matters when held notes reach the polyphony. At polyphony 1, overlapped notes form one continuous voice.
- Words: none in the word lists

### Portamento Time
- Keys: `portamento_time`
- Effect: Sets how long the pitch takes to slide from one note to the next. Seconds are 2 to the power of the stored value (measured on 1.0.7). Below 0.001 s there is no glide (source: portamento_slope.h).
- Words: [glide (portamento)](../core/words-lists.md#technique-words)

### Portamento Slope
- Keys: `portamento_slope`
- Effect: Bends the speed of the slide. At 0 the pitch moves at constant speed. Other values make it start fast or slow and finish the opposite way (source: portamento_slope.cpp, futils.h).
- Words: [glide (portamento)](../core/words-lists.md#technique-words)

### Portamento Force
- Keys: `portamento_force`
- Effect: Off, a note glides only while another key is held. On, every note glides from the last played note (source: portamento_slope.cpp).
- Words: [glide (portamento)](../core/words-lists.md#technique-words)

### Portamento Scale
- Keys: `portamento_scale`
- Effect: Scales the glide time by the distance between the two notes. The set time then applies to a jump of one octave, and a fifth takes 7 over 12 of it (source: portamento_slope.cpp).
- Words: [glide (portamento)](../core/words-lists.md#technique-words)

### Voice transpose
- Keys: `voice_transpose`
- Effect: Shifts the pitch of the whole voice in semitones, from -48 to 48 (source: synth_voice_handler.cpp, synth_parameters.cpp).
- Words: none in the word lists

### Voice tune
- Keys: `voice_tune`
- Effect: Shifts the pitch of the whole voice in cents. The stored value is in semitones and the screen shows 100 times that, so the span is plus and minus 100 cents (source: synth_parameters.cpp, synth_voice_handler.cpp).
- Words: none in the word lists

### Pitch bend range
- Keys: `pitch_bend_range`
- Effect: Sets how many semitones the pitch wheel moves the pitch at its full position. The pitch change is wheel position times this range (source: synth_voice_handler.cpp).
- Words: none in the word lists

### Pitch wheel
- Keys: `pitch_wheel`
- Effect: Holds the wheel position from -1 to 1. Its pitch change in semitones is the position times the bend range (source: synth_voice_handler.cpp).
- Words: none in the word lists

### MPE
- Keys: `mpe_enabled`
- Effect: On, a bend message on a non-master channel bends only the held notes of that channel, by a fixed span of 48 semitones that ignores the bend range (source: midi_manager.cpp, voice_handler.h, voice_handler.cpp).
- Words: none in the word lists

### Voice priority
- Keys: `voice_priority`
- Effect: Picks which voice a surplus note takes and which waiting note returns when a key lifts. Highest and Lowest keep those notes. Oldest keeps the first played. Newest and Round Robin take the oldest voice, and Round Robin cycles the waiting notes (source: voice_handler.cpp).
- Words: none in the word lists

### Voice override
- Keys: `voice_override`
- Effect: When a key lifts and notes wait, Kill fades the lifted voice out over 0.05 s and starts the waiting note on another voice. Steal reuses the voice (source: voice_handler.cpp, common.h, envelope.cpp).
- Words: none in the word lists
