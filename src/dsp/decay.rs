//! Decay times from backward integration of the energy after a stop.
//!
//! A stem has no impulse response, so the Schroeder integral runs on the
//! squared signal of a tail after an abrupt stop, such as the last note-off
//! of a phrase. The integral smooths the random fluctuation of a noisy
//! tail. Bleed raises the floor, so the caller takes the stop from the note
//! list, never from any silence.

use std::error::Error;
use std::fmt;

use crate::audio::SampleRate;
use crate::cast;
use crate::units::{Decibels, Seconds};

/// Length of the blocks that estimate the level and the noise floor.
const BLOCK_SECONDS: f64 = 0.01;
/// Share of the tail at its end that estimates the noise floor.
const FLOOR_SHARE: f64 = 0.1;
/// The floor must lie this far below the bottom of a fit range.
const FLOOR_MARGIN_DB: f64 = 10.0;

/// The level range of a decay fit, in decibels below the start of the
/// energy decay curve.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FitRange {
    /// From -5 dB to -25 dB.
    T20,
    /// From -5 dB to -35 dB.
    T30,
}

impl FitRange {
    /// Returns the top and the bottom of the range.
    #[must_use]
    pub fn limits(self) -> (Decibels, Decibels) {
        let (top, bottom) = match self {
            Self::T20 => (-5.0, -25.0),
            Self::T30 => (-5.0, -35.0),
        };
        (Decibels::from_const(top), Decibels::from_const(bottom))
    }
}

/// Why a decay fit has no value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DecayFailure {
    /// The tail is digital silence.
    Silent,
    /// The tail is too short to fit a line.
    TooShort,
    /// The noise floor sits less than 10 dB below the bottom of the range.
    FloorTooHigh {
        /// The floor relative to the level at the stop.
        floor: Decibels,
        /// The highest floor the range accepts.
        limit: Decibels,
    },
    /// The decay curve does not fall to the bottom of the range before the
    /// tail meets the floor.
    RangeNotReached,
}

impl fmt::Display for DecayFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Silent => write!(f, "the tail is silent"),
            Self::TooShort => write!(f, "the tail is too short for a decay fit"),
            Self::FloorTooHigh { floor, limit } => write!(
                f,
                "the noise floor at {floor} is above the {limit} that the fit range needs"
            ),
            Self::RangeNotReached => {
                write!(f, "the decay does not reach the bottom of the fit range")
            }
        }
    }
}

impl Error for DecayFailure {}

/// The decay of one tail: the noise floor and a decay time per fit range.
#[derive(Clone, Debug, PartialEq)]
pub struct Decay {
    floor: Option<Decibels>,
    truncation: Seconds,
    t20: Result<Seconds, DecayFailure>,
    t30: Result<Seconds, DecayFailure>,
}

impl Decay {
    /// Analyses the samples of a tail that starts at a stop.
    ///
    /// The analysis starts at the loudest 10 ms block, which is the first
    /// block unless the tail opens with silence or a pre-delay. That block
    /// is the reference level.
    ///
    /// 1. The noise floor is the mean power of the last 10 % of the tail,
    ///    relative to the power of the reference block.
    /// 2. A line fitted to the 10 ms block levels down to 10 dB above the
    ///    floor gives the time where the decay meets the floor.
    /// 3. The Schroeder integral runs from each sample to that time, plus
    ///    the energy the decay would add after it at the fitted rate.
    /// 4. A line fitted to the decay curve between the range limits gives
    ///    the decay time, `-60 / slope`: the time a 60 dB drop takes.
    ///
    /// A fit fails when the floor lies less than 10 dB below the bottom of
    /// its range or the curve never reaches that bottom.
    #[must_use]
    pub fn of(tail: &[f64], rate: SampleRate) -> Self {
        match Self::analyse(tail, rate) {
            Ok(decay) => decay,
            Err(failure) => Self {
                floor: None,
                truncation: rate.seconds(tail.len()),
                t20: Err(failure),
                t30: Err(failure),
            },
        }
    }

    fn analyse(tail: &[f64], rate: SampleRate) -> Result<Self, DecayFailure> {
        let fs = rate.hz().get();
        let block = cast::round_to_usize(BLOCK_SECONDS * fs).unwrap_or(1).max(1);
        if tail.iter().all(|&x| x == 0.0) {
            return Err(DecayFailure::Silent);
        }
        let loudest = tail
            .chunks_exact(block)
            .map(|chunk| chunk.iter().map(|x| x * x).sum::<f64>())
            .enumerate()
            .fold(
                (0, 0.0),
                |best, (index, sum)| if sum > best.1 { (index, sum) } else { best },
            )
            .0;
        let offset = loudest * block;
        Self::analyse_from(&tail[offset..], rate, block, offset)
    }

