//! The spectrogram image: two log-frequency panels of a 10 s passage.
//!
//! The image gives the reader coarse shape only; the numbers live in
//! `layers.json`. The full panel uses 4096-point frames from 20 Hz to
//! 20 kHz. The zoom panel uses 16384-point frames from 20 to 500 Hz, where
//! 4096-point bins of about 11 Hz put a 41 Hz fundamental and its second
//! harmonic only 4 bins apart.

use font8x8::UnicodeFonts as _;

use crate::audio::SampleRate;
use crate::cast;
use crate::dsp::{ActiveFrames, Stft, StftParams, Window};
use crate::units::{Decibels, Hz, Seconds};

/// The image width in pixels. The long edge stays below 1568 pixels, where
/// the image reader of the hosted model downscales.
pub const WIDTH: usize = 1500;
/// The image height in pixels.
pub const HEIGHT: usize = 512;
/// The length of the passage the image shows.
pub const PASSAGE_S: Seconds = Seconds::from_const(10.0);
/// The colour range below the stem peak.
pub const RANGE_DB: Decibels = Decibels::from_const(80.0);

/// One panel: its frame length and its frequency range.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanelSpec {
    /// The STFT frame length.
    pub frame_len: usize,
    /// The lowest frequency, at the bottom edge.
    pub lo: Hz,
    /// The highest frequency, at the top edge.
    pub hi: Hz,
}

/// The two panels, top first.
pub const PANELS: [PanelSpec; 2] = [
    PanelSpec {
        frame_len: 4096,
        lo: Hz::from_const(20.0),
        hi: Hz::from_const(20_000.0),
    },
    PanelSpec {
        frame_len: 16384,
        lo: Hz::from_const(20.0),
        hi: Hz::from_const(500.0),
    },
];

const LEFT: usize = 48;
const RIGHT: usize = 60;
const TITLE: usize = 12;
const PANEL_HEIGHT: usize = 232;
/// The plot width in pixels: one STFT frame per column.
pub const PLOT_WIDTH: usize = WIDTH - LEFT - RIGHT;
/// The plot height of each panel in pixels: one frequency band per row.
pub const PLOT_HEIGHT: usize = PANEL_HEIGHT;
const AXIS_TOP: usize = 2 * (TITLE + PANEL_HEIGHT);
const BACKGROUND: [u8; 3] = [0, 0, 0];
const INK: [u8; 3] = [255, 255, 255];
const FRAME: [u8; 3] = [128, 128, 128];

/// The level matrix of one panel in dB below the stem peak, before the
/// colour map.
#[derive(Clone, Debug, PartialEq)]
pub struct PanelDb {
    spec: PanelSpec,
    db: Vec<f64>,
}

impl PanelDb {
    /// Returns the panel's frame length and range.
    #[must_use]
    pub fn spec(&self) -> PanelSpec {
        self.spec
    }

    /// Returns the levels row by row, top row first, [`PLOT_WIDTH`] per
    /// row and [`PLOT_HEIGHT`] rows, each from `-RANGE_DB` to 0.
    #[must_use]
    pub fn db(&self) -> &[f64] {
        &self.db
    }
}

/// The spectrogram of a passage.
#[derive(Clone, Debug, PartialEq)]
pub struct Spectrogram {
    start: Seconds,
    span: Seconds,
    panels: Vec<PanelDb>,
}

impl Spectrogram {
    /// Computes both panels of a mono signal.
    ///
    /// The passage is the [`PASSAGE_S`] window that holds the most active
    /// 4096-point frames, the earliest on a tie, or the whole signal when
    /// it is shorter. 0 dB is the loudest bin above DC over the whole
    /// signal at the panel's frame length, hop `N / 4`, so both panels
    /// share one scale up to the frame length's resolution.
    #[must_use]
    pub fn of(samples: &[f64], rate: SampleRate) -> Self {
        let (first, len) = passage(samples, rate);
        let panels = PANELS
            .iter()
            .map(|&spec| PanelDb {
                spec,
                db: panel_db(samples, rate, spec, first, len),
            })
            .collect();
        Self {
            start: rate.seconds(first),
            span: rate.seconds(len),
            panels,
        }
    }

    /// Returns the start of the passage.
    #[must_use]
    pub fn start(&self) -> Seconds {
        self.start
    }

