# Analysis pipeline

`sound-to-patch analyze` turns a stem into `layers.json` and
`spectrogram.png`. This page follows the stem through each stage, with the
formula, the convention and the limits of each. The
[common methods page](methods.md) holds the rules that every stage
shares: the window, the frame rule, active frames and the statistics.

Every statement carries a tag:

- **[fact]**: the code does this, or a cited work states it.
- **[synthetic]**: a figure measured on generated signals. Generated
  signals are a best case: a real stem adds bleed, effects and noise.
- **[proposal]**: a threshold or rule chosen for this project that later
  work may tune.

`analyze` runs decoding, the stem, the stereo screen, the descriptors and
the report today [fact]. The analytic signal, the masks, the modulation
spectrum, the periodogram and the decay fits are library stages that the
layer estimators of later releases call [fact].

## Decoding

- The stem decodes at its native sample rate with every channel kept, as
  `f64` samples [fact]. The default audio track is the one the container
  flags as default, else the first with a known codec [fact].
- The build reads WAV, AIFF, FLAC, MP3, AAC (ADTS), MP4 or M4A, Ogg, MKV
  and WebM containers [fact]. A missing codec fails with its name and, if
  one exists, the decoder feature that adds it [fact].
- A packet that fails to decode becomes silence of its duration, so later
  samples keep their time, and the command prints a warning [fact]. More
  than 4 such packets fail the file [proposal].
- Mono is the channel mean, $m = (L + R) / 2$, with no gain normalisation
  [fact]. Analysis never resamples [fact].
- Resampling serves only a consumer with a fixed input rate, such as the
  22050 Hz transcription model [fact]. It is a band-limited FFT conversion
  with $\lceil n \cdot f_{to} / f_{from} \rceil$ output samples [fact]. Some
  rate pairs, among them 48 kHz to 22050 Hz, lag half an output sample, 23
  µs; the lag is documented and not compensated [synthetic]. A NaN or
  infinite sample stops the conversion with its index [fact].

## Transcription model

- The notes come from a transcription model that ships inside the binary
  and runs on a pure-Rust inference engine, with no native library
  [fact]. The model and its decoder are credited on the
  [credits page](../credits.md).
- Licence of the weights [fact]. The bundled model file has no licence
  statement of its own. It equals the file in the upstream repository
  byte for byte: both have the SHA-256
  `2c3c1d144bfa61ad236e92e169c13535c880469a12a047d4e73451f2c059a0ec`,
  and a test pins that hash against the bundled bytes. The upstream
  repository grants Apache-2.0 to every file it holds, the model
  included. The upstream NOTICE text ships in `THIRD-PARTY-LICENSES.html`
  with every release.

## f0 track

- One probabilistic YIN run covers the whole stem at 22050 Hz: frames of
  2048 samples, a hop of 128 (5.8 ms), pitch bins of a tenth of a
  semitone, from 25 Hz to 4200 Hz [fact]. The ceiling sits just above C8,
  the top of the transcription model: with a 400 Hz ceiling every tone
  above it read as a subharmonic [synthetic].
- The copy the tracker reads is scaled to a peak of 1, because the
  tracker treats a frame below a fixed energy as silence [fact]. A sine
  bank at 30.87, 41.2 and 55 Hz reads within 20 cents with no octave
  error [synthetic]. Glides of 150 and 400 ms rise within 5 % of their
  true 10-90 % time; a 50 ms glide reads 35 ms against a true 40 ms
  [synthetic].

## Register and voicing

- The detection sets the decode settings from a probe decode over the
  model's whole range and the f0 track [fact].
- Voicing: a part is polyphonic when 65 % or more of the probe notes start
  within 50 ms of a note of another pitch class and end within 100 ms of
  it, or within a quarter of the shorter note [proposal]. On the generated
  set lines read 0.40 or less and pads 0.88 or more [synthetic]. An
  octave-up ghost shares its note's pitch class and does not count, and
  reverb tails do not end together [fact].
- Register of a monophonic part: the 1st to 99th percentile of the voiced
  f0 frames within 20 dB of a reference level, plus 2 semitones on each
  side [proposal]. The reference is the 95th percentile of the voiced
  frames' levels, so a short loud burst, ring or thump does not move it
  [synthetic]. A polyphonic part takes the lowest and highest probe notes
  instead, because the tracker reads a chord as one low common period
  [synthetic].