    /// Analyses a tail whose first block is the reference and that starts
    /// `offset` samples after the stop.
    fn analyse_from(
        tail: &[f64],
        rate: SampleRate,
        block: usize,
        offset: usize,
    ) -> Result<Self, DecayFailure> {
        let fs = rate.hz().get();
        let energy: Vec<f64> = tail.iter().map(|x| x * x).collect();
        if energy.len() < 3 * block {
            return Err(DecayFailure::TooShort);
        }
        let levels: Vec<f64> = energy
            .chunks_exact(block)
            .map(|chunk| chunk.iter().sum::<f64>() / cast::usize_to_f64(block))
            .collect();
        let reference = levels[0];
        if reference <= 0.0 {
            return Err(DecayFailure::Silent);
        }
        let floor_start = energy.len()
            - cast::ceil_to_usize(FLOOR_SHARE * cast::usize_to_f64(energy.len()))
                .unwrap_or(block)
                .max(block);
        let noise = energy[floor_start..].iter().sum::<f64>()
            / cast::usize_to_f64(energy.len() - floor_start);
        let floor = (noise > 0.0).then(|| power_db(noise / reference));

        // Block levels from the start down to 10 dB above the floor.
        let floor_db = floor.unwrap_or(f64::NEG_INFINITY);
        let points: Vec<(f64, f64)> = levels
            .iter()
            .enumerate()
            .map(|(index, &level)| {
                let centre = (cast::usize_to_f64(index) + 0.5) * cast::usize_to_f64(block) / fs;
                (
                    centre,
                    if level > 0.0 {
                        power_db(level / reference)
                    } else {
                        f64::NEG_INFINITY
                    },
                )
            })
            .take_while(|&(_, level)| level >= floor_db + FLOOR_MARGIN_DB)
            .collect();
        if points.len() < 2 {
            return Err(DecayFailure::FloorTooHigh {
                floor: Decibels::clamped(floor_db),
                limit: Decibels::clamped(-FLOOR_MARGIN_DB),
            });
        }
        let (slope, intercept) = fit_line(&points).ok_or(DecayFailure::TooShort)?;
        let end = cast::usize_to_f64(energy.len()) / fs;
        let (cut, compensation) = if slope < 0.0 && floor.is_some() {
            let crossing = ((floor_db - intercept) / slope).clamp(0.0, end);
            // Power decays as exp(-t / tau) with tau = 10 / (ln 10 |slope|).
            let tau = 10.0 / (std::f64::consts::LN_10 * slope.abs());
            (crossing, noise * tau * fs)
        } else {
            (end, 0.0)
        };
        let cut_index = cast::floor_to_usize(cut * fs)
            .unwrap_or(energy.len())
            .clamp(1, energy.len());
        let mut curve = backward_integral(&energy[..cut_index]);
        for value in &mut curve {
            *value += compensation;
        }
        Ok(Self {
            floor: floor.map(Decibels::clamped),
            truncation: rate.seconds(offset + cut_index),
            t20: fit_range(&curve, fs, floor, FitRange::T20),
            t30: fit_range(&curve, fs, floor, FitRange::T30),
        })
    }

    /// Returns the noise floor relative to the level at the stop, or `None`
    /// when the end of the tail is digital silence.
    #[must_use]
    pub fn floor(&self) -> Option<Decibels> {
        self.floor
    }

    /// Returns the time after the stop where the integral was truncated.
    #[must_use]
    pub fn truncation(&self) -> Seconds {
        self.truncation
    }

    /// Returns the decay time from the -5 to -25 dB fit.
    ///
    /// # Errors
    ///
    /// Returns the [`DecayFailure`] of the fit.
    pub fn t20(&self) -> Result<Seconds, DecayFailure> {
        self.t20
    }

    /// Returns the decay time from the -5 to -35 dB fit.
    ///
    /// # Errors
    ///
    /// Returns the [`DecayFailure`] of the fit.
    pub fn t30(&self) -> Result<Seconds, DecayFailure> {
        self.t30
    }
}

fn power_db(ratio: f64) -> f64 {
    10.0 * ratio.log10()
}