    /// Returns the length of the passage.
    #[must_use]
    pub fn span(&self) -> Seconds {
        self.span
    }

    /// Returns the panels, top first.
    #[must_use]
    pub fn panels(&self) -> &[PanelDb] {
        &self.panels
    }

    /// Returns the RGB pixels, row by row, [`WIDTH`] by [`HEIGHT`].
    #[must_use]
    pub fn pixels(&self) -> Vec<u8> {
        let mut canvas = Canvas::new();
        for (index, panel) in self.panels.iter().enumerate() {
            let top = index * (TITLE + PANEL_HEIGHT);
            let spec = panel.spec;
            canvas.text(
                LEFT,
                top + 2,
                &format!(
                    "N = {}, {} Hz to {} Hz, dB below the stem peak",
                    spec.frame_len,
                    hz_label(spec.lo.get()),
                    hz_label(spec.hi.get())
                ),
            );
            let plot_top = top + TITLE;
            for (row, levels) in panel.db.chunks(PLOT_WIDTH).enumerate() {
                for (column, &db) in levels.iter().enumerate() {
                    canvas.set(LEFT + column, plot_top + row, colour(db));
                }
            }
            canvas.outline(LEFT - 1, plot_top - 1, PLOT_WIDTH + 2, PANEL_HEIGHT + 2);
            for &tick in frequency_ticks(spec) {
                let row = frequency_row(spec, tick);
                let y = plot_top + row;
                canvas.hline(LEFT - 4, y, 3);
                let label = hz_label(tick);
                let x = LEFT - 5 - 8 * label.len();
                canvas.text(x, y.saturating_sub(3), &label);
            }
        }
        canvas.text(0, 2, "Hz");
        self.time_axis(&mut canvas);
        colour_bar(&mut canvas);
        canvas.pixels
    }

    fn time_axis(&self, canvas: &mut Canvas) {
        let span = self.span.get();
        let start = self.start.get();
        let step = if span >= 2.0 { 1.0 } else { 0.1 };
        let first = (start / step).ceil();
        let mut index = 0.0;
        while (first + index) * step <= start + span + 1e-9 {
            let time = (first + index) * step;
            index += 1.0;
            let offset = if span > 0.0 {
                (time - start) / span
            } else {
                0.0
            };
            let column = cast::round_to_usize(offset * cast::usize_to_f64(PLOT_WIDTH - 1))
                .unwrap_or(0)
                .min(PLOT_WIDTH - 1);
            let x = LEFT + column;
            canvas.vline(x, AXIS_TOP, 4);
            let label = if step < 1.0 {
                format!("{time:.1}")
            } else {
                format!("{time:.0}")
            };
            let width = 8 * label.len();
            canvas.text(
                x.saturating_sub(width / 2).min(WIDTH - RIGHT - width),
                AXIS_TOP + 6,
                &label,
            );
        }
        canvas.text(WIDTH - RIGHT + 8, AXIS_TOP + 6, "time s");
    }

    /// Encodes the image as an 8-bit RGB PNG.
    ///
    /// # Errors
    ///
    /// Returns a [`png::EncodingError`] if the encoder fails, which an
    /// in-memory buffer of the right size never causes.
    pub fn to_png(&self) -> Result<Vec<u8>, png::EncodingError> {
        let mut bytes = Vec::new();
        let width = u32::try_from(WIDTH).unwrap_or(u32::MAX);
        let height = u32::try_from(HEIGHT).unwrap_or(u32::MAX);
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&self.pixels())?;
        writer.finish()?;
        Ok(bytes)
    }
}

/// The first sample and the length of the passage.
fn passage(samples: &[f64], rate: SampleRate) -> (usize, usize) {
    let window = cast::round_to_usize(PASSAGE_S.get() * rate.hz().get()).unwrap_or(usize::MAX);
    if samples.len() <= window {
        return (0, samples.len());
    }
    let params = StftParams::fixed(PANELS[0].frame_len);
    let hop = params.hop();
    let active = ActiveFrames::of(samples, params);
    // prefix[m] counts the active frames before frame m.
    let mut prefix = vec![0_usize; active.len() + 1];
    for frame in 0..active.len() {
        prefix[frame + 1] = prefix[frame] + usize::from(active.is_active(frame));
    }
    let span = window / hop;
    let last_start = (samples.len() - window) / hop;
    let (mut best, mut best_count) = (0, 0);
    for start in 0..=last_start {
        let end = (start + span).min(active.len());
        let count = prefix[end] - prefix[start.min(end)];
        if count > best_count {
            (best, best_count) = (start, count);
        }
    }
    (best * hop, window)
}

