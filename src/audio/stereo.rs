//! The stereo screen: correlation, side over mid and the cross-spectrum
//! phase, for the full band and three bands.
//!
//! The screen cannot see an anti-phase stereo chorus or a light reverb, and
//! a mono stem can still get its width from a reverb tail. The confidence of
//! the screen says so.

use crate::cast;
use crate::confidence::{Confidence, ConfidenceError, Level, NoSpread, Reason, ReasonCode, Signal};
use crate::dsp::{ACTIVE_GATE_DB, ActiveFrames, Complex64, Stft, StftParams, frame_rms};
use crate::units::{Correlation, Decibels, Degrees, Hz};

use super::SampleRate;

/// The upper edge of the sub band.
pub const SUB_EDGE_HZ: Hz = Hz::from_const(120.0);
/// The edge between the low-mid and the high band. The descriptors use it
/// as a band edge too.
pub const HIGH_EDGE_HZ: Hz = Hz::from_const(1000.0);
/// The STFT frame length of the screen, for every f0.
pub const SCREEN_FRAME_LEN: usize = 8192;
/// The harmonics whose cross-spectrum phase the screen averages.
pub const SCREEN_HARMONICS: usize = 20;
/// The convention of the screen.
pub const STEREO_CONVENTION: &str = "rho: Pearson correlation at zero lag; sm_db: \
20 log10(rms(S) / rms(M)) with M = (L + R) / 2 and S = (L - R) / 2; per band from the \
8192-point Hann STFT over bins inside the band, DC excluded; ipd_deg: abs(angle(L conj R)) \
at the largest cross-spectrum bin within 2 bins of h f0, h = 1..20 inside the band, \
weighted by |L conj R|, then the mean over active frames, whose RMS over both channels, \
sqrt((rms_L^2 + rms_R^2) / 2), lies within 40 dB of the loudest; bands: sub below 120 Hz, \
low_mid 120 Hz to 1 kHz, high above 1 kHz";

/// The bands of the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Band {
    /// Every frequency.
    Full,
    /// Below 120 Hz.
    Sub,
    /// From 120 Hz to 1 kHz.
    LowMid,
    /// From 1 kHz up.
    High,
}

impl Band {
    /// Every band, full band first.
    pub const ALL: [Self; 4] = [Self::Full, Self::Sub, Self::LowMid, Self::High];

    /// Returns whether a frequency lies in the band: lower edge inclusive,
    /// upper edge exclusive. 0 Hz lies in none.
    #[must_use]
    pub fn contains(self, frequency: f64) -> bool {
        frequency > 0.0
            && match self {
                Self::Full => true,
                Self::Sub => frequency < SUB_EDGE_HZ.get(),
                Self::LowMid => (SUB_EDGE_HZ.get()..HIGH_EDGE_HZ.get()).contains(&frequency),
                Self::High => frequency >= HIGH_EDGE_HZ.get(),
            }
    }
}

/// The statistics of one band. A value is `None` when it does not exist:
/// a silent channel, a zero mid or side, or no f0.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StereoStats {
    /// The correlation of the two channels.
    pub rho: Option<Correlation>,
    /// Side over mid in decibels.
    pub sm_db: Option<Decibels>,
    /// The mean phase difference at the harmonics of the f0.
    pub ipd_deg: Option<Degrees>,
}

/// The stereo screen of a stem or of a sample range.
#[derive(Clone, Debug, PartialEq)]
pub struct StereoScreen {
    bands: [StereoStats; 4],
    confidence: Confidence<NoSpread>,
}

impl StereoScreen {
    /// The screen of a one-channel file: every value is `None`.
    ///
    /// # Errors
    ///
    /// Never in practice; the reason is built from a fixed count.
    pub fn one_channel() -> Result<Self, ConfidenceError> {
        Ok(Self {
            bands: [StereoStats::default(); 4],
            confidence: Confidence::no_estimate(vec![Reason::new(
                ReasonCode::OneChannel,
                Signal::Count(1),
            )?])?,
        })
    }

    /// Returns the statistics of a band.
    #[must_use]
    pub fn band(&self, band: Band) -> StereoStats {
        self.bands[match band {
            Band::Full => 0,
            Band::Sub => 1,
            Band::LowMid => 2,
            Band::High => 3,
        }]
    }

    /// Returns the confidence of the screen.
    #[must_use]
    pub fn confidence(&self) -> &Confidence<NoSpread> {
        &self.confidence
    }
}

