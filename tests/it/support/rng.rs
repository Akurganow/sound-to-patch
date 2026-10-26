//! SplitMix64 noise, bit for bit the generator of `xtask/golden/generate.py`,
//! so a noise recipe gives the same signal in both languages.

const GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;

/// The first `count` outputs of SplitMix64 started at `seed`.
pub(crate) fn splitmix_u64(seed: u64, count: usize) -> Vec<u64> {
    let mut state = seed;
    (0..count)
        .map(|_| {
            state = state.wrapping_add(GAMMA);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        })
        .collect()
}

/// The top 53 bits of an output as an exact `f64`.
fn top53(z: u64) -> f64 {
    let high = u32::try_from(z >> 32).unwrap();
    let low = u32::try_from((z >> 11) & 0x1F_FFFF).unwrap();
    f64::from(high) * 2.0_f64.powi(21) + f64::from(low)
}

/// Uniform values in `[0, 1)`: the top 53 bits of each output.
pub(crate) fn uniform(seed: u64, count: usize) -> Vec<f64> {
    splitmix_u64(seed, count)
        .into_iter()
        .map(|z| top53(z) * 2.0_f64.powi(-53))
        .collect()
}

/// Standard normal values by Box-Muller, two per pair of uniforms.
pub(crate) fn gaussian(seed: u64, count: usize) -> Vec<f64> {
    let u = uniform(seed, 2 * count.div_ceil(2));
    let mut out = Vec::with_capacity(u.len());
    for [first, second] in u.as_chunks::<2>().0 {
        let radius = (-2.0 * (-first).ln_1p()).sqrt();
        let angle = 2.0 * std::f64::consts::PI * second;
        out.push(radius * angle.cos());
        out.push(radius * angle.sin());
    }
    out.truncate(count);
    out
}

#[test]
fn splitmix_matches_the_reference_sequence() {
    // The first outputs for seed 0, as published with the algorithm.
    assert_eq!(
        splitmix_u64(0, 3),
        vec![
            0xE220_A839_7B1D_CDAF,
            0x6E78_9E6A_A1B9_65F4,
            0x06C4_5D18_8009_454F
        ]
    );
}

#[test]
fn uniform_takes_the_top_53_bits() {
    assert_eq!(top53(u64::MAX), 2.0_f64.powi(53) - 1.0);
    assert_eq!(top53(1 << 11), 1.0);
    assert!(uniform(7, 1000).iter().all(|u| (0.0..1.0).contains(u)));
}