/// The dB matrix of one panel over `len` samples from `first`.
fn panel_db(
    samples: &[f64],
    rate: SampleRate,
    spec: PanelSpec,
    first: usize,
    len: usize,
) -> Vec<f64> {
    let params = StftParams::fixed(spec.frame_len);
    let window_sum: f64 = Window::Hann.samples(spec.frame_len).iter().sum();
    let bin_hz = params.bin_width(rate).get();
    let rows = row_bins(spec, bin_hz, params.bins());
    let mut stft = Stft::new(params);
    let mut peak = 0.0_f64;
    for frame in 0..params.frame_count(samples.len()) {
        // DC is left out, so an offset does not set the scale.
        for value in stft.frame(samples, frame).iter().skip(1) {
            peak = peak.max(value.norm() / window_sum);
        }
    }
    // One frame per column, centred on the column's time.
    let mut columns = Stft::new(params.with_hop(1).unwrap_or(params));
    let mut db = vec![-RANGE_DB.get(); PLOT_WIDTH * PLOT_HEIGHT];
    for column in 0..PLOT_WIDTH {
        let centre = first
            + cast::floor_to_usize(
                (cast::usize_to_f64(column) + 0.5) * cast::usize_to_f64(len)
                    / cast::usize_to_f64(PLOT_WIDTH),
            )
            .unwrap_or(0);
        // With a hop of 1, frame m is centred on sample m.
        let magnitudes: Vec<f64> = columns
            .frame(samples, centre)
            .iter()
            .map(|value| value.norm() / window_sum)
            .collect();
        for (row, bins) in rows.iter().enumerate() {
            db[row * PLOT_WIDTH + column] = level_db(bins.magnitude(&magnitudes), peak);
        }
    }
    db
}

/// A level in dB relative to `peak`, clamped to `-RANGE_DB..=0`; silence
/// and a silent stem read `-RANGE_DB`.
fn level_db(magnitude: f64, peak: f64) -> f64 {
    let floor = -RANGE_DB.get();
    if magnitude > 0.0 && peak > 0.0 {
        (20.0 * (magnitude / peak).log10()).clamp(floor, 0.0)
    } else {
        floor
    }
}

/// How a row reads the spectrum.
#[derive(Clone, Copy, Debug)]
enum RowBins {
    /// The largest magnitude of the bins whose centres lie in the row.
    Max(usize, usize),
    /// A row narrower than a bin: the magnitude interpolated linearly at
    /// the row's centre frequency, between bin `k` and `k + 1`.
    Between(usize, f64),
    /// Above the Nyquist frequency.
    Empty,
}

impl RowBins {
    fn magnitude(self, magnitudes: &[f64]) -> f64 {
        match self {
            Self::Max(first, last) => magnitudes
                .get(first..=last)
                .unwrap_or(&[])
                .iter()
                .copied()
                .fold(0.0, f64::max),
            Self::Between(k, fraction) => match (magnitudes.get(k), magnitudes.get(k + 1)) {
                (Some(a), Some(b)) => a + fraction * (b - a),
                _ => 0.0,
            },
            Self::Empty => 0.0,
        }
    }
}

/// The bins of each row, top row first. Row `r` covers the frequencies
/// from `lo (hi / lo)^(1 - (r + 1) / H)` to `lo (hi / lo)^(1 - r / H)`.
fn row_bins(spec: PanelSpec, bin_hz: f64, bins: usize) -> Vec<RowBins> {
    let height = cast::usize_to_f64(PLOT_HEIGHT);
    let edge = |position: f64| {
        spec.lo.get() * (spec.hi.get() / spec.lo.get()).powf(1.0 - position / height)
    };
    (0..PLOT_HEIGHT)
        .map(|row| {
            let r = cast::usize_to_f64(row);
            let (low, high) = (edge(r + 1.0), edge(r));
            let first = cast::ceil_to_usize(low / bin_hz).unwrap_or(0);
            let last = cast::ceil_to_usize(high / bin_hz)
                .unwrap_or(0)
                .saturating_sub(1);
            if first >= bins {
                RowBins::Empty
            } else if first <= last {
                RowBins::Max(first, last.min(bins - 1))
            } else {
                let centre = (low * high).sqrt() / bin_hz;
                let k = cast::floor_to_usize(centre).unwrap_or(0);
                RowBins::Between(k, centre - cast::usize_to_f64(k))
            }
        })
        .collect()
}