/// Runs the screen on two channels of equal length, or on a sample range
/// of them. `f0` is the fundamental from the caller; without it every
/// `ipd_deg` is `None`.
///
/// # Errors
///
/// Never in practice; the reasons are built from counts and flags.
pub fn stereo_screen(
    left: &[f64],
    right: &[f64],
    rate: SampleRate,
    f0: Option<Hz>,
) -> Result<StereoScreen, ConfidenceError> {
    let len = left.len().min(right.len());
    let (left, right) = (&left[..len], &right[..len]);
    let params = StftParams::fixed(SCREEN_FRAME_LEN);
    // Activity from the energy of both channels, so an anti-phase stem with
    // no mid still counts as sounding.
    let energy: Vec<f64> = frame_rms(left, params)
        .iter()
        .zip(frame_rms(right, params))
        .map(|(l, r)| f64::midpoint(l * l, r * r).sqrt())
        .collect();
    let active = ActiveFrames::from_rms(&energy, ACTIVE_GATE_DB);
    let bin_hz = params.bin_width(rate).get();
    let nyquist = rate.nyquist().get();
    let f0 = f0.map(Hz::get).filter(|f| *f > 0.0);

    // Per band: sums of |L|^2, |R|^2 and Re(L conj R) over all frames.
    let mut sums = [[0.0_f64; 3]; 4];
    // Per band: the per-frame phase means of the active frames.
    let mut phases: [Vec<f64>; 4] = Default::default();
    let mut stft_left = Stft::new(params);
    let mut stft_right = Stft::new(params);
    for frame in 0..params.frame_count(len) {
        let l = stft_left.frame(left, frame).to_vec();
        let r = stft_right.frame(right, frame);
        let cross: Vec<Complex64> = l.iter().zip(r).map(|(a, b)| a * b.conj()).collect();
        for (k, ((a, c), b)) in l.iter().zip(&cross).zip(r).enumerate() {
            let f = cast::usize_to_f64(k) * bin_hz;
            for (index, band) in Band::ALL.iter().enumerate() {
                if band.contains(f) {
                    sums[index][0] += a.norm_sqr();
                    sums[index][1] += b.norm_sqr();
                    sums[index][2] += c.re;
                }
            }
        }
        if let (Some(f0), true) = (f0, active.is_active(frame)) {
            for (index, band) in Band::ALL.iter().enumerate() {
                if let Some(phase) = frame_phase(&cross, f0, bin_hz, nyquist, *band) {
                    phases[index].push(phase);
                }
            }
        }
    }

    let full = StereoStats {
        rho: pearson(left, right),
        sm_db: side_over_mid(left, right),
        ipd_deg: mean_phase(&phases[0]),
    };
    let mut bands = [full; 4];
    for index in 1..4 {
        let [ll, rr, lr] = sums[index];
        bands[index] = StereoStats {
            rho: (ll > 0.0 && rr > 0.0).then(|| Correlation::clamped(lr / (ll * rr).sqrt())),
            sm_db: band_side_over_mid(ll, rr, lr),
            ipd_deg: mean_phase(&phases[index]),
        };
    }
    let reasons = vec![
        Reason::new(ReasonCode::StereoBlindSpot, Signal::Flag(true))?,
        Reason::new(
            ReasonCode::ActiveFrames,
            Signal::Count(u64::try_from(active.count()).unwrap_or(u64::MAX)),
        )?,
    ];
    let confidence = if active.count() == 0 {
        Confidence::no_estimate(vec![Reason::new(
            ReasonCode::SilentStem,
            Signal::Flag(true),
        )?])?
    } else {
        Confidence::estimate(Level::Medium, None, reasons)?
    };
    Ok(StereoScreen { bands, confidence })
}

/// The weighted mean of `abs(angle(L conj R))` at the harmonic peaks of one
/// frame, or `None` when no harmonic lies in the band or every weight is 0.
fn frame_phase(cross: &[Complex64], f0: f64, bin_hz: f64, nyquist: f64, band: Band) -> Option<f64> {
    let (mut weighted, mut total) = (0.0, 0.0);
    for h in 1..=SCREEN_HARMONICS {
        let centre_hz = cast::usize_to_f64(h) * f0;
        if centre_hz >= nyquist || !band.contains(centre_hz) {
            continue;
        }
        let centre = cast::round_to_usize(centre_hz / bin_hz).unwrap_or(0);
        let lo = centre.saturating_sub(2);
        let hi = (centre + 2).min(cross.len().saturating_sub(1));
        let Some(peak) = cross[lo..=hi]
            .iter()
            .max_by(|a, b| a.norm().total_cmp(&b.norm()))
        else {
            continue;
        };
        let weight = peak.norm();
        weighted += weight * peak.arg().abs().to_degrees();
        total += weight;
    }
    (total > 0.0).then(|| weighted / total)
}

fn mean_phase(values: &[f64]) -> Option<Degrees> {
    (!values.is_empty())
        .then(|| Degrees::clamped(values.iter().sum::<f64>() / cast::usize_to_f64(values.len())))
}