- When the f0 range holds fewer than 80 % of the loud probe notes that
  sound alone for at least half their span, the range spans the probe
  notes as well, so a misread f0 track cannot drop the part [proposal]. A
  line plays one note at a time, so an octave-up ghost or bleed over the
  line shares its span with another pitch and does not count [synthetic].
- A monophonic part takes the minimum note length from the BPM; a
  polyphonic part keeps the decoder's 127.7 ms [proposal].
- Limit: bleed louder than -20 dB in a passage where the part rests still
  widens the register, and its notes then fall inside the range
  [synthetic]. Pass `--profile` for such a stem.
- Limit: a line with an occasional held double stop reads as monophonic,
  and the upper note of the stop can fall outside the range [synthetic].
  Pass `--profile pad` for such a stem.

## The stem

- Mid and side are $M = (L + R) / 2$ and $S = (L - R) / 2$ [fact]. A
  one-channel file's mid is its channel and it has no side [fact].
- The beat grid takes the period $60 / \text{BPM}$ seconds from `--bpm`
  and an offset of 0. The offset is given, never estimated [fact].
- The frame length of harmonic analysis is 8192 samples when the note list
  has a median f0 below 100 Hz, else 4096 [fact]. A Hann main lobe is about
  4 bins wide, so harmonics $f_0$ apart need $N \ge 4 f_s / f_0$ [fact].

### Band edge

The band edge is the highest frequency that stays above the noise floor.
A cutoff estimate within one octave of it is never reported [fact].

1. The long-term power spectrum of the mid is averaged over the active
   frames that lie wholly inside the stem, in dB [fact].
2. A running median over one semitone on each side smooths it [fact].
3. Above 2 kHz, the steepest fall of the smoothed level over one semitone
   is found. When it is at least 25 dB and the median level of the octave
   below lies within 20 dB of where the fall starts, the edge is its
   half-depth point, at high confidence [fact, proposal for the 25 and
   20 dB].
4. Otherwise the floor is the lowest level above 1 kHz, and the edge is
   the highest bin at least 30 dB above it, at low confidence. With no such
   bin the edge is the Nyquist frequency with the reason `no_band_edge`
   [fact, proposal for the 30 dB].

The reason `band_edge_drop` carries the depth of the steepest fall [fact].
The plateau check of step 3 keeps a lone tone's skirt from counting as a
cut [fact].

- Synthetic cuts fall 57 to 91 dB over one semitone, and a 2-pole roll-off
  at 3 kHz about 1.3 dB, so the 25 dB threshold separates them with room
  on both sides [synthetic].
- On white, $1/\sqrt{f}$ and $1/f$ noise cut at 8 and 16 kHz at 44.1 and
  48 kHz, the steepest fall erred +0.01 to +0.07 semitone and the margin
  rule +0.03 to +0.16 [synthetic]. With a noise shelf 60 dB down above a
  16 kHz cut, the margin rule erred +3.2 semitones and the steepest fall
  +0.02 [synthetic]. Full-band white noise and the 2-pole roll-off have no
  steep fall, and both rules give the same margin value [synthetic].

### Flat tops

`hard_limited` is true when runs of at least 1 ms of samples within 1e-5
of the channel peak cover at least 0.1 % of a channel [fact, proposal for
the thresholds]. A run counts only when the samples on both sides lie at
least half the peak on the same side: clipping runs into the ceiling,
while a square or pulse wave jumps to it [fact]. Square and pulse waves at
55 and 220 Hz do not flag [synthetic]. The level descriptors carry the
flag as a reason [fact].

## Stereo screen

The screen runs on L and R with 8192-point Hann frames for every f0
[fact].

- $\rho$ is the Pearson correlation of L and R at zero lag [fact].
- $\text{sm}_{dB} = 20 \log_{10}(\text{rms}(S) / \text{rms}(M))$ [fact].
- Per band the screen sums $|L_k|^2$, $|R_k|^2$ and
  $\operatorname{Re}(L_k \overline{R_k})$ over the bins inside the band,
  DC left out, and forms both values from the sums [fact]. The bands are
  sub below 120 Hz, low_mid from 120 Hz to 1 kHz and high above [fact,
  proposal for the edges].
- `ipd_deg` needs the median f0 of a note list. At each harmonic
  $h f_0$, $h = 1 \ldots 20$, it takes the largest cross-spectrum bin
  within 2 bins and the phase $|\angle(L \overline{R})|$, weighted by
  $|L \overline{R}|$, then the mean over active frames [fact].