/// The row of a frequency, clamped to the panel.
fn frequency_row(spec: PanelSpec, frequency: f64) -> usize {
    let position = (frequency / spec.lo.get()).ln() / (spec.hi.get() / spec.lo.get()).ln();
    cast::round_to_usize((1.0 - position) * cast::usize_to_f64(PLOT_HEIGHT))
        .unwrap_or(0)
        .min(PLOT_HEIGHT - 1)
}

fn frequency_ticks(spec: PanelSpec) -> &'static [f64] {
    if spec.hi.get() > 1000.0 {
        &[
            20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10_000.0, 20_000.0,
        ]
    } else {
        &[20.0, 30.0, 50.0, 100.0, 200.0, 300.0, 500.0]
    }
}

fn hz_label(frequency: f64) -> String {
    if frequency >= 1000.0 {
        format!("{}k", frequency / 1000.0)
    } else {
        format!("{frequency}")
    }
}

/// The viridis colour of a level from `-RANGE_DB` (dark) to 0 (bright).
fn colour(db: f64) -> [u8; 3] {
    let colour = colorous::VIRIDIS.eval_continuous((db / RANGE_DB.get() + 1.0).clamp(0.0, 1.0));
    [colour.r, colour.g, colour.b]
}

/// The colour bar right of the panels with its dB labels.
fn colour_bar(canvas: &mut Canvas) {
    let x = WIDTH - RIGHT + 8;
    let (top, bottom) = (TITLE, AXIS_TOP - 1);
    let height = bottom - top;
    for y in top..=bottom {
        let db = -RANGE_DB.get() * cast::usize_to_f64(y - top) / cast::usize_to_f64(height);
        for dx in 0..16 {
            canvas.set(x + dx, y, colour(db));
        }
    }
    canvas.outline(x - 1, top - 1, 18, height + 3);
    canvas.text(x + 20, top, "0 dB");
    canvas.text(x + 20, top + height / 2 - 4, "-40");
    canvas.text(x + 20, bottom - 7, "-80");
}

struct Canvas {
    pixels: Vec<u8>,
}

impl Canvas {
    fn new() -> Self {
        Self {
            pixels: BACKGROUND.repeat(WIDTH * HEIGHT),
        }
    }

    fn set(&mut self, x: usize, y: usize, colour: [u8; 3]) {
        if x < WIDTH && y < HEIGHT {
            let index = 3 * (y * WIDTH + x);
            self.pixels[index..index + 3].copy_from_slice(&colour);
        }
    }

    fn hline(&mut self, x: usize, y: usize, len: usize) {
        for dx in 0..len {
            self.set(x + dx, y, INK);
        }
    }

    fn vline(&mut self, x: usize, y: usize, len: usize) {
        for dy in 0..len {
            self.set(x, y + dy, INK);
        }
    }

    /// A one-pixel frame around a `width` by `height` box at `(x, y)`.
    fn outline(&mut self, x: usize, y: usize, width: usize, height: usize) {
        for dx in 0..width {
            self.set(x + dx, y, FRAME);
            self.set(x + dx, y + height - 1, FRAME);
        }
        for dy in 0..height {
            self.set(x, y + dy, FRAME);
            self.set(x + width - 1, y + dy, FRAME);
        }
    }

    /// Text in the 8x8 font with its top left corner at `(x, y)`.
    fn text(&mut self, x: usize, y: usize, text: &str) {
        for (index, character) in text.chars().enumerate() {
            let glyph = font8x8::BASIC_FONTS.get(character).unwrap_or([0; 8]);
            for (dy, bits) in glyph.iter().enumerate() {
                for dx in 0..8 {
                    if bits >> dx & 1 == 1 {
                        self.set(x + 8 * index + dx, y + dy, INK);
                    }
                }
            }
        }
    }
}
