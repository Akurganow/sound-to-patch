//! Small deterministic minimisers for the layer fits: a golden-section
//! search on one parameter and a Nelder-Mead simplex on a few.

use crate::cast;

/// `(sqrt(5) - 1) / 2`.
const INVERSE_PHI: f64 = 0.618_033_988_749_894_9;

/// Minimises `f` on `lo..=hi` by golden-section search, down to an interval
/// of `tolerance`. Returns the argument and the value at it. `f` should
/// have one minimum in the interval; with several it finds one of them.
pub(crate) fn golden_section(
    f: impl Fn(f64) -> f64,
    mut lo: f64,
    mut hi: f64,
    tolerance: f64,
) -> (f64, f64) {
    let mut a = hi - INVERSE_PHI * (hi - lo);
    let mut b = lo + INVERSE_PHI * (hi - lo);
    let (mut fa, mut fb) = (f(a), f(b));
    // A bounded loop: each step shrinks the interval by 0.618, so 200 steps
    // reach any tolerance above 1e-40 of the start width.
    for _ in 0..200 {
        if hi - lo <= tolerance {
            break;
        }
        if fa < fb {
            hi = b;
            b = a;
            fb = fa;
            a = hi - INVERSE_PHI * (hi - lo);
            fa = f(a);
        } else {
            lo = a;
            a = b;
            fa = fb;
            b = lo + INVERSE_PHI * (hi - lo);
            fb = f(b);
        }
    }
    if fa < fb { (a, fa) } else { (b, fb) }
}

/// Minimises `f` from `start` with a Nelder-Mead simplex whose first
/// vertices step `steps` from the start, for at most `iterations` steps or
/// until the values of the simplex lie within `tolerance` of each other.
/// Returns the best point and its value. The standard coefficients:
/// reflection 1, expansion 2, contraction 0.5, shrink 0.5.
pub(crate) fn nelder_mead<const N: usize>(
    f: impl Fn(&[f64; N]) -> f64,
    start: [f64; N],
    steps: [f64; N],
    iterations: usize,
    tolerance: f64,
) -> ([f64; N], f64) {
    let mut simplex: Vec<([f64; N], f64)> = std::iter::once(start)
        .chain((0..N).map(|i| {
            let mut vertex = start;
            vertex[i] += steps[i];
            vertex
        }))
        .map(|vertex| (vertex, f(&vertex)))
        .collect();
    let towards = |from: &[f64; N], to: &[f64; N], t: f64| {
        let mut out = *from;
        for ((o, &a), &b) in out.iter_mut().zip(from).zip(to) {
            *o = a + t * (b - a);
        }
        out
    };
    for _ in 0..iterations {
        simplex.sort_by(|a, b| a.1.total_cmp(&b.1));
        let (best, worst) = (simplex[0].1, simplex[N].1);
        if (worst - best).abs() <= tolerance {
            break;
        }
        let mut centroid = [0.0; N];
        for (vertex, _) in &simplex[..N] {
            for (c, v) in centroid.iter_mut().zip(vertex) {
                *c += v / cast::usize_to_f64(N);
            }
        }
        let worst_vertex = simplex[N].0;
        let reflected = towards(&centroid, &worst_vertex, -1.0);
        let fr = f(&reflected);
        if fr < best {
            let expanded = towards(&centroid, &worst_vertex, -2.0);
            let fe = f(&expanded);
            simplex[N] = if fe < fr {
                (expanded, fe)
            } else {
                (reflected, fr)
            };
        } else if fr < simplex[N - 1].1 {
            simplex[N] = (reflected, fr);
        } else {
            let contracted = if fr < worst {
                towards(&centroid, &reflected, 0.5)
            } else {
                towards(&centroid, &worst_vertex, 0.5)
            };
            let fc = f(&contracted);
            if fc < worst.min(fr) {
                simplex[N] = (contracted, fc);
            } else {
                let best_vertex = simplex[0].0;
                for entry in &mut simplex[1..] {
                    let vertex = towards(&best_vertex, &entry.0, 0.5);
                    *entry = (vertex, f(&vertex));
                }
            }
        }
    }
    simplex.sort_by(|a, b| a.1.total_cmp(&b.1));
    simplex[0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_section_finds_a_parabola_minimum() {
        let (x, fx) = golden_section(|x| (x - 1.3).powi(2) + 2.0, -5.0, 5.0, 1e-9);
        assert!((x - 1.3).abs() < 1e-7, "{x}");
        assert!((fx - 2.0).abs() < 1e-12);
    }

    #[test]
    fn nelder_mead_finds_the_rosenbrock_minimum() {
        let rosenbrock = |p: &[f64; 2]| (1.0 - p[0]).powi(2) + 100.0 * (p[1] - p[0] * p[0]).powi(2);
        let (best, value) = nelder_mead(rosenbrock, [-1.2, 1.0], [0.5, 0.5], 2_000, 1e-14);
        assert!(
            (best[0] - 1.0).abs() < 1e-3 && (best[1] - 1.0).abs() < 1e-3,
            "{best:?}"
        );
        assert!(value < 1e-8);
    }
}
