# Render record

`render` writes a record next to each WAV file: everything the audio
depends on, so a later comparison can cite it and analyze the audio with
the same notes. The committed JSON Schema,
[`schemas/render-record.schema.json`](https://github.com/Akurganow/sound-to-patch/blob/main/schemas/render-record.schema.json),
holds the same contract, and the test suite validates records against it.

Times are in seconds and the tempo in beats per minute.

| Field | Value |
| :-- | :-- |
| `identity.synth` | `"vital"` or `"serum2"`. |
| `identity.build` | The plug-in's build, such as `"1.0.7"`. |
| `identity.host` | The host program and the `vst3-host` version it is built on. |
| `identity.arch` | The architecture the host ran as, such as `"x86_64"`. |
| `recipe_version` | The version of the render recipe. Any change to the recipe raises it. |
| `sample_rate` | 44100 in recipe version 1. |
| `block_size` | The samples per processing block, 512 in recipe version 1. |
| `process_mode` | `"offline"`: the host tells the plug-in that it renders a file. |
| `settle` | The silence rendered after the state loads and before the warm-up note. |
| `oversampling` | The oversampling of a Vital render, `"2x"` for every render of recipe version 1. |
| `bpm` | The tempo the MIDI file was read at; synced LFOs and delays follow it. |
| `last_note_off` | The end of the last note. |
| `length` | The clip length. |
| `tail_capped` | `true` when the tail still sounded 30 s after the last note-off and the clip was cut there. |
| `midi_sha256` | The SHA-256 of the MIDI file, 64 lowercase hex digits. |
| `load_check` | The [load check](../../guide/render.md#the-load-check): `init_distance_db`, `init_spread_db` and `margin_db`. |

`midi_sha256` and `load_check` are absent from a record whose notes came
from no file or whose render skipped the load check, such as the renders
of the test suite.

## Before the clip

The record names the recipe, and the recipe fixes what plays before the
clip, in one processing run with the clip:

1. 0.5 s of silence, so smoothed parameters settle.
2. A warm-up note, MIDI 60 at velocity 100 for 0.5 s in a 1 s step, which
   clears what an earlier render left behind.
3. 0.5 s of silence again.

The host renders each step in whole blocks, so a step lasts up to one block
longer. Its audio is dropped, and the clip starts on a block boundary at
the transport's start.