- A frame is active from the RMS over both channels,
  $\sqrt{(\text{rms}_L^2 + \text{rms}_R^2) / 2}$, so an anti-phase stem with
  no mid still counts [fact]. $L = -R$ reads 180 degrees [synthetic].
- The screen cannot see an anti-phase chorus or a light reverb, and a
  mono source can still get width from a reverb tail. Its confidence
  carries the reason `stereo_blind_spot` [fact].
- Detuned unison voices share one bin at harmonics 1 and 2 of an
  8192-point frame, so their beat phase raises `ipd_deg` to 12 to 43
  degrees for panned 3- and 7-voice unison; harmonics where the voices
  resolve read 0.2 to 4.6 degrees [synthetic].

## STFT and peaks

- Frames are centred: frame $m$ covers samples $m H - N/2$ to
  $m H - N/2 + N - 1$, zero-padded past the ends [fact]. A signal of $n$
  samples gives $1 + \lfloor n / H \rfloor$ frames for an even $N$ [fact].
- The transform is not normalised:
  $X[m, k] = \sum_n w[n]\, x[mH - N/2 + n]\, e^{-j 2 \pi k n / (pN)}$,
  with oversampling $p$ that zero-pads each frame [fact]. Oversampling
  refines peak positions and changes no resolution [fact].
- A frame is computed one at a time, so a long stem never holds its whole
  spectrogram [fact].
- A peak is a strict local maximum, refined by a parabola through the log
  magnitudes $a, b, c$ of the bin and its neighbours:
  $\delta = \frac{1}{2} \frac{a - c}{a - 2b + c}$ [fact]. Each peak also
  records its height over its neighbours in dB [fact].
- For one 100 Hz sine at 30 dB SNR in 8192-point frames at 44.1 kHz, 300
  trials, the Hann peak has a standard deviation of 1.6 mHz against a
  bound of 0.7 mHz, and the rectangular window 22 mHz [synthetic]. Without
  noise the Hann bias stays below 0.016 bin without padding and 1.1 mHz
  with 4 times padding [synthetic].

## Analytic signal

- The analytic signal $z = x + j \mathcal{H}\{x\}$ comes from a zero-padded
  FFT [fact]. It means something only for one narrowband component, so a
  caller band-passes around one partial first [fact]. Its memory grows
  with the segment, so it serves note-sized segments only [fact].
- The heterodyne multiplies by $2 e^{-j 2 \pi f_c t}$, low-passes with a
  fourth-order Butterworth and decimates [fact]. A partial
  $A \cos(2 \pi f t + \varphi)$ comes out as
  $A e^{j(2 \pi (f - f_c) t + \varphi)}$, so the envelope reads $A$ [fact].
- The zero-phase low-pass runs the Butterworth forward and backward: -6 dB
  at the cutoff, 48 dB per octave, no delay [fact]. Each end is extended by
  an odd reflection of three cutoff periods, and each pass starts in the
  steady state of its input, so a constant or a ramp passes without a
  transient [fact]. Attack envelopes use a 150 Hz cutoff, because a 5 Hz
  smoother smears attacks shorter than tens of milliseconds [fact,
  proposal for 150 Hz].
- The band envelope mixes the band centre to DC, low-passes at half the
  bandwidth forward and backward with an even reflection at each end, and
  takes the magnitude [fact]. Its memory stays linear in the length
  [fact]. A 10 % AM at 55 Hz reads a depth of 0.1013 over the whole track
  [synthetic].

## Note masks

- The mask of note $n$ at frequency $f$ and time $t$ is
  $M_n(f, t) = \sum_{h=1}^{H} e^{-(f - h f_n(t))^2 / (2 \sigma^2)}$, with
  $f_n(t)$ following the note's contour [fact].
- The defaults are $H = 20$ harmonics, $\sigma = f_s / N$ (one unpadded
  bin) and a threshold of $e^{-2}$, the mask value two $\sigma$ from a
  harmonic, which covers the 4-bin Hann main lobe [fact, proposal for the
  values]. Harmonics at or above the Nyquist frequency are skipped [fact].
- Each bin of a frame records the note that claims it and whether a second
  note claims it too [fact].
- The out-of-mask share is the power $|X|^2$ outside every mask over all
  cells: bleed plus any note the list missed [fact].
- `analyze --notes` reads the note list from a Standard MIDI File: note-on
  to the next note-off of the same key and channel, first in, first out,
  with the times from the file's tempo map [fact]. Notes on channel 10,
  the percussion channel, are left out, because their keys name drums
  [fact].

## Modulation spectrum

