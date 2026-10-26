# Sound to Patch

Turn an audio stem into a synth starting point, a preset and the notes, or
turn a written description of a sound into a preset. Vital and Serum 2 are
the target synths, and Vital comes first. The project is MIT-licensed, see
[LICENSE](LICENSE).

The project is at an early release. `sound-to-patch preset inspect` prints
the header and the decoded settings of a Vital preset. The commands of both
paths arrive in later releases.

Vital and Serum 2 are trademarks of their owners. This project is not
affiliated with or endorsed by them.

## Build

Building needs Rust 1.91 or newer.

```sh
git clone https://github.com/Akurganow/sound-to-patch.git
cd sound-to-patch
cargo build
target/debug/sound-to-patch --version
```

The last command prints `sound-to-patch` and the version. The tools, the
commit hook and the checks for contributing are in
[CONTRIBUTING.md](CONTRIBUTING.md).

## Credits

<!-- ANCHOR: credits -->

This project builds on the research and open-source work below. Each entry
gives the work's license. "Inspiration only" marks a work we read for facts
and ideas, from which we copy no code, text or data.

### Research

- Kevin Thor Hansen, [Sound2Serum: Neural Sound Matching System for Serum 2](https://projekter.aau.dk/projekter/files/835339029/Master_Thesis_Kevin_Thor_Hansen.pdf) (no license found), master thesis, Aalborg University Copenhagen, 2026, supervisor Cumhur Erkut: fixed-template presets, bank-toggle reload and per-category parameter priors. Inspiration only.
- Joakim Andén and Stéphane Mallat, [Deep Scattering Spectrum](https://doi.org/10.1109/TSP.2014.2326991) (in copyright), IEEE TSP 2014: wavelet scattering coefficients as timbre descriptors. Inspiration only.
- Joakim Andén et al., [Joint Time-Frequency Scattering](https://doi.org/10.1109/TSP.2019.2918992) (in copyright), IEEE TSP 2019: joint time-frequency scattering for modulation structure. Inspiration only.
- Les Atlas and Shihab A. Shamma, [Joint Acoustic and Modulation Frequency](https://doi.org/10.1155/S1110865703305013) (no license found), EURASIP JASP 2003: modulation-spectrum view for LFO rate analysis. Inspiration only.
- François Auger and Patrick Flandrin, [Improving the readability of time-frequency and time-scale representations by the reassignment method](https://doi.org/10.1109/78.382394) (in copyright), IEEE TSP 1995: reassigned spectrogram for sharper partial peaks. Inspiration only.
- Roland Badeau et al., [Performance of ESPRIT for Estimating Mixtures of Complex Exponentials Modulated by Polynomials](https://doi.org/10.1109/TSP.2007.906744) (in copyright), IEEE TSP 2008: ESPRIT accuracy for decaying partials. Inspiration only.
- Oren Barkan et al., [InverSynth: Deep Estimation of Synthesizer Parameter Configurations from Audio Signals](https://arxiv.org/abs/1812.06349) (arXiv non-exclusive license), IEEE/ACM TASLP 2019: ordinal-bin classification and per-parameter identifiability findings. Inspiration only.
- Emmanouil Benetos and Simon Dixon, [Polyphonic music transcription using note onset and offset detection](https://doi.org/10.1109/icassp.2011.5946322) (in copyright), ICASSP 2011: note offsets are harder to find than onsets. Inspiration only.
- Nancy Bertin et al., [Enforcing Harmonicity and Smoothness in Bayesian Non-Negative Matrix Factorization Applied to Polyphonic Music Transcription](https://doi.org/10.1109/TASL.2010.2041381) (in copyright), IEEE TASLP 2010: harmonic NMF for partial separation. Inspiration only.
- Rachel M. Bittner et al., [A Lightweight Instrument-Agnostic Model for Polyphonic Note Transcription and Multipitch Estimation](https://arxiv.org/abs/2203.09893) (CC BY-NC-SA 4.0), ICASSP 2022: the Basic Pitch model. Inspiration only.
- Boualem Boashash, [Estimating and interpreting the instantaneous frequency of a signal. I. Fundamentals](https://doi.org/10.1109/5.135376) (in copyright), Proceedings of the IEEE 1992: instantaneous frequency from the analytic signal. Inspiration only.
- Philipp Bogdan, [INSTRUMENTAL: Automatic Synthesizer Parameter Recovery from Audio via Evolutionary Optimization](https://arxiv.org/abs/2603.15905) (CC BY 4.0), arXiv 2026: bounded CMA-ES settings, composite spectral loss, parameter-count lesson.
- Stephen Brade et al., [SynthScribe: Deep Multimodal Tools for Synthesizer Sound Retrieval and Exploration](https://arxiv.org/abs/2312.04690) (CC BY 4.0), arXiv 2023: CLAP retrieval over rendered presets and the middle-C 4 s render recipe.
- Judith C. Brown, [Calculation of a constant Q spectral transform](https://doi.org/10.1121/1.400476) (in copyright), JASA 1991: constant-Q transform for bass-range resolution. Inspiration only.
- Fred Bruford et al., [Synthesizer Sound Matching Using Audio Spectrogram Transformers](https://arxiv.org/abs/2407.16643) (CC BY 4.0), DAFx 2024: factory-distribution sampling and audio-based scoring.
- Paul Callens and Milos Cernak, [Joint Blind Room Acoustic Characterization From Speech And Music Signals Using Convolutional Recurrent Neural Networks](https://arxiv.org/abs/2010.11167) (arXiv non-exclusive license), arXiv 2020: neural estimates of reverb time and clarity. Inspiration only.
- Alistair Carson et al., [Resampling Filter Design for Multirate Neural Audio Effect Processing](https://arxiv.org/abs/2501.18470) (CC BY 4.0), arXiv 2025: aliasing-to-signal ratio for judging distortion aliasing.
- H. T. Carvalho et al., [Bayesian suppression of memoryless nonlinear audio distortion](https://new.eurasip.org/Proceedings/Eusipco/Eusipco2015/papers/1570103841.pdf) (in copyright, free access), EUSIPCO 2015: blind piecewise-linear distortion-curve estimate. Inspiration only.
- Sungkyun Chang et al., [YourMT3+: Multi-instrument Music Transcription with Enhanced Transformer Architectures and Cross-dataset Stem Augmentation](https://arxiv.org/abs/2407.04822) (arXiv non-exclusive license), MLSP 2024: note-level evaluation metrics and the octave-error finding. Inspiration only.
- Théo Chasle Cauchy et al., [Predicting Timbre Traits for Interpretable Assessment of Musical Sound Synthesizers](https://arxiv.org/abs/2606.30369) (CC BY 4.0), arXiv 2026: timbre-trait probes on frozen audio embeddings.
- Ojas Chaturvedi et al., [Advancing Multi-Instrument Music Transcription: Results from the 2025 AMT Challenge](https://arxiv.org/abs/2603.27528) (CC BY 4.0), NeurIPS 2025 AI for Music Workshop: the polyphony score drop.
- Sanyuan Chen et al., [BEATs: Audio Pre-Training with Acoustic Tokenizers](https://arxiv.org/abs/2212.09058) (arXiv non-exclusive license), arXiv 2022: audio tagger architecture read for the model survey. Inspiration only.
- Zui Chen et al., [Sound2Synth: Interpreting Sound via FM Synthesizer Parameters Estimation](https://arxiv.org/abs/2205.03043) (CC BY-NC-SA 4.0), IJCAI 2022: ordinal classification with label smoothing, and the feature set. Inspiration only.
- Alain de Cheveigné and Hideki Kawahara, [YIN, a fundamental frequency estimator for speech and music](https://doi.org/10.1121/1.1458024) (in copyright), JASA 2002: the YIN period estimator. Inspiration only.
- Joseph T. Colonel et al., [Reverse engineering memoryless distortion effects with differentiable waveshapers](https://aes2.org/publications/elibrary-page/?id=21955) (no license found), AES Convention 153, 2022: Wiener-Hammerstein distortion model with a learned waveshaper. Inspiration only.
- Paolo Combes et al., [Neural Proxies for Sound Synthesizers: Learning Perceptually Informed Preset Representations](https://arxiv.org/abs/2509.07635) (CC BY 4.0), JAES 2025: audio-metric evaluation set for comparing sounds.
- Marco Comunità et al., [Guitar Effects Recognition and Parameter Estimation with Convolutional Neural Networks](https://arxiv.org/abs/2012.03216) (CC BY-NC-SA 4.0), JAES 2021: pedal classification and setting estimation for distortion. Inspiration only.
- Helena Cuesta et al., [A Framework for Multi-f0 Modeling in SATB Choir Recordings](https://arxiv.org/abs/1904.05086) (CC BY 4.0), SMC 2019: spectral peak width as a proxy for f0 spread.
- Orchisama Das, [An Open Source Stereo Widening Plugin](https://dafx.de/paper-archive/2024/papers/DAFx24_paper_92.pdf) (CC BY 4.0), DAFx 2024: catalogue of width methods such as mid/side, velvet-noise and allpass decorrelators.
- Torsten Dau et al., [Modeling auditory processing of amplitude modulation. I. Detection and masking with narrow-band carriers](https://doi.org/10.1121/1.420344) (in copyright), JASA 1997: modulation filterbank layout for LFO bands. Inspiration only.
- Ingrid Daubechies et al., [Synchrosqueezed wavelet transforms: An empirical mode decomposition-like tool](https://doi.org/10.1016/j.acha.2010.08.002) (in copyright, free access), ACHA 2011: synchrosqueezing for sharp time-frequency ridges. Inspiration only.
- Qixin Deng et al., [Do Joint Language-Audio Embeddings Encode Perceptual Timbre Semantics?](https://arxiv.org/abs/2510.14249) (CC BY 4.0), arXiv 2025: CLAP aligns weakly with timbre ratings.
- Satvik Dixit et al., [Vision Language Models Are Few-Shot Audio Spectrogram Classifiers](https://arxiv.org/abs/2411.12058) (CC BY 4.0), arXiv 2024: spectrogram reading by language models.
- Seungheon Doh et al., [Can Large Language Models Predict Audio Effects Parameters from Natural Language?](https://arxiv.org/abs/2505.20770) (CC BY-NC-SA 4.0), WASPAA 2025: prompt layout, in-context ablation, and bound tests for feature choice. Inspiration only.
- Seungheon Doh et al., [LLM2Fx-Tools: Tool Calling For Music Post-Production](https://arxiv.org/abs/2512.01559) (CC BY-NC-SA 4.0), ICLR 2026: tool-call schema idea for preset edits. Inspiration only.
- Jonathan Driedger et al., [Template-Based Vibrato Analysis of Music Signals](https://archives.ismir.net/ismir2016/paper/000005.pdf) (CC BY 4.0), ISMIR 2016: vibrato detection on the spectrogram, without an f0 stage.
- Jonathan Driedger et al., [Extending Harmonic-Percussive Separation of Audio Signals](https://doi.org/10.5281/zenodo.1415226) (CC BY 4.0), ISMIR 2014: harmonic-percussive separation with a residual.
- James Eaton et al., [Estimation of Room Acoustic Parameters: The ACE Challenge](https://doi.org/10.1109/TASLP.2016.2577502) (in copyright), IEEE/ACM TASLP 2016: benchmark for blind reverb-time estimates. Inspiration only.
- Felix Eichas and Udo Zölzer, [Black-box Modeling of Distortion Circuits with Block-Oriented Models](https://dafx.de/paper-archive/details/aW-6gR1PsD9EqSfEI9CYcg) (in copyright, free access), DAFx 2016: block-oriented models of distortion circuits. Inspiration only.
- Amro El-Jaroudi and John Makhoul, [Discrete all-pole modeling](https://doi.org/10.1109/78.80824) (in copyright), IEEE TSP 1991: all-pole envelope fitted at harmonic frequencies. Inspiration only.
- Sean Enderby and Ryan Stables, [A nonlinear method for manipulating warmth and brightness](https://www.dafx.de/paper-archive/2017/papers/DAFx17_paper_17.pdf) (in copyright, free access), DAFx 2017: centroid-like features track brightness. Inspiration only.
- Fabián Esqueda et al., [Virtual Analog Models of the Lockhart and Serge Wavefolders](https://www.mdpi.com/2076-3417/7/12/1328) (CC BY 4.0), Applied Sciences 2017: wavefolder transfer curves for the distortion layer.
- Angelo Farina, [Simultaneous measurement of impulse response and distortion with a swept-sine technique](https://aes2.org/publications/elibrary-page/?id=10211) (in copyright), AES Convention 108, 2000: swept-sine separation of harmonic distortion orders. Inspiration only.
- Cédric Févotte and Jérôme Idier, [Algorithms for Nonnegative Matrix Factorization with the β-Divergence](https://doi.org/10.1162/NECO_a_00168) (in copyright), Neural Computation 2011: NMF update rules for spectrogram factorization. Inspiration only.
- Cédric Févotte et al., [Nonnegative Matrix Factorization with the Itakura-Saito Divergence: With Application to Music Analysis](https://doi.org/10.1162/neco.2008.04-08-771) (in copyright, free access), Neural Computation 2009: Itakura-Saito NMF for note and component separation. Inspiration only.
- Kelly R. Fitz and Sean A. Fulop, [A Unified Theory of Time-Frequency Reassignment](https://arxiv.org/abs/0903.3080) (arXiv non-exclusive license), arXiv 2009: reassignment theory behind sharper spectrogram peaks. Inspiration only.
- Derry FitzGerald, [Harmonic/Percussive Separation using Median Filtering](https://dafx.de/paper-archive/details/DsmIVcydPX66AuaqEmKyTQ) (in copyright, free access), DAFx 2010: median-filter harmonic-percussive separation. Inspiration only.
- Hannes Gamper and Ivan J. Tashev, [Blind Reverberation Time Estimation Using a Convolutional Neural Network](https://doi.org/10.1109/iwaenc.2018.8521241) (in copyright), IWAENC 2018: neural blind reverb-time estimate, trained on speech. Inspiration only.
- Philipp Götz et al., [Blind Acoustic Parameter Estimation Through Task-Agnostic Embeddings Using Latent Approximations](https://arxiv.org/abs/2407.19989) (CC BY-NC-SA 4.0), IWAENC 2024: neural blind estimates of room acoustic parameters. Inspiration only.
- Masato Hagiwara et al., [Modeling Animal Vocalizations through Synthesizers](https://arxiv.org/abs/2210.10857) (CC BY 4.0), arXiv 2022: optimizer comparison on a parametric synthesizer.
- Ben Hayes et al., [Audio synthesizer inversion in symmetric parameter spaces with approximately equivariant flow matching](https://arxiv.org/abs/2506.07199) (arXiv non-exclusive license), ISMIR 2025: audio metrics, not parameter distance, score a sound match. Inspiration only.
- Ben Hayes, [Off-manifold robustness in synthesizer inversion with joint distribution flow matching](https://arxiv.org/abs/2609.29320) (arXiv non-exclusive license), arXiv 2026: real audio sits off the synth manifold, so exact matches are unlikely. Inspiration only.
- Perfecto Herrera and Jordi Bonada, [Vibrato Extraction and Parameterization in the Spectral Modeling Synthesis Framework](https://www.dafx.de/paper-archive/1998/HER60.PS.pdf) (in copyright, free access), DAFx 1998: vibrato and tremolo rate and depth from the f0 track. Inspiration only.
- Reemt Hinrichs et al., [Convolutional neural networks for the classification of guitar effects and extraction of the parameter settings of single and multi-guitar effects from instrument mixes](https://doi.org/10.1186/s13636-022-00257-4) (CC BY 4.0), EURASIP JASM 2022: effect class and setting estimates from mixes.
- Nicki Holighaus et al., [A Framework for Invertible, Real-Time Constant-Q Transforms](https://doi.org/10.1109/TASL.2012.2234114) (in copyright), IEEE TASLP 2013: invertible constant-Q transform. Inspiration only.
- Andrew Horner et al., [Methods for Multiple Wavetable Synthesis of Musical Instrument Tones](https://aes2.org/publications/elibrary-page/?id=7003) (in copyright), JAES 1993: basis spectra for multiple-wavetable matching. Inspiration only.
- Yingbo Hua and Tapan K. Sarkar, [Matrix pencil method for estimating parameters of exponentially damped/undamped sinusoids in noise](https://doi.org/10.1109/29.56027) (in copyright), IEEE TASSP 1990: matrix pencil method for partial frequency and damping. Inspiration only.
- Johannes Imort et al., [Distortion Audio Effects: Learning How to Recover the Clean Signal](https://arxiv.org/abs/2202.01664) (CC BY 4.0), ISMIR 2022: restoration baselines for overdrive and hard clipping.
- Anssi Klapuri et al., [Robust multipitch estimation for the analysis and manipulation of polyphonic musical signals](https://dafx.de/paper-archive/details/j0RptU__VKpCPM9TPRoWww) (in copyright, free access), DAFx 2000: iterative estimate-and-cancel multipitch for polyphony counts. Inspiration only.
- Ramdas Kumaresan and Donald W. Tufts, [Estimating the parameters of exponentially damped sinusoids and pole-zero modeling in noise](https://doi.org/10.1109/TASSP.1982.1163974) (in copyright), IEEE TASSP 1982: damped-sinusoid parameter estimation. Inspiration only.
- Jean Laroche, [The use of the matrix pencil method for the spectrum analysis of musical signals](https://doi.org/10.1121/1.407519) (in copyright), JASA 1993: matrix pencil analysis applied to music. Inspiration only.
- Sungho Lee et al., [Blind Estimation of Audio Processing Graph](https://arxiv.org/abs/2303.08610) (CC BY-SA 4.0), ICASSP 2023: effect graph estimate from a wet reference. Inspiration only.
- Hazounne Lee et al., [Wavespace: A Highly Explorable Wavetable Generator](https://arxiv.org/abs/2407.19862) (CC BY 4.0), arXiv 2024: wavetable descriptors such as brightness, richness, fullness, undulation and symmetry.
- Ying-Shuo Lee et al., [Distortion Recovery: A Two-Stage Method for Guitar Effect Removal](https://arxiv.org/abs/2407.16639) (CC BY 4.0), DAFx 2024: two-stage restoration of overdrive and amp effects.
- Yizhi Li et al., [MERT: Acoustic Music Understanding Model with Large-Scale Self-supervised Training](https://arxiv.org/abs/2306.00107) (CC BY-SA 4.0), ICLR 2024: music embedding architecture read for the model survey. Inspiration only.
- John Makhoul, [Linear prediction: A tutorial review](https://doi.org/10.1109/PROC.1975.9792) (in copyright), Proceedings of the IEEE 1975: linear prediction for all-pole filter envelopes. Inspiration only.
- Héctor Martel et al., [Probing Low-Level Acoustic Attribute Encoding in CLAP Audio Embeddings](https://arxiv.org/abs/2607.03806) (CC BY 4.0), DAFx 2026: centroid is not linearly decodable from CLAP.
- Naotake Masuda and Daisuke Saito, [Quality Diversity for Synthesizer Sound Matching](https://dafx2020.mdw.ac.at/proceedings/papers/DAFx20in21_paper_46.pdf) (CC BY 3.0), DAFx 2021: log-spectral fitness and novelty-based diversity.
- Kazuki Matsumoto and Kohei Yatabe, [Estimation of the number of unison and degree of detune of SuperSaw in subtractive synthesizers](https://web.tuat.ac.jp/~yatabe/papers/matsumoto_2022_9.pdf) (in copyright, free access), ASJ Autumn Meeting 2022: cost function for unison count and detune. Inspiration only.
- Robert J. McAulay and Thomas F. Quatieri, [Speech analysis/Synthesis based on a sinusoidal representation](https://doi.org/10.1109/TASSP.1986.1164910) (in copyright), IEEE TASSP 1986: sinusoidal model and partial tracking. Inspiration only.
- Christopher Mitcheltree and Hideki Koike, [SerumRNN: Step by Step Audio VST Effect Programming](https://arxiv.org/abs/2104.03876) (CC BY-NC-SA 4.0), EvoMUSART 2021: iterative one-block-at-a-time matching with a stop rule. Inspiration only.
- Christopher Mitcheltree et al., [Modulation Extraction for LFO-driven Audio Effects](https://arxiv.org/abs/2305.13262) (arXiv non-exclusive license), DAFx 2023: the only learned extractor of the LFO behind phaser, flanger and chorus effects. Inspiration only.
- Christopher Mitcheltree et al., [Modulation Discovery with Differentiable Digital Signal Processing](https://arxiv.org/abs/2510.06204) (arXiv non-exclusive license), WASPAA 2025: recovering modulation curves of oscillator, filter and envelope with differentiable synthesis. Inspiration only.
- Meinard Müller, [Fundamentals of Music Processing, notebook C1S3 Timbre](https://audiolabs-erlangen.de/resources/MIR/FMP/C1/C1S3_Timbre.html) (CC BY-NC-SA 4.0), Springer 2015: limits of the ADSR envelope model. Inspiration only.
- John Muradeli et al., [Differentiable Time-Frequency Scattering on GPU](https://arxiv.org/abs/2204.08269) (CC BY 4.0), DAFx 2022: differentiable joint time-frequency scattering.
- Anastasia Natsiou et al., [Interpretable Timbre Synthesis using Variational Autoencoders Regularized on Timbre Descriptors](https://arxiv.org/abs/2307.10283) (arXiv non-exclusive license), arXiv 2023: the 10-90 percent attack threshold, as weak evidence. Inspiration only.
- Shahan Nercessian, [Neural Parametric Equalizer Matching Using Differentiable Biquads](https://dafx2020.mdw.ac.at/Proceedings/papers/DAFx2020_paper_7.pdf) (CC BY 3.0), DAFx 2020: frequency-domain loss beats parameter loss for equalizer matching.
- Julian Neri and Philippe Depalle, [Fast Partial Tracking of Audio with Real-Time Capability through Linear Programming](https://dafx.de/paper-archive/details/7Nw5ACuZtSx_zWWdEXcagQ) (in copyright, free access), DAFx 2018: partial tracking by linear programming. Inspiration only.
- Antonín Novák et al., [Nonlinear System Identification Using Exponential Swept-Sine Signal](https://doi.org/10.1109/TIM.2009.2031836) (in copyright), IEEE TIM 2010: exponential swept-sine harmonic response analysis. Inspiration only.
- Youichi Okita and Haruhiro Katayose, [Audio Effect Estimation with DNN-Based Prediction and Search Algorithm](https://arxiv.org/abs/2604.22276) (arXiv non-exclusive license), ICASSP 2026: network predicts effect types, then search sets order and parameters. Inspiration only.
- Julian D. Parker et al., [Reducing the Aliasing of Nonlinear Waveshaping Using Continuous-Time Convolution](https://dafx.de/paper-archive/details/vem_XXF5qBbfiWOH2RVVAA) (in copyright, free access), DAFx 2016: antiderivative anti-aliasing for waveshapers. Inspiration only.
- Geoffroy Peeters et al., [The Timbre Toolbox: Extracting audio descriptors from musical signals](https://doi.org/10.1121/1.3642604) (in copyright), JASA 2011: descriptor definitions and the weakest-effort attack method. Inspiration only.
- Côme Peladeau and Geoffroy Peeters, [Blind estimation of audio effects using an auto-encoder approach and differentiable digital signal processing](https://arxiv.org/abs/2310.11781) (CC BY 4.0), ICASSP 2024: audio loss gives better sound, parameter loss better parameters.
- Antonio Pertusa, [Computationally efficient methods for polyphonic music transcription](https://grfia.dlsi.ua.es/repositori/grfia/pubs/244/pertusaphd.pdf) (in copyright, free access), PhD thesis, University of Alicante, 2010: loudness and spectral smoothness criterion for polyphony estimates. Inspiration only.
- Rama Ratnam et al., [Blind estimation of reverberation time](https://doi.org/10.1121/1.1616578) (in copyright), JASA 2003: maximum-likelihood decay-rate formulas for blind reverb time. Inspiration only.
- Xavier Riley and Simon Dixon, [FiloBass: A Dataset and Corpus Based Study of Jazz Basslines](https://arxiv.org/abs/2311.02023) (CC BY 4.0), ISMIR 2023: bass transcription baseline numbers.
- Xavier Riley and Simon Dixon, [CREPE Notes: A new method for segmenting pitch contours into discrete notes](https://arxiv.org/abs/2311.08884) (CC BY 4.0), SMC 2023: combining inverted pitch confidence with the pitch gradient to find note boundaries.
- Alain Riou et al., [PESTO: Pitch Estimation with Self-supervised Transposition-equivariant Objective](https://arxiv.org/abs/2309.02265) (CC BY 4.0), ISMIR 2023: compared as an f0 model, not used.
- Axel Röbel and Xavier Rodet, [Efficient spectral envelope estimation and its application to pitch shifting and envelope preservation](https://www.dafx.de/paper-archive/2005/P_030.pdf) (in copyright, free access), DAFx 2005: True Envelope through harmonic peaks. Inspiration only.
- Jonas Roth et al., [Alias-Free Oscillator Synchronization via Additive Synthesis](https://www.dafx.de/paper-archive/2026/papers/DAFx26_paper_49.pdf) (CC BY 4.0), DAFx 2026: sync waveform model behind the sync-ratio estimate.
- Richard Roy and Thomas Kailath, [ESPRIT--Estimation of Signal Parameters via Rotational Invariance Techniques](https://doi.org/10.1117/12.55606) (in copyright), Optical Engineering 1990: ESPRIT subspace method for line spectra. Inspiration only.
- Charalampos Saitis and Stefan Weinzierl, [The Semantics of Timbre](https://doi.org/10.14279/depositonce-15241) (in copyright, free access), Timbre: Acoustics, Perception, and Cognition, 2019: semantic dimensions of timbre and their acoustic correlates for the sound-word dictionary. Inspiration only.
- Hiroaki Sakoe and Seibi Chiba, [Dynamic programming algorithm optimization for spoken word recognition](https://doi.org/10.1109/TASSP.1978.1163055) (in copyright), IEEE TASSP 1978: dynamic time warping for aligning renders. Inspiration only.
- S Sakshi et al., [MMAU: A Massive Multi-Task Audio Understanding and Reasoning Benchmark](https://arxiv.org/abs/2410.19168) (CC BY 4.0), arXiv 2024: perception, not reasoning, limits audio language models.
- Amir Salimi et al., [Evaluating Sound Similarity Metrics for Differentiable, Iterative Sound-Matching](https://arxiv.org/abs/2506.22628) (CC BY 4.0), arXiv 2025: the case for several named distances.
- Ralph O. Schmidt, [Multiple emitter location and signal parameter estimation](https://doi.org/10.1109/TAP.1986.1143830) (in copyright), IEEE TAP 1986: MUSIC subspace method for line spectra. Inspiration only.
- Manfred R. Schroeder, [New Method of Measuring Reverberation Time](https://doi.org/10.1121/1.1909343) (in copyright), JASA 1965: backward integration of decay curves. Inspiration only.
- Xavier Serra and Julius O. Smith, [Spectral Modeling Synthesis: A Sound Analysis/Synthesis System Based on a Deterministic Plus Stochastic Decomposition](https://doi.org/10.2307/3680788) (in copyright), Computer Music Journal 1990: sines-plus-noise decomposition. Inspiration only.
- Siyuan Shan et al., [Differentiable Wavetable Synthesis](https://arxiv.org/abs/2111.10003) (CC BY 4.0), ICASSP 2022: learned dictionary of wavetables for neural synthesis.
- Di Sheng and George Fazekas, [Feature Selection for Dynamic Range Compressor Parameter Estimation](https://qmro.qmul.ac.uk/xmlui/handle/123456789/40945) (in copyright), AES 2018: standard features give threshold and ratio, not attack and release. Inspiration only.
- Christian J. Steinmetz et al., [Filtered Noise Shaping for Time Domain Room Impulse Response Estimation From Reverberant Speech](https://arxiv.org/abs/2107.07503) (arXiv non-exclusive license), WASPAA 2021: room impulse response from filtered noise. Inspiration only.
- Fabian-Robert Stöter et al., [Unison Source Separation](https://www.audiolabs-erlangen.de/resources/2014-DAFx-Unison) (in copyright, free access), DAFx 2014: AM and FM differences separate unison sources. Inspiration only.
- Haoran Sun et al., [Neural-Enhanced Dynamic Range Compression Inversion: A Hybrid Approach for Restoring Audio Dynamics](https://arxiv.org/abs/2411.04337) (CC BY 4.0), arXiv 2024: compressor parameter and dry-signal recovery.
- Haoran Sun et al., [Black-Box Optimization for Identifying and Inverting Audio Dynamic Range Control Effects](https://arxiv.org/abs/2607.19645) (arXiv non-exclusive license), arXiv 2026: search over compressor settings with dynamics descriptors. Inspiration only.
- Yirong Sun et al., [SonicBench: Dissecting the Physical Perception Bottleneck in Large Audio Language Models](https://arxiv.org/abs/2601.11039) (arXiv non-exclusive license), arXiv 2026: audio language models are weak on physical attributes. Inspiration only.
- Michal Švento et al., [Estimation and Restoration of Unknown Nonlinear Distortion using Diffusion](https://arxiv.org/abs/2501.05959) (CC BY 4.0), arXiv 2025: spline waveshaper fitted blindly to distorted audio.
- Adam Szabo, [How to Emulate the Super Saw](https://www.adamszabo.com/internet/adam_szabo_how_to_emulate_the_super_saw.pdf) (in copyright, free access), KTH degree project 2010: measured supersaw detune and mix. Inspiration only.
- Ryoya Tabata et al., [FM Parameter Estimation with Low-Order Rational Constraints on Wasserstein Loss Landscape](https://www.dafx.de/paper-archive/2026/papers/DAFx26_paper_37.pdf) (CC BY 4.0), DAFx 2026: low-order rational ratio constraint for FM parameter search.
- Osamu Take et al., [Audio Effect Chain Estimation and Dry Signal Recovery From Multi-Effect-Processed Musical Signals](https://dafx.de/paper-archive/2024/papers/DAFx24_paper_53.pdf) (CC BY 4.0), DAFx 2024: undo the last effect, then repeat to recover the chain.
- Gaurav Thakur et al., [The Synchrosqueezing algorithm for time-varying spectral analysis: Robustness properties and new paleoclimate applications](https://doi.org/10.1016/j.sigpro.2012.11.029) (in copyright), Signal Processing 2013: robustness of synchrosqueezing under noise. Inspiration only.
- Haokun Tian et al., [Assessing the Alignment of Audio Representations with Timbre Similarity Ratings](https://arxiv.org/abs/2507.07764) (CC BY 4.0), ISMIR 2025: the MFCC baseline and the alignment protocol for compare.
- Bernardo Torres et al., [Unsupervised Harmonic Parameter Estimation Using Differentiable DSP and Spectral Optimal Transport](https://arxiv.org/abs/2312.14507) (arXiv non-exclusive license), ICASSP 2024: optimal-transport loss for f0 and harmonic amplitudes. Inspiration only.
- Joseph Turian et al., [One Billion Audio Sounds from GPU-enabled Modular Synthesis](https://arxiv.org/abs/2104.12922) (CC BY 4.0), arXiv 2021: rank tests for audio distances.
- Joseph Turian and Max Henry, [I'm Sorry for Your Loss: Spectrally-Based Audio Distances Are Bad at Pitch](https://arxiv.org/abs/2012.04572) (CC BY 4.0), ICBINB at NeurIPS 2020: spectral losses are weak on pitch, so align pitch first.
- Noy Uzrad et al., [DiffMoog: a Differentiable Modular Synthesizer for Sound Matching](https://arxiv.org/abs/2401.12570) (CC BY 4.0), arXiv 2024: sound matching with a differentiable modular synthesizer.
- Cyrus Vahidi et al., [Timbre Space Representation of a Subtractive Synthesizer](https://arxiv.org/abs/2009.11706) (arXiv non-exclusive license), Timbre 2020: descriptors in a subtractive-synthesizer timbre space. Inspiration only.
- Cyrus Vahidi et al., [Mesostructures: Beyond Spectrogram Loss in Differentiable Time-Frequency Analysis](https://arxiv.org/abs/2301.10183) (CC BY 4.0), arXiv 2023: scattering-based similarity of arpeggio-scale parameters.
- Benno Weck et al., [MuChoMusic: Evaluating Music Understanding in Multimodal Audio-Language Models](https://arxiv.org/abs/2408.01337) (CC BY 4.0), ISMIR 2024: audio language models lean on the text prior.
- Luwei Yang et al., [AVA: An Interactive System for Visual and Quantitative Analyses of Vibrato and Portamento Performance Styles](https://archives.ismir.net/ismir2016/paper/000314.pdf) (CC BY 4.0), ISMIR 2016: vibrato detection and a logistic portamento model for glide.
- Chin-Yun Yu et al., [Differentiable All-pole Filters for Time-varying Audio Systems](https://arxiv.org/abs/2404.07970) (arXiv non-exclusive license), DAFx 2024: time-varying all-pole filter matching phaser and acid-style filter sweeps. Inspiration only.
- Chin-Yun Yu et al., [Improving Inference-Time Optimisation for Vocal Effects Style Transfer with a Gaussian Prior](https://arxiv.org/abs/2505.11315) (arXiv non-exclusive license), WASPAA 2025: a preset-distribution prior for effect parameter search. Inspiration only.
- Tsugumasa Yutani et al., [Wavetable Synthesis Using CVAE for Timbre Control Based on Semantic Label](https://arxiv.org/abs/2410.18628) (CC BY 4.0), APSIPA ASC 2024: wavetable generation from semantic labels.
- Pavel Záviška et al., [A survey and an extensive evaluation of popular audio declipping methods](https://arxiv.org/abs/2007.07663) (CC BY-NC-SA 4.0), IEEE JSTSP 2021: declipping methods for clipped distortion. Inspiration only.
- Haina Zhu et al., [MuQ: Self-Supervised Music Representation Learning with Mel Residual Vector Quantization](https://arxiv.org/abs/2501.01108) (CC BY-NC-SA 4.0), arXiv 2025: music embedding architecture read for the model survey. Inspiration only.

### Open-source projects

- [ableton-lom-skill](https://github.com/mikecfisher/ableton-lom-skill) (no license found): an instrument API reference packed as one agent skill with a topic router, as a model for the Serum 2 reference. Inspiration only.
- [ableton-mcp](https://github.com/ahujasid/ableton-mcp) (MIT): parameter-by-index tool design, kept as a counter-example to named parameters.
- [Audio Flamingo](https://github.com/NVIDIA/audio-flamingo) (MIT code, non-commercial weights): audio language model judged weak on synth attributes. Inspiration only.
- [audio.cpp](https://github.com/0xShug0/audio.cpp) (Apache-2.0): facts on running audio models locally in C++.
- [auraloss](https://github.com/csteinmetz1/auraloss) (Apache-2.0): multi-resolution STFT loss, reimplemented in Rust.
- [AutoSynth](https://github.com/Auto-Synth/AutoSynth) (Apache-2.0): perturb-and-render recipe with logged edit masks for the test bench.
- [BE-AFX](https://github.com/peladeaucome/ICASSP-2024-BEAFX-using-DDSP) (no license): blind effect-estimation auto-encoder with differentiable effects, read for design. Inspiration only.
- [BEATs](https://github.com/microsoft/unilm/tree/master/beats) (MIT): audio tagger facts for the model survey.
- [blind_rt60](https://github.com/nuniz/blind_rt60) (MIT): blind reverb-time formulas, reimplemented in Rust.
- [burn](https://github.com/tracel-ai/burn) (MIT OR Apache-2.0): Rust inference runtime facts for the fallback plan.
- [candle](https://github.com/huggingface/candle) (MIT OR Apache-2.0): Rust inference runtime facts for the fallback plan.
- [CLAP (microsoft/CLAP)](https://github.com/microsoft/CLAP) (MIT): audio tower used for the style-embedding study.
- [claude-collider](https://github.com/jeremyruppel/claude-collider) (MIT): server paired with theory skills, and a short pitfall list.
- [cochlea](https://github.com/richer-richard/cochlea) (MIT OR Apache-2.0): spectrogram PNG for agents, and stereo probe formulas.
- [coreml-native](https://github.com/robertelee78/coreml-native) (Apache-2.0 OR MIT): platform ML binding facts for the deferred runtime option.
- [CREPE](https://github.com/marl/crepe) (MIT): 360-bin pitch table and ONNX weights.
- [CREPE Notes](https://github.com/xavriley/crepe_notes) (GPL-3.0): inverted pitch confidence plus pitch gradient to find note boundaries. Inspiration only.
- [CTAG](https://github.com/PapayaResearch/ctag) (MIT): gradient-free, embedding-guided parameter search loop.
- [dasp-pytorch](https://github.com/csteinmetz1/dasp-pytorch) (Apache-2.0): tanh drive formula.
- [DawDreamer](https://github.com/DBraun/DawDreamer) (GPL-3.0): how a JUCE host loads plug-in state, schedules MIDI and renders offline. Inspiration only.
- [DDSP](https://github.com/magenta/ddsp) (Apache-2.0): multi-scale spectral loss definition.
- [DDSynth-RL](https://github.com/DDSynth-RL/DDSynthRL) (Apache-2.0): multi-metric render-and-compare recipe, group-relative scoring, crash-isolated render workers.
- [DeepAFx-ST](https://github.com/adobe-research/DeepAFx-ST) (non-commercial research license): neural effect style transfer, surveyed for the effects layer. Inspiration only.
- [DeepFormants](https://github.com/MLSpeech/DeepFormants) (MIT): neural formant tracker, surveyed for filter resonance estimates.
- [dereverb-room](https://huggingface.co/audiojs/dereverb-room) (GPL-3.0): voice reverb-removal model, surveyed for wet-signal handling. Inspiration only.
- [dereverb_mel_band_roformer](https://huggingface.co/anvuew/dereverb_mel_band_roformer) (GPL-3.0): vocal reverb-removal checkpoints, surveyed for wet-signal handling. Inspiration only.
- [Descript Audio Codec](https://github.com/descriptinc/descript-audio-codec) (MIT): neural codec latents, surveyed as audio features.
- [diff-wave-synth](https://github.com/gudgud96/diff-wave-synth) (no license): unofficial port of differentiable wavetable synthesis. Inspiration only.
- [DiffAPF TB-303](https://github.com/DiffAPF/TB-303) (MPL-2.0): differentiable all-pole filter experiments on acid-style sweeps. Inspiration only.
- [DiffMoog](https://github.com/aisynth/diffmoog) (MIT): chain-with-activation-flags template and spectral-distance findings.
- [diffsynth](https://github.com/hyakuchiki/diffsynth) (MIT): differentiable ADSR module design.
- [DiffVox](https://github.com/SonyResearch/diffvox) (MIT): chain design, losses and parameter priors for vocal effects.
- [EnCodec](https://github.com/facebookresearch/encodec) (MIT): neural codec latents, surveyed as audio features.
- [Essentia](https://github.com/MTG/essentia) (AGPL-3.0): audio descriptor extractor design. Inspiration only.
- [Essentia models](https://essentia.upf.edu/models.html) (CC BY-NC-SA 4.0): bright/dark, instrument and dry/wet classifier heads trained on NSynth. Inspiration only.
- [FCPE](https://github.com/CNChTu/FCPE) (MIT): comparison model and local-argmax decoding.
- [ferraus-pitch](https://github.com/djynnius/ferraus) (Apache-2.0 OR MIT): pYIN note segmentation defaults.
- [FL-Automation](https://github.com/Realynx/FL-Automation) (MIT, in its `sdk/` folders): frame-by-frame labels of Serum 2 wavetables and a state schema.
- [Flow Synthesizer](https://github.com/acids-ircam/flow_synthesizer) (MIT): nested parameter subsets, audio-versus-parameter error, timbre tag vocabulary.
- [flp-extract-fxp](https://github.com/pycabbage/flp-extract-fxp) (MIT): a corpus of Serum 2 factory preset metadata, tags, value ranges and description words.
- [FM-SynAPSE](https://github.com/DBraun/SynAPSE) (MIT): shared audio and preset embedding, and the MFCC, CLAP, PANNs timbre comparison.
- [Fx-Encoder++](https://github.com/SonyResearch/Fx-Encoder_PlusPlus) (CC BY-NC 4.0): instrument-wise effect embeddings, surveyed for distortion and effect layers. Inspiration only.
- [FXencoder (music_mixing_style_transfer)](https://github.com/jhtonyKoo/music_mixing_style_transfer) (MIT code, unlicensed weights): mixing-style effect embedding, surveyed for width and effect layers. Inspiration only.
- [gfx_classifier](https://github.com/mcomunita/gfx_classifier) (BSD-3-Clause): guitar-effect classifier as a distortion bench probe, with its published models.
- [GRAFX](https://github.com/sh-lee97/grafx) (Apache-2.0): differentiable effect-graph framework, read for chain design.
- [Helm](https://github.com/mtytel/helm) (GPL-3.0): preset role taxonomy of a synthesizer. Inspiration only.
- [helm_patches](https://github.com/mtytel/helm_patches) (CC BY 4.0, in `Factory Presets/`): preset category and name-word counts.
- [IIRNet](https://github.com/csteinmetz1/IIRNet) (Apache-2.0): neural IIR filter design from a magnitude response.
- [JUCE](https://github.com/juce-framework/JUCE) (AGPL-3.0): VST3 state envelope and base64 facts. Inspiration only.
- [Kimi-Audio](https://github.com/MoonshotAI/Kimi-Audio) (Apache-2.0 and MIT): audio language model tested on timbre attributes, CUDA-only.
- [kymatio](https://github.com/kymatio/kymatio) (BSD-3-Clause): scattering transform implementation, read for joint time-frequency scattering.
- [LAION-CLAP](https://github.com/LAION-AI/CLAP) (CC0-1.0): 48 kHz, 10 s, 64-band log-mel recipe.
- [libf0](https://github.com/groupmm/libf0) (MIT): YIN, pYIN and salience f0 estimator definitions.
- [librosa](https://github.com/librosa/librosa) (ISC): MFCC, mel and spectral descriptor definitions, reimplemented in Rust.
- [llama-cpp-2](https://github.com/utilityai/llama-cpp-rs) (MIT OR Apache-2.0): Rust binding facts for local audio language models.
- [LTU](https://github.com/YuanGongND/ltu) (no license): audio language model judged weak on timbre attributes. Inspiration only.
- [MERT](https://github.com/yizhilll/MERT) (Apache-2.0 code, CC BY-NC 4.0 weights): music embedding model, surveyed for pitch and reverb probes. Inspiration only.
- [mlx-audio](https://github.com/Blaizzy/mlx-audio) (MIT): facts on running audio models with MLX.
- [mlx-rs](https://github.com/oxiglade/mlx-rs) (MIT OR Apache-2.0): Rust MLX binding facts for the fallback plan.
- [mod_discovery](https://github.com/christhetree/mod_discovery) (MPL-2.0): envelope and LFO control signals found with differentiable DSP. Inspiration only.
- [mod_extraction](https://github.com/christhetree/mod_extraction) (MPL-2.0): LFO extraction from dry and wet audio, which needs a dry signal. Inspiration only.
- [MT3](https://github.com/magenta/mt3) (Apache-2.0): segment-seam bookkeeping and overlap clean-up rules.
- [MU-LLaMA](https://github.com/shansongliu/MU-LLaMA) (GPL-3.0): music language model judged weak on synth attributes. Inspiration only.
- [MuQ](https://github.com/tencent-ailab/MuQ) (MIT code, CC BY-NC 4.0 weights): music embedding model, surveyed for instrument and reverb probes. Inspiration only.
- [MuScriptor](https://github.com/muscriptor/muscriptor) (MIT code, CC BY-NC 4.0 weights): hard instrument mask, gap filling, onset-lag calibration ideas. Inspiration only.
- [nara_wpe](https://github.com/fgnt/nara_wpe) (MIT): weighted prediction error dereverberation, read for reverb handling.
- [node-serum2-preset-packager](https://github.com/CharlesBT/node-serum2-preset-packager) (MIT): container layout, zstd level 3, and the Init preset shape.
- [objc2-core-ml](https://github.com/madsmtm/objc2) (Zlib OR Apache-2.0 OR MIT): platform ML binding facts for the deferred runtime option.
- [Odin 2](https://github.com/TheWaveWarden/odin2) (GPL-3.0 code, OFL-1.1 font, no preset terms stated): factory preset category counts. Inspiration only.
- [onnxcrepe](https://github.com/yqzhishen/onnxcrepe) (MIT): ONNX export of CREPE and its weights.
- [OpenL3](https://github.com/marl/openl3) (MIT code, CC BY 4.0 weights): audio embedding surveyed, with no ONNX export.
- [Pedalboard](https://github.com/spotify/pedalboard) (GPL-3.0): headless VST3 instrument hosting design with MIDI in, audio out and state get and set. Inspiration only.
- [PESTO](https://github.com/SonyCSLParis/pesto) (LGPL-3.0): harmonic CQT and cached-convolution streaming idea. Inspiration only.
- [pitch-estimate](https://github.com/hey-jj/pitch-estimate) (MIT): per-frame 55 Hz regression test layout.
- [Plugalyzer](https://github.com/CrushedPixel/Plugalyzer) (GPL-3.0): CLI shape for rendering a VST3 instrument from MIDI to WAV. Inspiration only.
- [producer-pal](https://github.com/adamjmurray/producer-pal) (GPL-3.0-or-later): how an agent server assembles its skill text from fragments. Inspiration only.
- [pYIN and Tony](https://github.com/c4dm/pyin) (GPL-2.0-or-later): pYIN note-tracking HMM on the pitch track. Inspiration only.
- [Qwen2-Audio](https://github.com/QwenLM/Qwen2-Audio) (no license, Apache-2.0 weights): audio language model tested on timbre attributes. Inspiration only.
- [rack](https://github.com/sinkingsugar/rack) (MIT OR Apache-2.0): reference for plug-in state and MIDI mapping across formats.
- [RecklessSerum2Bank](https://github.com/RecklessBoise/RecklessSerum2Bank) (MIT code, presets free to use per its README): 250 generated Serum 2 presets, the one open bank of parser fixtures.
- [RemFx](https://github.com/mhrice/RemFx) (Apache-2.0 code, non-commercial weights): effect detection and removal chain, surveyed for the effects layer. Inspiration only.
- [RMVPE](https://github.com/Dream-High/RMVPE) (Apache-2.0): local weighted-average decode over 9 bins.
- [SALMONN](https://github.com/bytedance/SALMONN) (Apache-2.0): audio language model judged weak on synth attributes.
- [SciPy](https://github.com/scipy/scipy) (BSD-3-Clause): reference signal-processing routines, such as Hilbert and Butterworth filters.
- [serum (rich6feet)](https://github.com/rich6feet/serum) (no license found): an index of Serum 2 factory table names and content paths. Inspiration only.
- [Serum 2 preset format gist (0xdevalias)](https://gist.github.com/0xdevalias/5a06349b376d01b2a76ad27a86b08c1b) (no license): original reverse-engineering thread for the Serum preset format. Inspiration only.
- [Serum 2 parameter dump gist (KennethWussmann)](https://gist.github.com/KennethWussmann/5b58e4de728680a0bf8906a8b113103d) (no license): parameter defaults of a freshly loaded Serum 2. Inspiration only.
- [serum-2 (cboone)](https://github.com/cboone/serum-2) (MIT code, CC0 1.0 assets): a working user-wavetable recipe and the Serum 2 file format notes.
- [serum-mcp](https://github.com/Celian-mrc/serum-mcp) (MIT): Serum 2 preset generation driven by a language model, and its parameter tables.
- [serum-preset-packager](https://github.com/KennethWussmann/serum-preset-packager) (no license): first public decoding of the Serum 2 container. Inspiration only.
- [serum-render](https://github.com/wiillownet/serum-render) (GPL-3.0): render reproducibility: batch state carry-over and warm-up render. Inspiration only.
- [serum2-neurosync](https://github.com/HermannPR/serum2-neurosync) (no license found): the folder listing of factory presets as a role taxonomy; no files copied. Inspiration only.
- [serum2-preset-loader](https://github.com/wiillownet/serum2-preset-loader) (MIT): Serum 2 presets as plugin state, and JUCE state notes.
- [serum2gen](https://github.com/dougwithseismic/serum2gen) (MIT): Serum 2 preset reading, writing and parameter aliases.
- [serum2vital](https://github.com/btesser/serum2vital) (GPL-3.0): container facts, modulation source ids, LFO fields, FX rack structure, hosting quirks. Inspiration only.
- [SerumPreset (lwy0v0)](https://github.com/lwy0v0/SerumPreset) (no license found): decoded preset payloads and the hash rule of the preset container. Inspiration only.
- [sms-tools](https://github.com/MTG/sms-tools) (AGPL-3.0): spectral modeling analysis and synthesis reference. Inspiration only.
- [SPTK](https://github.com/sp-nitech/SPTK) (Apache-2.0): reference LPC and cepstrum routines.
- [ssqueezepy](https://github.com/OverLordGoldDragon/ssqueezepy) (MIT): synchrosqueezing and wavelet transform implementation.
- [StemFX](https://github.com/barry-mir/stemfx) (MIT): per-stem effect chain retrieval and transfer.
- [Surge XT](https://github.com/surge-synthesizer/surge) (GPL-3.0): the wavetable chunk format and the factory patch folders. Inspiration only.
- [surge-xt-mcp](https://github.com/redsquidleader/surge-xt-mcp) (MIT): the pattern of setting any plugin parameter by name, and what it cannot reach.
- [SwiftF0](https://github.com/lars76/swift-f0) (MIT): change-point note segmentation with loudness-dip splits.
- [synth-permutations](https://github.com/ben-hayes/synth-permutations) (MIT): metric definitions MSS, wMFCC, SOT and RMS, checked against our own tests.
- [synth-rl](https://github.com/luwux/synth-rl) (GPL-3.0-or-later): edit, repair and match tasks, and the mute and blind controls. Inspiration only.
- [synth-setter](https://github.com/tinaudio/synth-setter) (GPL-3.0): render loop and noise-floor tests. Inspiration only.
- [Syntheon](https://github.com/gudgud96/syntheon) (Apache-2.0): Init-preset-plus-minimal-subset approach and single-cycle wavetable extraction.
- [torchlpc](https://github.com/DiffAPF/torchlpc) (MIT): differentiable linear-prediction filter, read for time-varying filters.
- [torchsynth](https://github.com/torchsynth/torchsynth) (Apache-2.0): seeded random-parameter synthesizer idea for optimizer tests.
- [u-he Preset Randomizer](https://github.com/Fannon/u-he-preset-randomizer) (MIT): per-synth statistical guides and clamp-and-blend variation.
- [Vita](https://github.com/DBraun/Vita) (GPL-3.0): Python bindings over the open Vital engine, run as a separate process for test renders. Inspiration only.
- [Vital](https://github.com/mtytel/vital) (GPL-3.0-or-later): preset file structure, parameter laws, unison, filter and modulation mechanisms, described in our own words. Our own tool extracts its parameter facts (names, ranges, defaults, engine scales, UI and menu labels) into `src/preset/vital/registry.json` for compatibility; no code or comments are copied. Inspiration only.
- [vital-mcp](https://github.com/mard4/vital-mcp) (GPL-3.0): render-measure-fix loop and its descriptor set. Inspiration only.
- [vital_discord_preset_archive](https://github.com/ArcerionDev/vital_discord_preset_archive) (no license found): preset role and macro-label counts; no files copied. Inspiration only.
- [WildFX](https://github.com/IsaacYQH/WildFX) (no license): DAW-hosted plugin pipeline for effect-estimation data. Inspiration only.
- [WORLD](https://github.com/mmorise/World) (BSD-3-Clause): CheapTrick F0-adaptive spectral envelope, ported with attribution if needed.
- [YAMNet](https://github.com/tensorflow/models/tree/master/research/audioset/yamnet) (Apache-2.0): AudioSet event classifier, read as a distortion tag probe.

### Datasets

- [AKWF-FREE](https://github.com/KristofferKarlAxelEkstrand/AKWF-FREE) (CC0-1.0): single-cycle waveforms as bench data for wavetable matching.
- [AudioSet ontology](https://github.com/audioset/ontology) (CC BY-SA 4.0): class names for effect tags. Inspiration only.
- [DX7 timbre dataset](https://zenodo.org/records/4677102) (CC BY 4.0): fixed-pitch timbre-order test for compare.
- [dx7-patches-and-prompts](https://huggingface.co/datasets/ccerati/dx7-patches-and-prompts) (Apache-2.0 per its card, patch origin and caption terms unclear): the vocabulary of generated patch captions. Inspiration only.
- [Faronbi et al. preset dataset](https://zenodo.org/records/7686668) (CC BY 4.0): Serum parameter-vector layout as a reference, not used as data.
- [Freesound metadata scrape](https://huggingface.co/datasets/Chr0my/freesound.org) (CC BY-SA 3.0 per its card, each sound under its own license): tag, title and description word counts only. Inspiration only.
- [IDMT-SMT-Audio-Effects](https://zenodo.org/records/7544032) (CC BY-NC-ND 4.0): labelled guitar and bass effect recordings, surveyed for modulation and distortion. Inspiration only.
- [MusicCaps](https://huggingface.co/datasets/google/MusicCaps) (CC BY-SA 4.0): how listeners describe synth parts, from caption word counts. Inspiration only.
- [NSynth](https://magenta.tensorflow.org/datasets/nsynth) (CC BY 4.0): bright/dark and pitch labels as a feature sanity check.
- [ReverbFX](https://arxiv.org/abs/2505.20533) (CC BY-NC 4.0): room impulse responses derived from reverb plugins, as an effect-estimation reference. Inspiration only.
- [SocialFX listener data, re-upload](https://huggingface.co/datasets/seungheondoh/socialfx-original) (no license found): band means of the listener EQ curves behind each sound word. Inspiration only.
- [Surge pitch dataset](https://zenodo.org/records/4677097) (CC BY 4.0): pitch-labelled test audio and rank-based compare tests.
- [timbremetrics timbre ratings (Thoret et al., Vahidi et al.)](https://github.com/tiianhk/timbremetrics) (no license): 21 human timbre-rating sets as the compare benchmark. Inspiration only.

<!-- ANCHOR_END: credits -->
