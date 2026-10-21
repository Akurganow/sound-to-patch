# Sound to Serum

Turn an audio stem into a Serum 2 starting point: a preset and the notes.

The project is in the planning stage.

## Credits

This project builds on the research and open-source work below.

### Research

- Kevin Thor Hansen,
  [Sound2Serum: Neural Sound Matching System for Serum 2](https://projekter.aau.dk/projekter/files/835339029/Master_Thesis_Kevin_Thor_Hansen.pdf),
  master thesis, Aalborg University Copenhagen, 2026, supervisor Cumhur
  Erkut.
- Rachel M. Bittner et al.,
  [A Lightweight Instrument-Agnostic Model for Polyphonic Note Transcription and Multipitch Estimation](https://arxiv.org/abs/2203.09893),
  ICASSP 2022. The Basic Pitch model.
- Haokun Tian, Stefan Lattner and Charalampos Saitis,
  [Assessing the Alignment of Audio Representations with Timbre Similarity Ratings](https://arxiv.org/abs/2507.07764),
  ISMIR 2025.
- Seungheon Doh et al.,
  [Can Large Language Models Predict Audio Effects Parameters from Natural Language?](https://arxiv.org/abs/2505.20770),
  WASPAA 2025.

### Open-source projects

- [node-serum2-preset-packager](https://github.com/CharlesBT/node-serum2-preset-packager)
  (MIT): `.SerumPreset` packing and unpacking.
- [serum2gen](https://github.com/dougwithseismic/serum2gen) (MIT): Serum 2
  preset reading, writing and parameter aliases.
- [serum2-preset-loader](https://github.com/wiillownet/serum2-preset-loader)
  (MIT): Serum 2 presets as plugin state.
- [serum-mcp](https://github.com/Celian-mrc/serum-mcp) (MIT): Serum 2 preset
  generation driven by a language model.