- Band envelopes are tracked at 200 Hz, a 5 ms hop [fact].
- The relative modulation of a track is its natural log, floored 60 dB
  below the peak, minus its least-squares line [fact]. The log turns a
  decay in dB times an LFO into a line plus the LFO [fact]. A cubic lost a
  swell's 5 Hz LFO and a quintic misread a 2-cycle 4-bar LFO, where the
  line kept both [synthetic].
- The spectrum is the magnitude of the Hann-windowed track, zero-padded 8
  times; the resolution stays $1/T$ [fact].
- The scan runs from two semitones below $\min(0.2\ \text{Hz}, \text{the
  4-bar rate})$ to 20 Hz, so the 2-bar and 4-bar divisions have room for
  their peak [fact]. Above 20 Hz a modulation shows as sidebands, not in
  the envelope [fact].
- The rate is the strongest strict maximum in the scan, with guard points
  outside it as neighbours, refined by a least-squares sine fit [fact]. It
  needs a line 8 times the median amplitude of the scan, a depth of at
  least 0.5 % and two full cycles, else no rate [fact, proposal for the
  thresholds]. The reasons `line_prominence` and `cycles_in_span` carry
  both values, with a rate or without one [fact].
- On 200 seeded noise tracks the largest line stood at most 4.4 times the
  median [synthetic]. On a 104-case grid of periods, depths and rates the
  strongest line missed 0 sine, 8 saw, 4 square and 4 triangle cases
  [synthetic]. Noise, a steady tone and a pure decay give no rate
  [synthetic].
- The period search uses the cumulative mean normalised difference of the
  detrended track: the first lag below 0.1, else the global minimum,
  refined by a parabola [fact]. A 7.3 Hz modulation reads 7.30 Hz
  [synthetic].
- Folding averages the track over a period into 64 phase bins [fact]. The
  shape reads square-like with a plateau of 35 % of the cycle, a ramp up
  with a rise of 75 % or more, a ramp down with 25 % or less, and smooth
  otherwise [proposal].
- A rate snaps to the nearest note division in log distance, at the BPM
  and at tempos within 2 % in 0.1 % steps [fact].

## Periodogram

Note gaps make a plain FFT see the note rhythm as modulation, so gated
samples go to the Lomb-Scargle periodogram [fact].

- A sample counts when its level lies within 40 dB of the track's peak,
  inside a note, at least 60 ms after the onset [fact, proposal for 60 ms].
- Each trial frequency fits a mean, a sine and a cosine together: the
  floating-mean form [fact]. On gated notes the plain form put a rate at the
  note rate on its harmonics [synthetic].
- The power is $A^2 N / 4$ for a sine of amplitude $A$ over $N$ samples,
  and a constant offset changes nothing [fact]. The line amplitude is
  $2 \sqrt{P / N}$ of the log level, the same depth as in the modulation
  spectrum [fact].
- The grid steps by 0.01 Hz, and the pick and its thresholds match the
  modulation spectrum [fact]. The spread is the resolution $1/T$ of the
  span [fact].

## Backward integration

- The decay curve of a tail after a stop is the Schroeder integral of the
  squared signal, from each sample to the end [fact]. A stem has no impulse
  response, so the stop comes from the note list, never from a silence
  [fact].
- The analysis starts at the loudest 10 ms block. The floor is the mean
  power of the last 10 % of the tail [fact].
- A line through the 10 ms block levels down to 10 dB above the floor gives
  the time where the decay meets it; the integral stops there and adds the
  energy the fitted decay would add after it [fact].
- The decay time is $-60 / \text{slope}$ of a line fitted between -5 and
  -25 dB (T20) or -5 and -35 dB (T30) [fact]. A fit fails when the floor
  lies less than 10 dB below the bottom of its range [fact, proposal for
  10 dB].

## Descriptors

Descriptors run on the mid with Hann frames of the frame rule, over active
frames [fact].

- Shape descriptors use magnitude weights $|X_k|$ divided by the window
  sum [fact]. The centroid is $\sum_k f_k |X_k| / \sum_k |X_k|$, the
  spread $\sqrt{\sum_k (f_k - c)^2 |X_k| / \sum_k |X_k|}$, and the rolloff
  the lowest bin where the cumulative magnitude reaches 85 % or 95 %
  [fact].
- Flatness is the geometric over the arithmetic mean of the magnitude from
  100 Hz to 10 kHz, each bin floored at $10^{-5}$ after the window-sum
  scale [fact].