/// Pearson correlation at zero lag, or `None` when a channel has no
/// variance.
fn pearson(left: &[f64], right: &[f64]) -> Option<Correlation> {
    if left.is_empty() {
        return None;
    }
    let n = cast::usize_to_f64(left.len());
    let mean_l = left.iter().sum::<f64>() / n;
    let mean_r = right.iter().sum::<f64>() / n;
    let (mut ll, mut rr, mut lr) = (0.0, 0.0, 0.0);
    for (l, r) in left.iter().zip(right) {
        let (dl, dr) = (l - mean_l, r - mean_r);
        ll += dl * dl;
        rr += dr * dr;
        lr += dl * dr;
    }
    (ll > 0.0 && rr > 0.0).then(|| Correlation::clamped(lr / (ll * rr).sqrt()))
}

/// `20 log10(rms(S) / rms(M))`, or `None` when either is zero.
fn side_over_mid(left: &[f64], right: &[f64]) -> Option<Decibels> {
    let (mut mid, mut side) = (0.0, 0.0);
    for (l, r) in left.iter().zip(right) {
        mid += ((l + r) / 2.0).powi(2);
        side += ((l - r) / 2.0).powi(2);
    }
    power_ratio_db(side, mid)
}

/// Side over mid from band sums of `|L|^2`, `|R|^2` and `Re(L conj R)`.
fn band_side_over_mid(ll: f64, rr: f64, lr: f64) -> Option<Decibels> {
    power_ratio_db((ll + rr - 2.0 * lr).max(0.0), (ll + rr + 2.0 * lr).max(0.0))
}

fn power_ratio_db(numerator: f64, denominator: f64) -> Option<Decibels> {
    // Relative to the larger, a residue of rounding is no side signal.
    let floor = 1e-24 * numerator.max(denominator);
    (numerator > floor && denominator > floor)
        .then(|| Decibels::from_power_ratio(numerator / denominator).ok())
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_constants_pass_the_unit_check() {
        assert_eq!(SUB_EDGE_HZ, Hz::new(120.0).unwrap());
        assert_eq!(HIGH_EDGE_HZ, Hz::new(1000.0).unwrap());
    }

    fn rate() -> SampleRate {
        SampleRate::new(44_100).unwrap()
    }

    fn tone(len: usize) -> Vec<f64> {
        (0..len)
            .map(|n| {
                (cast::usize_to_f64(n) * 0.031).sin() + 0.3 * (cast::usize_to_f64(n) * 0.17).sin()
            })
            .collect()
    }

    #[test]
    fn equal_channels_are_fully_correlated_with_no_side_and_no_phase() {
        let x = tone(20_000);
        let f0 = Hz::new(0.031 * 44_100.0 / std::f64::consts::TAU).ok();
        let screen = stereo_screen(&x, &x, rate(), f0).unwrap();
        let full = screen.band(Band::Full);
        assert!((full.rho.unwrap().get() - 1.0).abs() < 1e-12);
        assert_eq!(full.sm_db, None);
        assert!(full.ipd_deg.unwrap().get().abs() < 1e-9);
    }

    #[test]
    fn opposite_channels_are_anti_correlated_with_no_mid() {
        let x = tone(20_000);
        let negated: Vec<f64> = x.iter().map(|v| -v).collect();
        let screen = stereo_screen(&x, &negated, rate(), None).unwrap();
        let full = screen.band(Band::Full);
        assert!((full.rho.unwrap().get() + 1.0).abs() < 1e-12);
        assert_eq!(full.sm_db, None);
        assert_eq!(full.ipd_deg, None);
        // No mid, but both channels sound: an estimate, and with an f0 the
        // phase reads 180 degrees.
        assert!(screen.confidence().is_estimate());
        let f0 = Hz::new(0.031 * 44_100.0 / std::f64::consts::TAU).unwrap();
        let with_f0 = stereo_screen(&x, &negated, rate(), Some(f0)).unwrap();
        let phase = with_f0.band(Band::Full).ipd_deg.unwrap().get();
        assert!((phase - 180.0).abs() < 1e-6, "{phase}");
    }

    #[test]
    fn a_silent_channel_has_no_correlation_and_one_channel_no_values() {
        let x = tone(20_000);
        let screen = stereo_screen(&x, &vec![0.0; 20_000], rate(), None).unwrap();
        assert_eq!(screen.band(Band::Full).rho, None);
        let mono = StereoScreen::one_channel().unwrap();
        assert_eq!(mono.band(Band::High), StereoStats::default());
        assert!(!mono.confidence().is_estimate());
    }

    #[test]
    fn bands_split_at_120_hz_and_1_khz() {
        assert!(Band::Sub.contains(119.9) && !Band::Sub.contains(120.0));
        assert!(Band::LowMid.contains(120.0) && !Band::LowMid.contains(1000.0));
        assert!(Band::High.contains(1000.0));
        assert!(!Band::Full.contains(0.0));
    }
}