/// The Schroeder integral of an energy sequence: element `n` is the sum
/// from `n` to the end. Summed backward, so the small late terms add first.
#[must_use]
pub fn backward_integral(energy: &[f64]) -> Vec<f64> {
    let mut curve = vec![0.0; energy.len()];
    let mut sum = 0.0;
    for (slot, &value) in curve.iter_mut().zip(energy).rev() {
        sum += value;
        *slot = sum;
    }
    curve
}

fn fit_range(
    curve: &[f64],
    fs: f64,
    floor: Option<f64>,
    range: FitRange,
) -> Result<Seconds, DecayFailure> {
    let (top, bottom) = range.limits();
    let (top, bottom) = (top.get(), bottom.get());
    if let Some(floor) = floor
        && floor > bottom - FLOOR_MARGIN_DB
    {
        return Err(DecayFailure::FloorTooHigh {
            floor: Decibels::clamped(floor),
            limit: Decibels::clamped(bottom - FLOOR_MARGIN_DB),
        });
    }
    let start = curve
        .first()
        .copied()
        .filter(|&v| v > 0.0)
        .ok_or(DecayFailure::Silent)?;
    let levels: Vec<f64> = curve.iter().map(|&v| power_db(v / start)).collect();
    if levels.last().is_none_or(|&last| last > bottom) {
        return Err(DecayFailure::RangeNotReached);
    }
    let points: Vec<(f64, f64)> = levels
        .iter()
        .enumerate()
        .filter(|&(_, &level)| level <= top && level >= bottom)
        .map(|(n, &level)| (cast::usize_to_f64(n) / fs, level))
        .collect();
    let (slope, _) = fit_line(&points).ok_or(DecayFailure::TooShort)?;
    if slope >= 0.0 {
        return Err(DecayFailure::RangeNotReached);
    }
    Ok(Seconds::clamped(-60.0 / slope))
}

/// Least-squares line through `(x, y)` points: `(slope, intercept)`.
pub(crate) fn fit_line(points: &[(f64, f64)]) -> Option<(f64, f64)> {
    if points.len() < 2 {
        return None;
    }
    let n = cast::usize_to_f64(points.len());
    let mean_x = points.iter().map(|p| p.0).sum::<f64>() / n;
    let mean_y = points.iter().map(|p| p.1).sum::<f64>() / n;
    let sxx: f64 = points.iter().map(|p| (p.0 - mean_x).powi(2)).sum();
    let sxy: f64 = points.iter().map(|p| (p.0 - mean_x) * (p.1 - mean_y)).sum();
    (sxx > 0.0).then(|| {
        let slope = sxy / sxx;
        (slope, mean_y - slope * mean_x)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fit_limits_pass_the_unit_check() {
        let db = |v| Decibels::new(v).unwrap();
        assert_eq!(FitRange::T20.limits(), (db(-5.0), db(-25.0)));
        assert_eq!(FitRange::T30.limits(), (db(-5.0), db(-35.0)));
    }

    fn rate() -> SampleRate {
        SampleRate::new(16_000).unwrap()
    }

    fn exponential(rt60: f64, seconds: f64) -> Vec<f64> {
        let fs = 16_000.0;
        (0..cast::floor_to_usize(seconds * fs).unwrap())
            .map(|n| (-6.907_755_278_982_137 * cast::usize_to_f64(n) / fs / rt60).exp())
            .collect()
    }

    #[test]
    fn the_backward_integral_sums_to_the_end() {
        assert_eq!(backward_integral(&[1.0, 2.0, 3.0]), vec![6.0, 5.0, 3.0]);
    }

    #[test]
    fn a_pure_exponential_decays_60_db_per_rt60() {
        // Amplitude exp(-ln(1000) t / RT60) is -60 dB at RT60 exactly.
        let decay = Decay::of(&exponential(0.4, 2.0), rate());
        for time in [decay.t20().unwrap(), decay.t30().unwrap()] {
            assert!((time.get() - 0.4).abs() < 0.4e-3, "{time:?}");
        }
    }

    #[test]
    fn silence_and_short_tails_fail() {
        assert_eq!(
            Decay::of(&vec![0.0; 16_000], rate()).t20(),
            Err(DecayFailure::Silent)
        );
        assert_eq!(
            Decay::of(&[1.0; 100], rate()).t30(),
            Err(DecayFailure::TooShort)
        );
    }

    #[test]
    fn a_flat_tail_never_reaches_the_range() {
        let decay = Decay::of(&vec![0.5; 16_000], rate());
        assert!(
            matches!(decay.t20(), Err(DecayFailure::FloorTooHigh { .. })),
            "{decay:?}"
        );
    }
}