- Band shares use power $|X_k|^2$ summed over active frames per band,
  divided by the power in the bands reported; the edges are 20, 60, 250
  Hz, 1, 4, 10 and 20 kHz [fact, proposal for the edges]. A band that
  starts at or above the Nyquist frequency is left out, and one that
  crosses it ends there, so a 16 kHz file reports five bands, the last
  from 4 to 8 kHz [fact].
- Every statistic over frames, the band shares and the band edge need at
  least 16 active frames, else no estimate with the reason
  `too_few_frames` [fact, proposal for 16]. On noise, sawtooth and sine
  clips at 48 kHz the worst error of the medians against a 10 s run was 4 %
  at 10 frames and 2 % at 16, and many times the value at 8 or fewer
  [synthetic].
- Peak and RMS are of the mono mix over the whole stem in dB re full scale
  1.0, and the crest is their difference [fact]. Digital silence gives no
  estimate with the reason `silent_stem`, never $-\infty$ [fact].
- The conventions of published tools differ in weights, floors and
  rolloff arguments, so a value is comparable only with its convention
  [fact]. An ideal 55 Hz sawtooth has a magnitude-weighted centroid of
  3348 Hz and a power-weighted one of 220 Hz, 15 times lower [synthetic].
- `centroid_over_f0` divides each frame's centroid by the median f0 of the
  note list [fact].

## The report

- `layers.json` holds the `schema` and `synth` fields, the `stem` group,
  the `transcription` group and the `conventions` group; the
  [layer report page](../reference/formats/layers.md) lists them [fact].
- Output is deterministic: two runs on the same input write the same bytes
  [fact]. `layers.json` names its stem in `source`, and a run of another
  stem into the same folder stops unless forced [fact].
- Each file is written to a temporary sibling and renamed into place, and
  `layers.json` comes last, after the old one is removed. A `layers.json`
  that exists therefore always belongs to the spectrogram next to it, and
  neither file is ever cut short [fact].
- `spectrogram.png` is 1500 by 512 pixels, below the size at which a
  hosted image reader downscales [fact]. The image gives coarse shape; the
  numbers live in the JSON [fact].
- The top panel uses 4096-point frames from 20 Hz to 20 kHz, the bottom
  panel 16384-point frames from 20 to 500 Hz, both on a log-frequency axis
  [fact]. Each row takes the largest bin magnitude in its band, or the
  linear interpolation at its centre when the row is narrower than a bin
  [fact]. One frame is centred on each column [fact].
- The passage is the 10 s window with the most active 4096-point frames,
  the earliest on a tie, or the whole stem when shorter [fact, proposal for
  the rule].
- Colour is viridis over 80 dB below the stem peak: the loudest bin above
  DC over the whole stem at the panel's frame length [fact]. A relative
  scale keeps strong partials from clipping to the top colour [fact].
  Every level is finite and clamped before the colour map [fact].

## Sources

Read for the problem statements and the formulas; the code is our own.

- Window choice and the peak accuracy bound: Harris,
  [On the use of windows for harmonic analysis with the discrete Fourier transform](https://doi.org/10.1109/PROC.1978.10837);
  Rife and Boorstyn,
  [Single-tone parameter estimation from discrete-time observations](https://doi.org/10.1109/TIT.1974.1055282).
- Analytic signal: Boashash,
  [Estimating and interpreting the instantaneous frequency of a signal](https://doi.org/10.1109/5.135376).
- Harmonic masks: Klapuri,
  [Multiple fundamental frequency estimation based on harmonicity and spectral smoothness](https://doi.org/10.1109/TSA.2003.815516).
- Modulation spectrum and period search: Atlas and Shamma,
  [Joint Acoustic and Modulation Frequency](https://doi.org/10.1155/S1110865703305013);
  de Cheveigné and Kawahara,
  [YIN, a fundamental frequency estimator for speech and music](https://doi.org/10.1121/1.1458024).
- Periodogram: Lomb,
  [Least-squares frequency analysis of unequally spaced data](https://doi.org/10.1007/BF00648343);
  Scargle,
  [Studies in astronomical time series analysis. II](https://doi.org/10.1086/160554);
  Zechmeister and Kürster,
  [The generalised Lomb-Scargle periodogram](https://doi.org/10.1051/0004-6361:200811296).
- Backward integration: Schroeder,
  [New Method of Measuring Reverberation Time](https://doi.org/10.1121/1.1909343).
- Descriptor definitions: Peeters et al.,
  [The Timbre Toolbox](https://doi.org/10.1121/1.3642604).
