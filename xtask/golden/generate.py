"""Golden numbers for the signal tests of sound-to-patch.

Run it through `cargo xtask golden`, which installs the hash-locked tools
from requirements.txt into target/golden-venv and calls this script with
the repository root. It writes every vector under tests/fixtures/golden/.

Each signal comes from a recipe that the Rust tests rebuild with their own
code. Noise comes from SplitMix64, which both sides implement, so a noise
signal is the same in Python and in Rust up to the last bits of `log` and
`cos`. The reference values come from librosa and SciPy, or from a closed
form, never from the Rust code. The prototype tables under prototype/
record figures that the task issues quote; they are NumPy computations of
the issue's method on the recipe given here.

A second run with the same pins writes byte-identical files.
"""

import json
import math
import sys
from pathlib import Path

import librosa
import mir_eval
import numpy as np
import scipy
import scipy.optimize
import scipy.signal

GENERATOR = "xtask/golden/generate.py"
TOOLS = {
    "python": ".".join(str(part) for part in sys.version_info[:3]),
    "numpy": np.__version__,
    "scipy": scipy.__version__,
    "librosa": librosa.__version__,
    "mir-eval": mir_eval.__version__,
}
LN_1000 = math.log(1000.0)

# SplitMix64 constants, as in the Rust support module.
GAMMA = np.uint64(0x9E3779B97F4A7C15)
MIX1 = np.uint64(0xBF58476D1CE4E5B9)
MIX2 = np.uint64(0x94D049BB133111EB)


def splitmix_u64(seed, count):
    """The first `count` outputs of SplitMix64 started at `seed`."""
    index = np.arange(1, count + 1, dtype=np.uint64)
    with np.errstate(over="ignore"):
        z = np.uint64(seed) + index * GAMMA
        z = (z ^ (z >> np.uint64(30))) * MIX1
        z = (z ^ (z >> np.uint64(27))) * MIX2
    return z ^ (z >> np.uint64(31))


def uniform(seed, count):
    """Uniform values in [0, 1): the top 53 bits of each output."""
    return (splitmix_u64(seed, count) >> np.uint64(11)).astype(np.float64) * 2.0**-53


def gaussian(seed, count):
    """Standard normal values by Box-Muller, two per pair of uniforms."""
    pairs = (count + 1) // 2
    u = uniform(seed, 2 * pairs)
    radius = np.sqrt(-2.0 * np.log1p(-u[0::2]))
    angle = 2.0 * np.pi * u[1::2]
    out = np.empty(2 * pairs)
    out[0::2] = radius * np.cos(angle)
    out[1::2] = radius * np.sin(angle)
    return out[:count]


def times(rate, length):
    return np.arange(length, dtype=np.float64) / rate


def tones(recipe):
    """A sum of sines plus optional seeded Gaussian noise."""
    t = times(recipe["sample_rate"], recipe["length"])
    x = np.zeros(recipe["length"])
    for tone in recipe["tones"]:
        x += tone["amplitude"] * np.sin(2.0 * np.pi * tone["frequency_hz"] * t + tone["phase"])
    noise = recipe.get("noise")
    if noise is not None:
        x += noise["std"] * gaussian(noise["seed"], recipe["length"])
    return x


def bandlimited_saw(f0, rate, t, phase_shift=None):
    """Harmonic h at amplitude 1/h up to the Nyquist frequency."""
    x = np.zeros_like(t)
    h = 1
    while h * f0 < rate / 2:
        shift = 0.0 if phase_shift is None else phase_shift(h)
        x += np.sin(2.0 * np.pi * h * f0 * t - shift) / h
        h += 1
    return x


def write(root, name, document):
    path = root / "tests" / "fixtures" / "golden" / f"{name}.json"
    path.parent.mkdir(parents=True, exist_ok=True)
    text = json.dumps(document, indent=1, sort_keys=True, allow_nan=False) + "\n"
    path.write_text(text, encoding="utf-8")


def document(name, issue, description, function, tools, recipe, values, tolerance, **extra):
    doc = {
        "name": name,
        "issue": issue,
        "description": description,
        "generator": f"{GENERATOR}:{function}",
        "tools": {tool: TOOLS[tool] for tool in tools},
        "recipe": recipe,
        "values": values,
        "tolerance": tolerance,
    }
    doc.update(extra)
    return doc


def floats(array):
    return [float(v) for v in np.asarray(array).ravel()]


def window_vectors():
    sizes = [16, 63, 64, 1024]
    values = {}
    for kind, name in [("hann", "hann"), ("blackmanharris", "blackman_harris")]:
        values[name] = {str(n): floats(scipy.signal.get_window(kind, n, fftbins=True)) for n in sizes}
    return document(
        "window",
        48,
        "Periodic Hann and four-term Blackman-Harris windows.",
        "window_vectors",
        ["python", "scipy"],
        {"kind": "window", "sizes": sizes, "periodic": True},
        values,
        {"abs": 1e-12, "rel": 0.0},
    )


STFT_SIGNAL = {
    "kind": "tones",
    "sample_rate": 8000,
    "length": 1000,
    "tones": [
        {"frequency_hz": 440.0, "amplitude": 0.5, "phase": 0.3},
        {"frequency_hz": 1234.5, "amplitude": 0.25, "phase": 1.1},
    ],
    "noise": {"seed": 48, "std": 0.01},
}


def stft_vectors():
    x = tones(STFT_SIGNAL)
    cases = []
    for n_fft, hop, pad_mode in [(256, 64, "constant"), (128, 50, "reflect")]:
        spectrum = librosa.stft(x, n_fft=n_fft, hop_length=hop, window="hann", center=True, pad_mode=pad_mode)
        cases.append(
            {
                "frame_len": n_fft,
                "hop": hop,
                "padding": "zero" if pad_mode == "constant" else "reflect",
                "frames": int(spectrum.shape[1]),
                "bins": int(spectrum.shape[0]),
                # Frame-major: element [m][k] is frame m, bin k.
                "re": [floats(spectrum[:, m].real) for m in range(spectrum.shape[1])],
                "im": [floats(spectrum[:, m].imag) for m in range(spectrum.shape[1])],
            }
        )
    return document(
        "stft",
        48,
        "Centred STFT frames with a periodic Hann window, unnormalised.",
        "stft_vectors",
        ["python", "numpy", "librosa"],
        {"signal": STFT_SIGNAL, "window": "hann", "center": True},
        {"cases": cases},
        {"abs": 1e-10, "rel": 1e-12},
    )


HILBERT_SIGNAL = {
    "kind": "tones",
    "sample_rate": 8000,
    "length": 2000,
    "tones": [
        {"frequency_hz": 500.0, "amplitude": 1.0, "phase": 0.2},
        {"frequency_hz": 493.0, "amplitude": 0.25, "phase": 0.0},
        {"frequency_hz": 507.0, "amplitude": 0.25, "phase": 0.4},
    ],
}


def hilbert_vectors():
    pad = 256
    x = tones(HILBERT_SIGNAL)
    z = scipy.signal.hilbert(np.pad(x, pad))[pad:-pad]
    return document(
        "hilbert",
        49,
        "Analytic signal of a zero-padded AM tone, padding cut after the transform.",
        "hilbert_vectors",
        ["python", "numpy", "scipy"],
        {"signal": HILBERT_SIGNAL, "pad": pad},
        {"re": floats(z.real), "im": floats(z.imag), "envelope": floats(np.abs(z))},
        {"abs": 1e-10, "rel": 1e-12},
    )


def decay_closed_forms():
    rt60 = [0.4, 1.2, 2.5]
    taus = [0.05, 0.3, 1.0]
    return document(
        "decay",
        53,
        "Closed forms: an amplitude decay exp(-ln(1000) t / RT60) falls 60 dB per RT60, "
        "and a release exp(-t / tau) has RT60 = ln(1000) tau, about 6.9 tau.",
        "decay_closed_forms",
        ["python"],
        {"kind": "closed_form"},
        {
            "slope_db_per_s": [{"rt60_s": r, "slope": -60.0 / r} for r in rt60],
            "release_rt60_s": [{"tau_s": tau, "rt60": LN_1000 * tau} for tau in taus],
        },
        {"abs": 0.0, "rel": 1e-12},
    )


def parabolic_peak(magnitude, k):
    a, b, c = np.log(magnitude[k - 1 : k + 2])
    return k + 0.5 * (a - c) / (a - 2.0 * b + c)


def estimate_frequency(x, rate, window, oversample):
    n = len(x)
    spectrum = np.abs(np.fft.rfft(x * window, n * oversample))
    k = int(np.argmax(spectrum[1:-1])) + 1
    return parabolic_peak(spectrum, k) * rate / (n * oversample)


PEAK_RECIPE = {
    "frame_len": 8192,
    "sample_rate": 44100,
    "trials": 300,
    "seed": 4800,
    "frequency_hz": "100 Hz plus a uniform offset of up to half a bin of 8192, one draw per trial",
    "phase": "uniform in [0, 2 pi), one draw per trial",
    "amplitude": 1.0,
    "snr_db": 30.0,
    "snr_definition": "A^2 / (2 sigma^2)",
    "oversample": 16,
    "draw_order": "uniform(seed, 2 * trials) gives offset then phase per trial; "
    "the noise of trial i is gaussian(seed + 1 + i, frame_len)",
}


def peak_accuracy():
    r = PEAK_RECIPE
    n, rate, trials = r["frame_len"], r["sample_rate"], r["trials"]
    sigma = math.sqrt(r["amplitude"] ** 2 / (2.0 * 10 ** (r["snr_db"] / 10.0)))
    draws = uniform(r["seed"], 2 * trials)
    t = np.arange(n) / rate
    bin_hz = rate / n
    windows = {
        "hann": scipy.signal.get_window("hann", n, fftbins=True),
        "rectangular": np.ones(n),
    }
    errors = {name: [] for name in windows}
    for i in range(trials):
        frequency = 100.0 + (draws[2 * i] - 0.5) * bin_hz
        phase = 2.0 * np.pi * draws[2 * i + 1]
        x = r["amplitude"] * np.sin(2.0 * np.pi * frequency * t + phase)
        x = x + sigma * gaussian(r["seed"] + 1 + i, n)
        for name, window in windows.items():
            errors[name].append(estimate_frequency(x, rate, window, r["oversample"]) - frequency)
    eta = r["amplitude"] ** 2 / sigma**2
    bound_cycles = math.sqrt(12.0 / ((2.0 * math.pi) ** 2 * eta * n * (n * n - 1)))
    values = {
        "std_hz": {name: float(np.std(e)) for name, e in errors.items()},
        "bound_std_hz": bound_cycles * rate,
    }
    # Noiseless bias over 101 frequencies across one bin.
    t = np.arange(n) / rate
    offsets = np.linspace(-0.5, 0.5, 101)
    bias = {}
    for name, kind, oversample in [
        ("hann_no_padding_bins", "hann", 1),
        ("hann_4x_padding_hz", "hann", 4),
        ("blackman_harris_no_padding_bins", "blackmanharris", 1),
    ]:
        window = scipy.signal.get_window(kind, n, fftbins=True)
        worst = 0.0
        for offset in offsets:
            frequency = 100.0 + offset * bin_hz
            x = np.sin(2.0 * np.pi * frequency * t + 0.7)
            worst = max(worst, abs(estimate_frequency(x, rate, window, oversample) - frequency))
        bias[name] = worst / bin_hz if name.endswith("_bins") else worst
    values["max_noiseless_bias"] = bias
    return document(
        "48-peak-accuracy",
        48,
        "Accuracy of the parabolic log-magnitude peak picker on one noisy sine.",
        "peak_accuracy",
        ["python", "numpy", "scipy"],
        r,
        values,
        {"abs": 0.0, "rel": 0.02},
        tag="[own, synthetic]",
        rounding="the issue prints mHz to two decimals and bins to three",
        issue_figures={
            "hann_std_mhz": 1.55,
            "bound_mhz": 0.73,
            "rectangular_std_mhz": 21.7,
            "hann_no_padding_bias_bins": 0.016,
            "hann_4x_padding_bias_hz": 0.001,
            "blackman_harris_no_padding_bias_bins": 0.003,
        },
    )


def schroeder(energy):
    return np.cumsum(energy[::-1])[::-1]


def plain_decay_fit(x, rate, top, bottom):
    """T20 or T30 from the plain Schroeder integral, no floor handling."""
    curve = schroeder(x * x)
    level = 10.0 * np.log10(curve / curve[0])
    inside = (level <= top) & (level >= bottom)
    if inside.sum() < 2:
        return None
    t = np.arange(len(x))[inside] / rate
    slope = np.polyfit(t, level[inside], 1)[0]
    return -60.0 / slope if slope < 0 else None


def decay_ir(rt60, length, rate, seed):
    t = times(rate, length)
    return gaussian(seed, length) * np.exp(-LN_1000 * t / rt60)


DECAY_FLOOR_RECIPE = {
    "sample_rate": 16000,
    "rt60_s": [0.4, 1.2, 2.5],
    "floors_db": [None, -60.0, -40.0, -30.0],
    "trials": 15,
    "length": "2 RT60",
    "impulse_response": "gaussian(seed, n) * exp(-ln(1000) t / RT60), direct power 1",
    "floor": "independent gaussian noise of power 10^(floor / 10)",
    "seeds": "trial i of cell (r, f) uses seed 5300 + 100 r + 10 f + i for the response "
    "and that seed + 50000 for the floor",
    "method": "plain Schroeder integral to the end, least-squares line between -5 and -25 dB (T20) "
    "or -5 and -35 dB (T30), RT = -60 / slope; median over trials",
}


def decay_noise_floor():
    r = DECAY_FLOOR_RECIPE
    rate = r["sample_rate"]
    rows = []
    for ri, rt60 in enumerate(r["rt60_s"]):
        row = {"rt60_s": rt60, "cells": []}
        for fi, floor in enumerate(r["floors_db"]):
            length = int(round(2 * rt60 * rate))
            t20s, t30s = [], []
            for trial in range(r["trials"]):
                seed = 5300 + 100 * ri + 10 * fi + trial
                x = decay_ir(rt60, length, rate, seed)
                if floor is not None:
                    x = x + math.sqrt(10 ** (floor / 10.0)) * gaussian(seed + 50000, length)
                t20s.append(plain_decay_fit(x, rate, -5.0, -25.0))
                t30s.append(plain_decay_fit(x, rate, -5.0, -35.0))
            row["cells"].append(
                {
                    "floor_db": floor,
                    "t20_s": median_or_none(t20s),
                    "t30_s": median_or_none(t30s),
                }
            )
        rows.append(row)
    return document(
        "53-noise-floor",
        53,
        "Bias of plain T20 and T30 fits from a noise floor under the decay.",
        "decay_noise_floor",
        ["python", "numpy"],
        r,
        {"rows": rows},
        {"abs": 0.0, "rel": 0.05},
        tag="[own, synthetic]",
        rounding="the issue prints seconds to two decimals",
        issue_figures=issue_figures_53_noise_floor(rows),
    )


def within(ours, figure, rel, abs_):
    """Whether our value matches an issue figure: both None, or within tolerance."""
    if ours is None or figure is None:
        return ours is None and figure is None
    return abs(ours - figure) <= abs_ + rel * abs(figure)


# The figures that #53 prints, T20 / T30 in seconds per floor: none, -60,
# -40 and -30 dB.
ISSUE_53_NOISE_FLOOR = {
    0.4: [(0.41, 0.40), (0.39, 0.40), (0.48, 3.27), (5.14, 4.38)],
    1.2: [(1.19, 1.20), (1.21, 1.21), (1.39, 5.82), (9.58, 8.48)],
    2.5: [(2.51, 2.50), (2.51, 2.51), (2.72, 6.23), (10.05, 10.02)],
}


def issue_figures_53_noise_floor(rows):
    cells = []
    for row in rows:
        for cell, (t20, t30) in zip(row["cells"], ISSUE_53_NOISE_FLOOR[row["rt60_s"]]):
            reproduced = within(cell["t20_s"], t20, 0.05, 0.005) and within(cell["t30_s"], t30, 0.05, 0.005)
            cells.append(
                {
                    "rt60_s": row["rt60_s"],
                    "floor_db": cell["floor_db"],
                    "issue_t20_s": t20,
                    "issue_t30_s": t30,
                    "reproduced": reproduced,
                }
            )
    return {
        "check": "5 % plus 0.005 s for the issue's rounding, on both T20 and T30",
        "cells": cells,
    }


def median_or_none(values):
    kept = [v for v in values if v is not None]
    return float(np.median(kept)) if len(kept) == len(values) else None


def note_with_reverb(release_tau, reverb_rt60, rate, seed):
    """Gated noise note, exponential release, fully wet noise reverb."""
    gate = 0.5
    tail = max(6.0 * release_tau, 3.0 * max(reverb_rt60, LN_1000 * release_tau))
    length = int(round((gate + tail) * rate))
    t = times(rate, length)
    envelope = np.where(t < gate, 1.0, np.exp(-(t - gate) / release_tau))
    x = gaussian(seed, length) * envelope
    if reverb_rt60 > 0:
        ir_length = int(round(1.5 * reverb_rt60 * rate))
        ir = decay_ir(reverb_rt60, ir_length, rate, seed + 70000)
        ir /= math.sqrt(np.sum(ir * ir))
        x = scipy.signal.fftconvolve(x, ir)[:length]
    return x[int(round(gate * rate)) :]


RELEASE_RECIPE = {
    "sample_rate": 16000,
    "release_tau_s": [0.05, 0.3, 1.0],
    "reverb_rt60_s": [0.0, 0.5, 1.0, 2.0],
    "trials": 15,
    "note": "gaussian(seed, n) gated for 0.5 s, then amplitude exp(-(t - 0.5) / tau)",
    "reverb": "fully wet: the note convolved with gaussian(seed + 70000, n) * exp(-ln(1000) t / R), "
    "1.5 R long, scaled to unit energy; R = 0 means no reverb",
    "length": "0.5 s gate plus 3 times the slower of R and ln(1000) tau, at least 6 tau",
    "seeds": "trial i of cell (a, b) uses seed 5310 + 1000 a + 100 b + i",
    "method": "plain Schroeder integral of the tail from note-off, -5 to -25 dB line, "
    "RT = -60 / slope; median, 10th and 90th percentile over trials",
}


def decay_release_reverb():
    r = RELEASE_RECIPE
    rate = r["sample_rate"]
    rows = []
    for ai, tau in enumerate(r["release_tau_s"]):
        row = {"release_tau_s": tau, "equivalent_rt60_s": LN_1000 * tau, "cells": []}
        for bi, reverb in enumerate(r["reverb_rt60_s"]):
            fits = []
            for trial in range(r["trials"]):
                tail = note_with_reverb(tau, reverb, rate, 5310 + 1000 * ai + 100 * bi + trial)
                fits.append(plain_decay_fit(tail, rate, -5.0, -25.0))
            fits = np.array(fits, dtype=float)
            row["cells"].append(
                {
                    "reverb_rt60_s": reverb,
                    "median_s": float(np.median(fits)),
                    "p10_s": float(np.percentile(fits, 10)),
                    "p90_s": float(np.percentile(fits, 90)),
                }
            )
        rows.append(row)
    return document(
        "53-release-reverb",
        53,
        "The -5 to -25 dB slope after note-off follows the slower of release and reverb.",
        "decay_release_reverb",
        ["python", "numpy", "scipy"],
        r,
        {"rows": rows},
        {"abs": 0.0, "rel": 0.05},
        tag="[own, synthetic]",
        rounding="the issue prints seconds to two decimals",
    )


DROP_RECIPE = {
    "sample_rate": 16000,
    "release_tau_s": 0.05,
    "reverb_rt60_s": [0.5, 1.0, 2.0],
    "trials": 15,
    "signal": "as in 53-release-reverb",
    "seeds": "trial i of reverb b uses seed 5390 + 100 b + i",
    "method": "time from note-off until the 10 ms RMS of the raw tail first falls 30 dB below "
    "the RMS of its first 10 ms; minimum and maximum over reverbs and trials",
}


def decay_drop_time():
    r = DROP_RECIPE
    rate = r["sample_rate"]
    block = int(round(0.01 * rate))
    drops = {30.0: [], 15.0: []}
    for bi, reverb in enumerate(r["reverb_rt60_s"]):
        for trial in range(r["trials"]):
            tail = note_with_reverb(r["release_tau_s"], reverb, rate, 5390 + 100 * bi + trial)
            usable = len(tail) // block * block
            rms = np.sqrt(np.mean((tail[:usable] ** 2).reshape(-1, block), axis=1))
            for drop, found in drops.items():
                below = np.nonzero(rms <= rms[0] * 10 ** (-drop / 20.0))[0]
                found.append(float(below[0] * block / rate))
    ours = {"min_s": min(drops[30.0]), "max_s": max(drops[30.0])}
    issue = {"min_s": 0.141, "max_s": 0.554}
    return document(
        "53-drop-time",
        53,
        "A fixed 30 dB drop on the raw envelope misreads a 50 ms release under reverb.",
        "decay_drop_time",
        ["python", "numpy", "scipy"],
        r,
        ours,
        {"abs": 0.0, "rel": 0.1},
        tag="[own, synthetic]",
        rounding="the issue prints 141 to 554 ms",
        issue_figures={
            "check": "10 % on both ends",
            "issue": issue,
            "reproduced": all(within(ours[k], issue[k], 0.1, 0.0) for k in issue),
            "same_method_at_15_db": {"min_s": min(drops[15.0]), "max_s": max(drops[15.0])},
        },
    )


STEREO_RECIPE = {
    "sample_rate": 44100,
    "duration_s": 1.5,
    "f0_hz": 220.0,
    "source": "band-limited sawtooth: harmonic h at amplitude 1/h below the Nyquist frequency, phase 0",
    "rows": {
        "mono": "L = R = saw",
        "delay_0_4_ms": "R is L delayed by 0.4 ms, exactly, per harmonic phase",
        "haas_10_ms": "R is L delayed by 10 ms, exactly, per harmonic phase",
        "velvet_20_ms": "each channel is the saw convolved with its own velvet noise: one impulse per "
        "1 ms cell over 20 ms at a uniform position, sign + or - with equal odds; "
        "L draws uniform(4710, 40), R uniform(4711, 40): position then sign per cell",
        "unison": "voices detuned evenly across -25 to +25 cents, phase 2 pi uniform(4720 + v, 1) for "
        "voice v; voices alternate left and right with rank, pan p = width * rank / max rank, "
        "constant-power gains L = cos(pi/4 (1 + p)), R = sin(pi/4 (1 + p))",
        "chorus": "each channel is saw + saw delayed by 7 ms + 2 ms sin(2 pi 0.8 t + phi), phi = 0 on L "
        "and pi on R, per harmonic phase",
        "reverb": "each channel is (1 - wet) saw + wet (saw convolved with its own unit-energy noise "
        "reverb of RT60 1 s and 1.5 s length, seeds 4730 for L and 4731 for R)",
    },
    "rho": "Pearson correlation of L and R at zero lag",
    "sm_db": "20 log10(rms(S) / rms(M)), M = (L + R) / 2, S = (L - R) / 2; None when either RMS is 0",
    "ipd_deg": "8192-point centred Hann STFT, hop 2048; per frame, at the bin of the largest |L conj R| "
    "within 2 bins of h f0 for h = 1..20, abs(angle(L conj R)) in degrees, averaged with weights "
    "|L conj R|; then the mean over frames whose RMS over both channels, sqrt((rms_L^2 + rms_R^2) / 2), "
    "is within 40 dB of the loudest",
    "channel_scaling": "none; rows compare the raw channels",
}


def saw_channel(t, rate, f0, delay=None):
    if delay is None:
        return bandlimited_saw(f0, rate, t)
    return bandlimited_saw(f0, rate, t, lambda h: 2.0 * np.pi * h * f0 * delay)


def time_varying_delay_saw(t, rate, f0, delay):
    x = np.zeros_like(t)
    h = 1
    while h * f0 < rate / 2:
        x += np.sin(2.0 * np.pi * h * f0 * (t - delay)) / h
        h += 1
    return x


def velvet(seed, rate):
    cells = 20
    cell = rate // 1000
    draws = uniform(seed, 2 * cells)
    v = np.zeros(cells * cell)
    for c in range(cells):
        position = c * cell + int(draws[2 * c] * cell)
        v[position] = 1.0 if draws[2 * c + 1] < 0.5 else -1.0
    return v


def stereo_pair(row, t, rate, f0):
    saw = bandlimited_saw(f0, rate, t)
    n = len(t)
    if row == "mono":
        return saw, saw.copy()
    if row == "delay_0_4_ms":
        return saw, saw_channel(t, rate, f0, 0.0004)
    if row == "haas_10_ms":
        return saw, saw_channel(t, rate, f0, 0.010)
    if row == "velvet_20_ms":
        return (
            scipy.signal.fftconvolve(saw, velvet(4710, rate))[:n],
            scipy.signal.fftconvolve(saw, velvet(4711, rate))[:n],
        )
    if row.startswith("unison"):
        _, voices, width = row.split("_")
        voices, width = int(voices), float(width)
        left, right = np.zeros(n), np.zeros(n)
        detunes = np.linspace(-25.0, 25.0, voices)
        order = np.argsort(np.abs(detunes), kind="stable")
        max_rank = (voices - 1) / 2
        for rank_index, v in enumerate(order):
            rank = (rank_index + 1) // 2
            side = 1.0 if rank_index % 2 == 1 else -1.0
            pan = side * width * rank / max_rank
            phase = 2.0 * np.pi * uniform(4720 + int(v), 1)[0]
            f = f0 * 2.0 ** (detunes[v] / 1200.0)
            voice = bandlimited_saw(f, rate, t, lambda h: -phase * h)
            left += math.cos(math.pi / 4.0 * (1.0 + pan)) * voice
            right += math.sin(math.pi / 4.0 * (1.0 + pan)) * voice
        return left, right
    if row == "chorus":
        def channel(phi):
            delay = 0.007 + 0.002 * np.sin(2.0 * np.pi * 0.8 * t + phi)
            return saw + time_varying_delay_saw(t, rate, f0, delay)
        return channel(0.0), channel(np.pi)
    if row.startswith("reverb"):
        wet = float(row.split("_")[1])
        def channel(seed):
            ir = decay_ir(1.0, int(1.5 * rate), rate, seed)
            ir /= math.sqrt(np.sum(ir * ir))
            return (1.0 - wet) * saw + wet * scipy.signal.fftconvolve(saw, ir)[:n]
        return channel(4730), channel(4731)
    raise ValueError(row)


# The figures that #47 prints: rho, sm_db, ipd_deg per row.
ISSUE_47_STEREO = {
    "mono": (1.000, None, 0.0),
    "delay_0_4_ms": (0.538, -5.2, 53.0),
    "haas_10_ms": (0.040, -0.3, 88.0),
    "velvet_20_ms": (0.572, -5.1, 41.0),
    "unison_3_0.5": (0.862, -10.9, 0.7),
    "unison_3_1.0": (0.479, -4.4, 2.5),
    "unison_7_0.5": (0.886, -12.2, 0.3),
    "unison_7_1.0": (0.588, -5.9, 0.7),
    "chorus": (0.819, -10.0, 3.7),
    "reverb_0.3": (0.962, -15.9, 8.0),
    "reverb_1.0": (0.518, -4.7, 42.0),
}


def issue_figures_47_stereo(values):
    rows = {}
    for row, (rho, sm_db, ipd) in ISSUE_47_STEREO.items():
        ours = values[row]
        rows[row] = {
            "issue": {"rho": rho, "sm_db": sm_db, "ipd_deg": ipd},
            "reproduced": within(ours["rho"], rho, 0.1, 0.0005)
            and within(ours["sm_db"], sm_db, 0.1, 0.05)
            and within(ours["ipd_deg"], ipd, 0.0, max(2.0, 0.1 * ipd)),
        }
    return {
        "check": "10 % on rho and sm_db plus half the issue's last digit; 10 % or 2 degrees on ipd_deg",
        "note": "the issue gives no recipe for the velvet, chorus and reverb rows and asks only for "
        "their order of magnitude; the recipe here defines them",
        "rows": rows,
    }


def stereo_stats(left, right, rate, f0):
    if np.all(left == 0) or np.all(right == 0):
        rho = None
    else:
        rho = float(np.corrcoef(left, right)[0, 1])
    mid, side = (left + right) / 2.0, (left - right) / 2.0
    rms_m, rms_s = np.sqrt(np.mean(mid**2)), np.sqrt(np.mean(side**2))
    sm_db = None if rms_m == 0 or rms_s == 0 else float(20.0 * np.log10(rms_s / rms_m))
    n_fft = 8192
    spec_l = librosa.stft(left, n_fft=n_fft, hop_length=n_fft // 4, window="hann", center=True, pad_mode="constant")
    spec_r = librosa.stft(right, n_fft=n_fft, hop_length=n_fft // 4, window="hann", center=True, pad_mode="constant")
    def rms(x):
        frames = librosa.util.frame(np.pad(x, n_fft // 2), frame_length=n_fft, hop_length=n_fft // 4)
        return np.sqrt(np.mean(frames**2, axis=0))
    frame_rms = np.sqrt((rms(left) ** 2 + rms(right) ** 2) / 2.0)
    active = (frame_rms > 0) & (frame_rms >= frame_rms.max() * 10 ** (-40.0 / 20.0))
    bin_hz = rate / n_fft
    per_frame = []
    for m in range(spec_l.shape[1]):
        if not active[m]:
            continue
        cross = spec_l[:, m] * np.conj(spec_r[:, m])
        weights, angles = [], []
        for h in range(1, 21):
            centre = int(round(h * f0 / bin_hz))
            lo, hi = centre - 2, centre + 2
            k = lo + int(np.argmax(np.abs(cross[lo : hi + 1])))
            weights.append(abs(cross[k]))
            angles.append(abs(np.degrees(np.angle(cross[k]))))
        weights = np.array(weights)
        if weights.sum() > 0:
            per_frame.append(float(np.sum(weights * np.array(angles)) / weights.sum()))
    return {"rho": rho, "sm_db": sm_db, "ipd_deg": float(np.mean(per_frame))}


def stereo_screen():
    r = STEREO_RECIPE
    rate, f0 = r["sample_rate"], r["f0_hz"]
    t = times(rate, int(round(r["duration_s"] * rate)))
    rows = [
        "mono",
        "delay_0_4_ms",
        "haas_10_ms",
        "velvet_20_ms",
        "unison_3_0.5",
        "unison_3_1.0",
        "unison_7_0.5",
        "unison_7_1.0",
        "chorus",
        "reverb_0.3",
        "reverb_1.0",
    ]
    values = {}
    for row in rows:
        left, right = stereo_pair(row, t, rate, f0)
        values[row] = stereo_stats(left, right, rate, f0)
    return document(
        "47-stereo-screen",
        47,
        "Stereo screen of a 220 Hz sawtooth under delay, decorrelation, unison, chorus and reverb.",
        "stereo_screen",
        ["python", "numpy", "scipy", "librosa"],
        r,
        values,
        {"abs": 2.0, "rel": 0.1},
        tag="[own, synthetic]",
        rounding="the issue prints rho to three decimals, dB to one and degrees to one",
        tolerance_note="10 % or 2 degrees, whichever is larger, as the issue states",
        issue_figures=issue_figures_47_stereo(values),
    )


TWO_SAW_RECIPE = {
    "sample_rate": 48000,
    "duration_s": 8.0,
    "f0_hz": 55.0,
    "detune_cents": 10.0,
    "source": "sum of two naive sawtooths 2 (f t mod 1) - 1 at f0 and f0 2^(10/1200), phase 0",
    "harmonics": [1, 2, 5],
    "band": "fourth-order Butterworth band-pass of +/- 15 Hz around h f0, run forward and backward",
    "envelope": "magnitude of the analytic signal of the band",
    "track": "block means of 240 samples, a 200 Hz track",
    "spectrum": "linear detrend, periodic Hann, 8x zero padding, magnitude FFT; peak bin between 0.2 and 20 Hz, "
    "no interpolation",
}


def two_saw_peaks():
    r = TWO_SAW_RECIPE
    rate = r["sample_rate"]
    t = times(rate, int(round(r["duration_s"] * rate)))
    f1 = r["f0_hz"]
    f2 = f1 * 2.0 ** (r["detune_cents"] / 1200.0)
    x = (2.0 * np.mod(f1 * t, 1.0) - 1.0) + (2.0 * np.mod(f2 * t, 1.0) - 1.0)
    peaks = {}
    for h in r["harmonics"]:
        sos = scipy.signal.butter(4, [h * f1 - 15.0, h * f1 + 15.0], btype="bandpass", fs=rate, output="sos")
        band = scipy.signal.sosfiltfilt(sos, x)
        envelope = np.abs(scipy.signal.hilbert(band))
        block = 240
        track = envelope[: len(envelope) // block * block].reshape(-1, block).mean(axis=1)
        track = scipy.signal.detrend(track)
        track = track * scipy.signal.get_window("hann", len(track), fftbins=True)
        n_fft = 8 * len(track)
        spectrum = np.abs(np.fft.rfft(track, n_fft))
        freqs = np.fft.rfftfreq(n_fft, d=1.0 / 200.0)
        band_bins = (freqs >= 0.2) & (freqs <= 20.0)
        k = np.nonzero(band_bins)[0][np.argmax(spectrum[band_bins])]
        peaks[str(h)] = {"peak_hz": float(freqs[k]), "predicted_hz": float(h * (f2 - f1))}
    return document(
        "51-two-saw-peaks",
        51,
        "Beat peaks of two detuned sawtooths in band envelopes of harmonics 1, 2 and 5.",
        "two_saw_peaks",
        ["python", "numpy", "scipy"],
        r,
        peaks,
        {"abs": 0.125, "rel": 0.0},
        tag="[own, synthetic]",
        rounding="the issue prints Hz to two decimals",
        tolerance_note="the rate rule of the issue: within 1 / T, 0.125 Hz for 8 s",
    )


def spectral_frames(x, rate, n_fft):
    spectrum = np.abs(librosa.stft(x, n_fft=n_fft, hop_length=n_fft // 4, window="hann", center=True, pad_mode="constant"))
    window_sum = np.sum(scipy.signal.get_window("hann", n_fft, fftbins=True))
    return spectrum / window_sum, np.fft.rfftfreq(n_fft, d=1.0 / rate)


def centroid(weights, freqs):
    return float(np.sum(freqs * weights) / np.sum(weights))


def rolloff(weights, freqs, share):
    cumulative = np.cumsum(weights)
    return float(freqs[np.nonzero(cumulative >= share * cumulative[-1])[0][0]])


SAW_TABLE_RECIPE = {
    "sample_rate": 44100,
    "duration_s": 2.0,
    "f0_hz": [55.0, 220.0, 880.0],
    "source": "ideal sawtooth: harmonic h at amplitude 1/h below the Nyquist frequency, phase 0",
    "frames": "4096-point centred Hann STFT, hop 1024, magnitude divided by the window sum; "
    "frames that lie wholly inside the signal",
    "statistic": "median over those frames",
    "centroid": "sum f_k w_k / sum w_k with w = |X| (magnitude) or |X|^2 (power)",
    "rolloff": "frequency of the first bin whose cumulative weight reaches 85 % of the total",
}


def sawtooth_table():
    r = SAW_TABLE_RECIPE
    rate = r["sample_rate"]
    n_fft = 4096
    t = times(rate, int(round(r["duration_s"] * rate)))
    rows = {}
    for f0 in r["f0_hz"]:
        magnitude, freqs = spectral_frames(bandlimited_saw(f0, rate, t), rate, n_fft)
        hop = n_fft // 4
        inside = [m for m in range(magnitude.shape[1]) if m * hop - n_fft // 2 >= 0 and m * hop + n_fft // 2 <= len(t)]
        stats = {"centroid_magnitude_hz": [], "centroid_power_hz": [], "rolloff85_magnitude_hz": [], "rolloff85_power_hz": []}
        for m in inside:
            a = magnitude[:, m]
            stats["centroid_magnitude_hz"].append(centroid(a, freqs))
            stats["centroid_power_hz"].append(centroid(a * a, freqs))
            stats["rolloff85_magnitude_hz"].append(rolloff(a, freqs, 0.85))
            stats["rolloff85_power_hz"].append(rolloff(a * a, freqs, 0.85))
        harmonics = np.arange(1, int(math.ceil(rate / 2 / f0)))
        harmonics = harmonics[harmonics * f0 < rate / 2]
        rows[str(int(f0))] = {key: float(np.median(v)) for key, v in stats.items()}
        rows[str(int(f0))]["closed_form_centroid_magnitude_hz"] = float(f0 * len(harmonics) / np.sum(1.0 / harmonics))
        rows[str(int(f0))]["closed_form_centroid_power_hz"] = float(f0 * np.sum(1.0 / harmonics) / np.sum(1.0 / harmonics**2))
    return document(
        "54-sawtooth",
        54,
        "Centroid and rolloff of ideal sawtooths under magnitude and power weights.",
        "sawtooth_table",
        ["python", "numpy", "scipy", "librosa"],
        r,
        rows,
        {"abs": 1.0, "rel": 0.0},
        tag="[own, synthetic]",
        rounding="the issue prints whole hertz",
    )


NOISE_FLATNESS_RECIPE = {
    "sample_rate": 44100,
    "frames": 200,
    "frame_len": 4096,
    "seed": 5400,
    "signal": "gaussian(seed + m, 4096) for frame m, windowed by a periodic Hann",
    "scale": "magnitude divided by the window sum, then floored at 1e-5",
    "band": "bins from 100 Hz to 10 kHz inclusive",
    "flatness": "geometric over arithmetic mean of the weights, magnitude |X| or power |X|^2; "
    "mean over frames",
}


def noise_flatness():
    r = NOISE_FLATNESS_RECIPE
    n, rate = r["frame_len"], r["sample_rate"]
    window = scipy.signal.get_window("hann", n, fftbins=True)
    freqs = np.fft.rfftfreq(n, d=1.0 / rate)
    band = (freqs >= 100.0) & (freqs <= 10000.0)
    flat_mag, flat_pow = [], []
    for m in range(r["frames"]):
        a = np.abs(np.fft.rfft(gaussian(r["seed"] + m, n) * window)) / np.sum(window)
        a = np.maximum(a, 1e-5)[band]
        flat_mag.append(np.exp(np.mean(np.log(a))) / np.mean(a))
        p = a * a
        flat_pow.append(np.exp(np.mean(np.log(p))) / np.mean(p))
    return document(
        "54-noise-flatness",
        54,
        "Spectral flatness of white noise under magnitude and power weights.",
        "noise_flatness",
        ["python", "numpy", "scipy"],
        r,
        {
            "magnitude": float(np.mean(flat_mag)),
            "power": float(np.mean(flat_pow)),
            "power_closed_form": float(math.exp(-np.euler_gamma)),
        },
        {"abs": 0.01, "rel": 0.0},
        tag="[own, synthetic]",
        rounding="the issue prints three decimals",
    )


CREST_RECIPE = {
    "sample_rate": 44100,
    "duration_s": 1.0,
    "f0_hz": 110.0,
    "signals": {
        "sine": "sin(2 pi f t)",
        "saw": "naive ramp 2 (f t mod 1) - 1",
        "square": "sign of sin(2 pi f t), with sign(0) = 1",
        "tanh_sine": "tanh(4 sin(2 pi f t))",
        "tanh_saw": "tanh(4 (2 (f t mod 1) - 1))",
    },
    "crest": "20 log10(max |x| / rms(x))",
}


def crest_values():
    r = CREST_RECIPE
    rate = r["sample_rate"]
    t = times(rate, int(round(r["duration_s"] * rate)))
    f = r["f0_hz"]
    sine = np.sin(2.0 * np.pi * f * t)
    saw = 2.0 * np.mod(f * t, 1.0) - 1.0
    square = np.where(sine >= 0.0, 1.0, -1.0)
    signals = {
        "sine": sine,
        "saw": saw,
        "square": square,
        "tanh_sine": np.tanh(4.0 * sine),
        "tanh_saw": np.tanh(4.0 * saw),
    }
    values = {
        name: float(20.0 * np.log10(np.max(np.abs(x)) / np.sqrt(np.mean(x * x)))) for name, x in signals.items()
    }
    values["closed_forms_db"] = {
        "sine": 20.0 * math.log10(math.sqrt(2.0)),
        "saw": 20.0 * math.log10(math.sqrt(3.0)),
        "square": 0.0,
    }
    return document(
        "54-crest",
        54,
        "Crest factor of a 110 Hz tone, plain and saturated.",
        "crest_values",
        ["python", "numpy"],
        r,
        values,
        {"abs": 0.02, "rel": 0.0},
        tag="[own, synthetic]",
        rounding="the issue prints two decimals",
        tolerance_note="0.02 dB for the closed forms, 0.05 dB for the two tanh values",
    )


STEREO_DELAY_RECIPE = {
    "sample_rate": 44100,
    "length": 88200,
    "signal": "L = sin(2 pi f t), R = sin(2 pi f (t - delay))",
    "frequency_hz": 200.0,
    "delay_s": 0.001,
    "method": "the ipd_deg of 47-stereo-screen with f0 = frequency_hz",
}


def stereo_delay():
    r = STEREO_DELAY_RECIPE
    rate, f, delay = r["sample_rate"], r["frequency_hz"], r["delay_s"]
    t = times(rate, r["length"])
    left = np.sin(2.0 * np.pi * f * t)
    right = np.sin(2.0 * np.pi * f * (t - delay))
    stats = stereo_stats(left, right, rate, f)
    return document(
        "stereo",
        47,
        "The cross-spectrum phase of a pure delay through the screen's STFT, against the "
        "closed form 360 f delay.",
        "stereo_delay",
        ["python", "numpy", "librosa"],
        r,
        {
            "pure_delay": {
                "frequency_hz": f,
                "delay_s": delay,
                "ipd_deg": stats["ipd_deg"],
                "closed_form_ipd_deg": 360.0 * f * delay,
            }
        },
        {"abs": 0.05, "rel": 0.0},
    )


DESCRIPTOR_SIGNAL = {
    "kind": "tones",
    "sample_rate": 22050,
    "length": 11025,
    "tones": [
        {"frequency_hz": 220.0 * h, "amplitude": 1.0 / h, "phase": 0.1 * h}
        for h in range(1, 40)
        if 220.0 * h < 11025.0
    ],
    "noise": {"seed": 54, "std": 0.003},
}


def descriptor_vectors():
    r = DESCRIPTOR_SIGNAL
    rate = r["sample_rate"]
    n_fft = 1024
    x = tones(r)
    magnitude, freqs = spectral_frames(x, rate, n_fft)
    band = (freqs >= 100.0) & (freqs <= 10000.0)
    values = {
        "frame_len": n_fft,
        "centroid_hz": floats(librosa.feature.spectral_centroid(S=magnitude, sr=rate, n_fft=n_fft)[0]),
        "spread_hz": floats(librosa.feature.spectral_bandwidth(S=magnitude, sr=rate, n_fft=n_fft, p=2)[0]),
        "rolloff85_hz": floats(librosa.feature.spectral_rolloff(S=magnitude, sr=rate, n_fft=n_fft, roll_percent=0.85)[0]),
        "rolloff95_hz": floats(librosa.feature.spectral_rolloff(S=magnitude, sr=rate, n_fft=n_fft, roll_percent=0.95)[0]),
        "flatness": floats(librosa.feature.spectral_flatness(S=magnitude[band], power=1.0, amin=1e-5)[0]),
    }
    return document(
        "descriptors",
        54,
        "Per-frame centroid, spread, rolloff and flatness on magnitude weights.",
        "descriptor_vectors",
        ["python", "numpy", "scipy", "librosa"],
        {
            "signal": r,
            "frames": "1024-point centred Hann STFT, hop 256, zero padding, magnitude divided by the window sum",
            "flatness_band_hz": [100.0, 10000.0],
            "flatness_floor": 1e-5,
        },
        values,
        {"abs": 1e-6, "rel": 1e-9},
    )


LOMB_RECIPE = {
    "track_rate_hz": 200.0,
    "duration_s": 8.0,
    "signal": "1.5 + sin(2 pi 4 t + 0.4), kept while (t mod 1) < 0.5",
    "frequencies": "0.2 Hz to 20 Hz in 0.01 Hz steps, passed to SciPy as angular frequency",
    "fit": "floating mean: mean, sine and cosine fitted together per frequency (floating_mean=True); normalize=False",
}


def lomb_times_values():
    t = np.arange(int(8.0 * 200)) / 200.0
    keep = np.mod(t, 1.0) < 0.5
    t = t[keep]
    return t, 1.5 + np.sin(2.0 * np.pi * 4.0 * t + 0.4)


def lomb_vectors():
    t, y = lomb_times_values()
    freqs = np.arange(20, 2001) * 0.01
    power = scipy.signal.lombscargle(t, y, 2.0 * np.pi * freqs, normalize=False, floating_mean=True)
    return document(
        "lombscargle",
        52,
        "Lomb-Scargle power of a gated 4 Hz sinusoid with a mean offset.",
        "lomb_vectors",
        ["python", "numpy", "scipy"],
        LOMB_RECIPE,
        {"frequencies_hz": floats(freqs), "power": floats(power)},
        {"abs": 1e-9, "rel": 1e-9},
    )


MIDI_TIMING_RECIPE = {
    "ticks_per_beat": 480,
    "bpm": [4.0, 60.0, 120.0, 140.0, 173.5, 1000.0],
    "notes": [
        {"onset_s": 0.0, "offset_s": 0.5, "pitch": 45},
        {"onset_s": 0.25, "offset_s": 1.125, "pitch": 57},
        {"onset_s": 1.3333333333333333, "offset_s": 2.0, "pitch": 60},
        {"onset_s": 2.0, "offset_s": 2.7071, "pitch": 64},
        {"onset_s": 3.14159, "offset_s": 4.5, "pitch": 33},
        {"onset_s": 7.777, "offset_s": 9.001, "pitch": 96},
    ],
    "ticks": "seconds * BPM / 60 * ticks_per_beat, unrounded",
    "tempo": "60,000,000 / BPM microseconds per quarter note, unrounded",
}


def midi_timing():
    r = MIDI_TIMING_RECIPE
    per_beat = r["ticks_per_beat"]
    values = {}
    for bpm in r["bpm"]:
        scale = np.float64(bpm) / 60.0 * per_beat
        values[repr(bpm)] = {
            "onset_ticks": floats(np.array([n["onset_s"] for n in r["notes"]]) * scale),
            "offset_ticks": floats(np.array([n["offset_s"] for n in r["notes"]]) * scale),
            "tempo_us": float(60e6 / np.float64(bpm)),
        }
    return document(
        "midi-timing",
        62,
        "Note times in ticks and the tempo of a MIDI file, as closed forms of the BPM.",
        "midi_timing",
        ["python", "numpy"],
        r,
        values,
        {"abs": 0.5, "rel": 0.0},
        tolerance_note="half a tick, the most that rounding to whole ticks moves a time; the tempo rounds to whole microseconds",
    )


MIN_NOTE_RECIPE = {
    "frame_s": "256 / 22050",
    "settings_ms": [127.7, 58.0, 30.0, 0.0],
    "frames": "round half to even of ms / 1000 * 22050 / 256",
    "shortest_kept": "frames + 1, because the decoder drops a note of that many frames or fewer",
    "rule": "clamp(0.5 * 15000 / BPM, 58, 127.7) ms",
    "rule_bpm": [60.0, 90.0, 120.0],
}

ISSUE_FIGURES_59 = {
    "settings": {"127.7": [11, 12, 139.3], "58.0": [5, 6, 69.7], "30.0": [3, 4, 46.4], "0.0": [0, 1, 11.6]},
    "rule": {"60.0": [125.0, 11], "90.0": [83.3, 7], "120.0": [62.5, 5]},
    "ceiling_bpm": 58.7,
    "floor_bpm": 129.3,
}


def min_note_length():
    r = MIN_NOTE_RECIPE
    frame_s = 256.0 / 22050.0

    def frames(ms):
        return int(np.round(ms / 1000.0 / frame_s))

    settings = {}
    for ms in r["settings_ms"]:
        n = frames(ms)
        settings[repr(ms)] = {"frames": n, "shortest_kept_frames": n + 1, "shortest_kept_ms": (n + 1) * frame_s * 1000.0}
    rule = {}
    for bpm in r["rule_bpm"]:
        ms = float(np.clip(0.5 * 15000.0 / bpm, 58.0, 127.7))
        rule[repr(bpm)] = {"ms": ms, "frames": frames(ms)}
    values = {
        "settings": settings,
        "rule": rule,
        "ceiling_bpm": 0.5 * 15000.0 / 127.7,
        "floor_bpm": 0.5 * 15000.0 / 58.0,
    }
    issue = ISSUE_FIGURES_59
    reproduced = all(
        settings[k]["frames"] == f and settings[k]["shortest_kept_frames"] == kept and within(settings[k]["shortest_kept_ms"], ms, 0.0, 0.05)
        for k, (f, kept, ms) in issue["settings"].items()
    ) and all(
        within(rule[k]["ms"], ms, 0.0, 0.05) and rule[k]["frames"] == f for k, (ms, f) in issue["rule"].items()
    ) and within(values["ceiling_bpm"], issue["ceiling_bpm"], 0.0, 0.05) and within(values["floor_bpm"], issue["floor_bpm"], 0.0, 0.05)
    return document(
        "59-min-note-length",
        59,
        "Minimum note length settings in model frames, the shortest kept note, and the BPM rule.",
        "min_note_length",
        ["python", "numpy"],
        r,
        values,
        {"abs": 1e-9, "rel": 0.0},
        tag="[own, synthetic]",
        rounding="the issue prints one decimal",
        issue_figures={"figures": issue, "reproduced": reproduced},
    )


MATCH_RECIPE = {
    "notes": "[onset_s, offset_s, midi pitch]; pitches become Hz by 440 * 2 ** ((m - 69) / 12)",
    "onset_tolerance_s": 0.05,
    "pitch_tolerance_cents": 50.0,
    "offset": "offset_ratio 0.2 with offset_min_tolerance 0.05; onset scores pass offset_ratio=None",
    "pitch_class": "every pitch folds to 60 + (m mod 12) before matching",
    "octave_errors": "maximum onset matchings with the estimate shifted down (up) an octave count estimates an octave above (below) the reference",
    "cases": {
        "exact": [[[0.5, 1.0, 60], [1.0, 1.5, 62]], [[0.5, 1.0, 60], [1.0, 1.5, 62]]],
        "onset_plus_60_ms": [[[0.5, 1.0, 60]], [[0.56, 1.0, 60]]],
        "onset_plus_40_ms": [[[0.5, 1.0, 60]], [[0.54, 1.0, 60]]],
        "onset_plus_50_ms": [[[0.5, 1.0, 60]], [[0.55, 1.0, 60]]],
        "one_semitone": [[[0.5, 1.0, 60]], [[0.5, 1.0, 61]]],
        "octave_up": [[[0.5, 1.0, 45]], [[0.5, 1.0, 57]]],
        "octave_down": [[[0.5, 1.0, 57]], [[0.51, 1.0, 45]]],
        "duplicate": [[[0.5, 1.0, 60]], [[0.5, 1.0, 60], [0.52, 1.0, 60]]],
        "late_offset": [[[0.5, 1.0, 60], [1.5, 2.0, 62]], [[0.5, 1.3, 60], [1.5, 2.05, 62]]],
        "crowded_repeats": [[[0.0, 0.05, 40], [0.06, 0.1, 40]], [[0.04, 0.1, 40], [0.11, 0.2, 40]]],
        "no_estimate": [[[0.5, 1.0, 60]], []],
        "chord": [[[0.0, 1.0, 57], [0.0, 1.0, 60], [0.0, 1.0, 64]], [[0.01, 0.9, 57], [0.02, 1.4, 64], [0.0, 1.0, 69]]],
    },
}


def note_arrays(notes):
    a = np.asarray(notes, dtype=float).reshape(-1, 3)
    return a[:, :2], 440.0 * 2.0 ** ((a[:, 2] - 69.0) / 12.0), a[:, 2]


def prf(ref, est, offset_ratio):
    (ri, rp, _), (ei, ep, _) = note_arrays(ref), note_arrays(est)
    if len(rp) == 0 or len(ep) == 0:
        return {"precision": 0.0, "recall": 0.0, "f1": 0.0}
    p, r, f, _ = mir_eval.transcription.precision_recall_f1_overlap(
        ri, rp, ei, ep, onset_tolerance=0.05, pitch_tolerance=50.0, offset_ratio=offset_ratio
    )
    return {"precision": float(p), "recall": float(r), "f1": float(f)}


def octave_matches(ref, est, factor):
    (ri, rp, _), (ei, ep, _) = note_arrays(ref), note_arrays(est)
    if len(rp) == 0 or len(ep) == 0:
        return 0
    return len(mir_eval.transcription.match_notes(ri, rp, ei, ep * factor, offset_ratio=None))


def folded(notes):
    return [[n[0], n[1], 60 + n[2] % 12] for n in notes]


def match_cases():
    values = {}
    for name, (ref, est) in MATCH_RECIPE["cases"].items():
        values[name] = {
            "onset": prf(ref, est, None),
            "offset": prf(ref, est, 0.2),
            "pitch_class": prf(folded(ref), folded(est), None),
            "octave_up": octave_matches(ref, est, 0.5),
            "octave_down": octave_matches(ref, est, 2.0),
        }
    return document(
        "notes-match",
        65,
        "Note-transcription precision, recall and F1 by the mir_eval rules on hand-built note lists.",
        "match_cases",
        ["python", "numpy", "mir-eval"],
        MATCH_RECIPE,
        values,
        {"abs": 1e-12, "rel": 0.0},
    )


TRACK_SETTINGS = {
    "sample_rate": 22050,
    "fmin_hz": 25.0,
    "fmax_hz": 4200.0,
    "frame_length": 2048,
    "hop_length": 128,
    "resolution": 0.1,
    "center": True,
    "pad_mode": "constant",
}

TRACK_RECIPE = {
    "settings": TRACK_SETTINGS,
    "sine_bank": {
        "frequencies_hz": [30.87, 41.2, 55.0],
        "amplitude": 0.5,
        "tone_s": 0.3,
        "gap_s": 0.1,
        "layout": "gap, tone, gap, tone, gap, tone, gap; sin(2 pi f t) with t from the tone start",
        "middle": "medians and voiced counts over each tone without its first and last 0.1 s",
    },
    "glides": {
        "start_hz": 110.0,
        "semitones": 7.0,
        "glide_ms": [50.0, 150.0, 400.0],
        "hold_s": 0.15,
        "signal": "0.5 * band-limited saw, harmonic h at 1/h below the Nyquist frequency of the top pitch; phase is the running sum of f / rate; pitch linear in semitones over the glide",
        "rise": "time between the first voiced frames above 10 % and 90 % of the step, in semitones over the start pitch",
    },
}


def track_f0(y, settings):
    f0, voiced, _ = librosa.pyin(
        y,
        fmin=settings["fmin_hz"],
        fmax=settings["fmax_hz"],
        sr=settings["sample_rate"],
        frame_length=settings["frame_length"],
        hop_length=settings["hop_length"],
        resolution=settings["resolution"],
        center=settings["center"],
        pad_mode=settings["pad_mode"],
    )
    return np.where(voiced, f0, 0.0)


def sine_bank_signal(r, rate):
    gap = np.zeros(int(round(r["gap_s"] * rate)))
    t = times(rate, int(round(r["tone_s"] * rate)))
    parts = [gap]
    for f in r["frequencies_hz"]:
        parts += [r["amplitude"] * np.sin(2.0 * np.pi * f * t), gap]
    return np.concatenate(parts)


def glide_signal(r, glide_ms, rate):
    hold = int(round(r["hold_s"] * rate))
    glide = int(round(glide_ms / 1000.0 * rate))
    steps = np.concatenate([np.zeros(hold), np.arange(glide) / glide, np.ones(hold)])
    f = r["start_hz"] * 2.0 ** (steps * r["semitones"] / 12.0)
    phase = np.concatenate([[0.0], np.cumsum(f[:-1] / rate)])
    top = r["start_hz"] * 2.0 ** (r["semitones"] / 12.0)
    x = np.zeros_like(phase)
    h = 1
    while h * top < rate / 2:
        x += np.sin(2.0 * np.pi * h * phase) / h
        h += 1
    return 0.5 * x


def rise_ms(f0, r, hop_s):
    semis = np.where(f0 > 0, 12.0 * np.log2(np.maximum(f0, 1e-9) / r["start_hz"]), np.nan)
    above = lambda level: int(np.argmax(np.nan_to_num(semis, nan=-1e9) >= level))
    return (above(0.9 * r["semitones"]) - above(0.1 * r["semitones"])) * hop_s * 1000.0


def pitch_track():
    r = TRACK_RECIPE
    settings = r["settings"]
    rate = settings["sample_rate"]
    hop_s = settings["hop_length"] / rate
    bank = r["sine_bank"]
    f0 = track_f0(sine_bank_signal(bank, rate), settings)
    tones = []
    for i, f in enumerate(bank["frequencies_hz"]):
        start = bank["gap_s"] + i * (bank["tone_s"] + bank["gap_s"])
        lo, hi = int(np.ceil((start + 0.1) / hop_s)), int(np.floor((start + bank["tone_s"] - 0.1) / hop_s))
        middle = f0[lo : hi + 1]
        voiced = middle[middle > 0]
        tones.append({
            "frequency_hz": f,
            "frames": int(len(middle)),
            "voiced": int(len(voiced)),
            "median_hz": float(np.median(voiced)),
        })
    glides = {}
    for ms in r["glides"]["glide_ms"]:
        glides[repr(ms)] = {"true_rise_ms": 0.8 * ms, "rise_ms": float(rise_ms(track_f0(glide_signal(r["glides"], ms, rate), settings), r["glides"], hop_s))}
    return document(
        "pitch-track",
        113,
        "pYIN f0 of a low sine bank and of linear saw glides, with the f0 track settings of sound-to-patch.",
        "pitch_track",
        ["python", "numpy", "scipy", "librosa"],
        r,
        {"sine_bank": tones, "glides": glides},
        {"abs": 0.0, "rel": 0.0116},
        tolerance_note="medians within 20 cents (rel 0.0116); the Rust crate states in its crate docs that it is based on librosa 0.9.1, so frames differ slightly from librosa 1.0.0; rises within two hops (11.6 ms) and voiced counts within 2 % are checked in the test",
    )


LINE_RECIPE = {
    "sample_rate": 22050,
    "signal": "0.8 sin(2 pi (f_end t + df tau (1 - exp(-t / tau)))) exp(-t / decay_s) from lead_s on, silence before, length_s in all",
    "decay_s": 0.5,
    "lead_s": 0.25,
    "length_s": 1.25,
    "rows": [[41.2, 41.2, 0.020], [41.2, 41.2, 0.050], [55.0, 165.0, 0.030], [41.2, 13.8, 0.100]],
    "band": "fourth-order Butterworth high-pass at 20 Hz, then low-pass at 200 Hz, each by sosfiltfilt with odd padding of ceil(3 fs / 20) samples",
    "analytic": "scipy.signal.hilbert of the band zero-padded by its own length on each side",
    "frequency": "angle(z[n+1] conj(z[n])) fs / 2 pi",
    "median": "centred median of 31 samples, shortened at the ends",
    "line_window_s": [0.257, 0.65],
    "line_every": 25,
    "onset": "first sample in 0 to 0.5 s whose envelope |z|, averaged over a centred 221 samples (10 ms), reaches half its peak in that span",
    "fit": "scipy.optimize.least_squares of line - (f_end + df exp(-t / tau)) over the line values from 7 ms to 400 ms after the onset, t from the onset to halfway between two samples, tau bounded to 7 ms / ln 10, about 3.04 ms, to 1 s",
}

# The Hilbert-fit column of the drop table in #67: f_end (Hz), df (Hz),
# tau (ms) per row.
ISSUE_67_HILBERT = [(41.1, 34.3, 23.7), (41.1, 38.8, 53.6), (55.2, 172.5, 28.7), (40.5, 12.3, 135.0)]


def drop_tone(r, row, rate):
    f_end, df, tau = row
    n = int(round(r["length_s"] * rate))
    t = np.arange(n) / rate - r["lead_s"]
    tp = np.maximum(t, 0.0)
    phase = f_end * tp + df * tau * (1.0 - np.exp(-tp / tau))
    x = 0.8 * np.sin(2.0 * np.pi * phase) * np.exp(-tp / r["decay_s"])
    return np.where(t < 0.0, 0.0, x)


def shrinking_median(x, length):
    half = length // 2
    out = np.empty_like(x)
    for n in range(len(x)):
        out[n] = np.median(x[max(0, n - half) : n + half + 1])
    return out


def sub_band(x, rate):
    pad = int(math.ceil(3.0 * rate / 20.0))
    high = scipy.signal.butter(4, 20.0, "highpass", fs=rate, output="sos")
    low = scipy.signal.butter(4, 200.0, "lowpass", fs=rate, output="sos")
    band = scipy.signal.sosfiltfilt(high, x, padtype="odd", padlen=pad)
    band = scipy.signal.sosfiltfilt(low, band, padtype="odd", padlen=pad)
    z = scipy.signal.hilbert(np.pad(band, len(band)))[len(band) : 2 * len(band)]
    frequency = np.angle(z[1:] * np.conj(z[:-1])) * rate / (2.0 * np.pi)
    return shrinking_median(frequency, 31), np.abs(z)


def band_onset(envelope, rate, to_s):
    end = int(math.ceil(to_s * rate))
    half = int(round(0.005 * rate))
    sums = np.concatenate([[0.0], np.cumsum(envelope)])
    smooth = np.array([(sums[min(n + half + 1, len(envelope))] - sums[max(n - half, 0)]) / (min(n + half + 1, len(envelope)) - max(n - half, 0)) for n in range(end)])
    first = int(np.argmax(smooth >= 0.5 * smooth.max()))
    return first / rate


def drop_fit(line, onset, rate):
    start = int(math.ceil((onset + 0.007) * rate))
    stop = int(math.ceil((onset + 0.4) * rate))
    n = np.arange(start, stop)
    t = (n + 0.5) / rate - onset
    y = line[n]
    model = lambda p: p[0] + p[1] * np.exp(-t / p[2])
    fit = scipy.optimize.least_squares(
        lambda p: y - model(p),
        [y[-1], y[0] - y[-1], 0.02],
        bounds=([-np.inf, -np.inf, 0.007 / math.log(10)], [np.inf, np.inf, 1.0]),
        xtol=1e-15, ftol=1e-15, gtol=1e-15, max_nfev=10000,
    )
    f_end, df, tau = fit.x
    rms = float(np.sqrt(np.mean((y - model(fit.x)) ** 2)))
    seen = abs(float(df)) * math.exp(-0.007 / float(tau))
    return {"onset_s": onset, "f_end_hz": float(f_end), "df_hz": float(df), "tau_s": float(tau), "residual": rms / seen, "points": int(len(n))}


def sub_band_line():
    r = LINE_RECIPE
    rate = r["sample_rate"]
    first = drop_tone(r, r["rows"][0], rate)
    line, _ = sub_band(first, rate)
    lo, hi = (int(round(t * rate)) for t in r["line_window_s"])
    indices = list(range(lo, hi, r["line_every"]))
    rows = []
    for row in r["rows"]:
        line_row, envelope = sub_band(drop_tone(r, row, rate), rate)
        rows.append(drop_fit(line_row, band_onset(envelope, rate, 0.5), rate))
    figures = {
        "hilbert_fit": [{"f_end_hz": f, "df_hz": d, "tau_ms": t} for f, d, t in ISSUE_67_HILBERT],
    }
    reproduced = [
        within(row["f_end_hz"], f, 0.0, 0.1) and within(row["df_hz"], d, 0.05, 0.0) and within(1000.0 * row["tau_s"], t, 0.05, 0.0)
        for row, (f, d, t) in zip(rows, ISSUE_67_HILBERT)
    ]
    return document(
        "sub-band-line",
        114,
        "The sub-band Hilbert line of a generated 808 tone and the drop fit on it, for the four rows of the #67 drop table.",
        "sub_band_line",
        ["python", "numpy", "scipy"],
        r,
        {"line_indices": indices, "line_hz": floats(line[indices]), "drops": rows},
        {"abs": 5e-4, "rel": 1e-5},
        tolerance_note="line values within 5e-4 Hz, about 1e-5 of the tone: the Rust filter sections and the SciPy ones round differently, and the frequency between samples amplifies that, measured up to 3.2e-4 Hz; fitted f_end, df and tau within a relative 1e-5, since the two minimisers stop at slightly different points of the same minimum, measured up to 1.5e-6",
        issue_figures={**figures, "rows": reproduced, "reproduced": all(reproduced), "why": "the issue fit from 3 ms after the true onset; this recipe fits from 7 ms after the envelope onset, because the line of a tone switched on from silence overshoots by about 11 Hz at 3 ms"},
    )


GLIDE_ISSUE_RECIPE = {
    "settings": {**TRACK_SETTINGS, "fmin_hz": 30.0, "fmax_hz": 400.0},
    "glides": {**TRACK_RECIPE["glides"], "hold_s": 0.5},
    "note": "the #61 method: frame 2048, hop 128, fmin 30 Hz; the issue states no fmax, so the #61 track ceiling of 400 Hz is used",
    "tolerance": "a rise is a whole number of hops (5.8 ms), so a row reproduces when it lies within one hop of the issue figure",
}


def glide_issue_table():
    r = GLIDE_ISSUE_RECIPE
    rate = r["settings"]["sample_rate"]
    hop_s = r["settings"]["hop_length"] / rate
    values = {}
    for ms in r["glides"]["glide_ms"]:
        values[repr(ms)] = {"true_rise_ms": 0.8 * ms, "rise_ms": float(rise_ms(track_f0(glide_signal(r["glides"], ms, rate), r["settings"]), r["glides"], hop_s))}
    issue = {"50.0": 29.0, "150.0": 116.0, "400.0": 319.0}
    hop_ms = hop_s * 1000.0
    rows = {k: within(values[k]["rise_ms"], v, 0.0, hop_ms) for k, v in issue.items()}
    return document(
        "61-glide",
        61,
        "10-90 % rise time of 7-semitone linear saw glides read by librosa pYIN.",
        "glide_issue_table",
        ["python", "numpy", "scipy", "librosa"],
        r,
        values,
        {"abs": hop_ms, "rel": 0.0},
        tag="[own, synthetic]",
        rounding="the issue prints whole milliseconds",
        issue_figures={"figures": issue, "rows": rows, "reproduced": all(rows.values())},
    )


ENVELOPE_HOP_S = 0.001


def envelope_saw(f0, rate, length, phase, top_hz=6000.0):
    """The saw of the envelope tests: harmonic h at 1/h up to 6 kHz or the
    Nyquist frequency, starting at `phase` cycles."""
    n = np.arange(length, dtype=np.float64)
    x = np.zeros(length)
    for h in range(1, int(math.floor(min(top_hz, 0.5 * rate) / f0)) + 1):
        x += np.sin(2.0 * np.pi * h * phase + 2.0 * np.pi * h * f0 / rate * n) / h
    return x


def centred_rms_db(x, rate, f0, periods, floor_s, start_s, end_s):
    """Centred RMS in dB on a 1 ms hop over the fewest whole periods of f0
    that reach both `periods` and `floor_s`; the window shrinks at the ends."""
    window = max(periods, math.ceil(floor_s * f0)) / f0
    half = max(int(round(0.5 * window * rate)), 1)
    sums = np.concatenate([[0.0], np.cumsum(x * x)])
    count = int(math.floor((end_s - start_s) / ENVELOPE_HOP_S + 1e-9)) + 1
    out = np.empty(count)
    for i in range(count):
        centre = int(round((start_s + i * ENVELOPE_HOP_S) * rate))
        lo, hi = min(max(centre - half, 0), len(x)), min(centre + half, len(x))
        power = (sums[hi] - sums[lo]) / (hi - lo) if hi > lo else 0.0
        out[i] = max(10.0 * math.log10(power), -200.0) if power > 0 else -200.0
    return out


def attack_top(db, onset, least):
    """The top level of an attack from frame `onset`, in dB: at the first
    frame that the level exceeds by less than 1 dB over as long again as it
    took to rise to it from under 10 % of it, and at least over `least`
    frames, the highest frame over that span."""
    for i in range(max(onset, 1), len(db)):
        under = [j for j in range(i) if db[j] < db[i] - 20.0]
        if not under:
            continue
        ahead = db[i:min(i + max(i - under[-1], least), len(db) - 1) + 1].max()
        if ahead < db[i] + 1.0:
            return ahead
    raise ValueError("no attack top")


def rise_10_90(db, onset, least=None):
    """The 10-90 % rise of the linear amplitude from frame `onset`, in
    seconds, both crossings linear between frames: to the attack's top with
    `least` frames of look-ahead, or without it to the highest frame from
    the onset on."""
    amp = 10.0 ** ((db - (db[onset:].max() if least is None else attack_top(db, onset, least))) / 20.0)

    def cross(i, level):
        a, b = amp[i - 1], amp[i]
        w = (level - a) / (b - a) if b > a else 1.0
        return i - 1 + min(max(w, 0.0), 1.0)

    up = next(i for i in range(max(onset, 1), len(amp)) if amp[i] >= 0.9)
    low = next(i for i in range(up - 1, -1, -1) if amp[i] < 0.1)
    return (cross(up, 0.9) - cross(low + 1, 0.1)) * ENVELOPE_HOP_S


ATTACK_RECIPE = {
    "sample_rate": 22050,
    "f0_hz": [55.0, 220.0],
    "attack_s": [0.001, 0.005, 0.02, 0.1],
    "note": "one saw note, harmonic h at 1/h up to 6 kHz, at phase uniform(31, 1) cycles and level 0.3, from 0.2 s; a linear attack to 1, then a decay linear in dB over 0.2 s to -6 dB, held to 0.8 s past the attack and released linearly over 50 ms",
    "track": "centred RMS on a 1 ms hop from 30 ms before the onset",
    "top": "ours rises to the attack's top: the first frame from the onset that the level exceeds by less than 1 dB over as long again as it took to rise to it from under 10 % of it, and at least over the shortest rise the track can tell, here 1.5 RMS windows, and the shares are of the highest frame over that span; the #69 window rises to the highest frame from the onset on",
    "ours": "a window of one period of f0, or the fewest whole periods spanning 2 ms",
    "issue": "the #69 window: two periods of f0 and at least 5 ms, in whole periods",
    "seed": 31,
}

# The 10-90 % rise ranges that #69 prints, in ms, per f0 and attack.
ISSUE_69_ATTACK = {
    55.0: {0.001: (14.0, 16.6), 0.005: (14.0, 17.1), 0.02: (10.0, 19.0), 0.1: (74.3, 87.3)},
    220.0: {0.001: (2.7, 4.6), 0.005: (5.0, 7.8), 0.02: (15.1, 18.0), 0.1: (78.2, 81.8)},
}


def attack_note(f0, attack_s, rate):
    off = 0.2 + attack_s + 0.8
    length = int(round((off + 0.5) * rate))
    start, stop = int(round(0.2 * rate)), min(int(round((off + 0.05) * rate)), length)
    t = np.arange(stop - start) / rate
    held = np.where(t < attack_s, t / attack_s, 10.0 ** (-6.0 * np.clip((t - attack_s) / 0.2, 0.0, 1.0) / 20.0))
    at_off = 10.0 ** (-6.0 * min((off - 0.2 - attack_s) / 0.2, 1.0) / 20.0)
    gain = np.where(t < off - 0.2, held, at_off * np.maximum(1.0 - (t - (off - 0.2)) / 0.05, 0.0))
    x = np.zeros(length)
    x[start:stop] = 0.3 * envelope_saw(f0, rate, stop - start, uniform(31, 1)[0]) * gain
    return x, off


def attack_table():
    r = ATTACK_RECIPE
    rate = r["sample_rate"]
    rows, reproduced = [], []
    for f0 in r["f0_hz"]:
        for attack_s in r["attack_s"]:
            x, off = attack_note(f0, attack_s, rate)
            start = 0.2 - 0.03
            periods = max(1.0, math.ceil(0.002 * f0))
            least = math.ceil(1.5 * periods / f0 / ENVELOPE_HOP_S)
            ours = rise_10_90(centred_rms_db(x, rate, f0, 1.0, 0.002, start, off + 0.09), 30, least)
            issue = rise_10_90(centred_rms_db(x, rate, f0, 2.0, 0.005, start, off + 0.09), 30)
            lo, hi = ISSUE_69_ATTACK[f0][attack_s]
            hop_ms = 1000.0 * ENVELOPE_HOP_S
            reproduced.append(bool(lo - hop_ms <= 1000.0 * issue <= hi + hop_ms))
            rows.append({"f0_hz": f0, "attack_s": attack_s, "true_rise_s": 0.8 * attack_s, "rise_s": ours, "issue_window_rise_s": issue})
    return document(
        "69-attack",
        69,
        "10-90 % rise of the centred RMS of a saw note with a linear attack, with the layer's window and with the #69 window.",
        "attack_table",
        ["python", "numpy"],
        r,
        {"rows": rows},
        {"abs": 1e-4, "rel": 0.0},
        tag="[own, synthetic]",
        tolerance_note="rise_s within 0.1 ms: both sides read the same frames, and the saws differ only in the last bits of their sines",
        issue_figures={"figures_ms": {str(f): {str(a): list(v) for a, v in rows_.items()} for f, rows_ in ISSUE_69_ATTACK.items()}, "rows": reproduced, "reproduced": all(reproduced), "why": "the issue gives ranges over estimators; a row reproduces when the #69 window's rise lies within one hop of its range"},
    )


PUMP_RECIPE = {
    "sample_rate": 22050,
    "bpm": 128.0,
    "beats": 16,
    "chord_hz": [110.0, 130.81, 164.81],
    "seed": 1,
    "chord": "saws of the envelope tests, each at phase uniform(1, 3) cycles in chord order, summed and scaled to a peak of 0.5, held from 0 s to the end",
    "duck": "gain 1 - (1 - 10^(-depth / 20)) exp(-(t mod beat) / release) on every beat from 0 s",
    "rows": [[0.0, 0.15], [6.0, 0.15], [12.0, 0.08], [12.0, 0.15], [12.0, 0.3], [18.0, 0.15]],
    "track": "centred RMS in dB over two periods of 110 Hz on a 1 ms hop from 0 s to the end",
    "beats_counted": "a beat counts when it starts 50 ms or more after 0 s and ends by the end",
    "curve": "64 phase bins per beat, each beat minus its 90th percentile, the median over the beats",
    "recovery": "from the curve's minimum, over the beat's wrap, until it comes back within 1.5 dB, linear between bins",
}

# The rows that #71 prints at 128 BPM: depth read in dB and time back in s.
ISSUE_71_PUMP = [(1.6, None), (5.6, 0.212), (9.4, 0.176), (10.1, 0.234), (9.3, 0.264), (13.7, 0.242)]


def pump_curve(db, period):
    bins = 64
    curves = []
    b = 0
    while True:
        start = b * period
        end = start + period
        b += 1
        if end > len(db) * ENVELOPE_HOP_S:
            break
        if start - 0.05 < 0.0:
            continue
        lo = int(math.ceil(start / ENVELOPE_HOP_S - 1e-9))
        hi = int(math.ceil(end / ENVELOPE_HOP_S - 1e-9))
        sums, counts = np.zeros(bins), np.zeros(bins)
        for i in range(lo, hi):
            k = min(int(math.floor((i * ENVELOPE_HOP_S - start) / period * bins)), bins - 1)
            sums[k] += db[i]
            counts[k] += 1
        means = sums / counts
        curves.append(means - np.quantile(means, 0.9))
    return np.median(np.array(curves), axis=0), len(curves)


def pump_recovery(curve, period):
    bins = len(curve)
    low = int(np.argmin(curve))
    at = lambda k: curve[(low + k) % bins]
    for k in range(1, bins):
        if at(k) >= -1.5:
            a, b = at(k - 1), at(k)
            w = (-1.5 - a) / (b - a) if b > a else 1.0
            return (k - 1 + min(max(w, 0.0), 1.0)) * period / bins
    return None


def pump_table():
    r = PUMP_RECIPE
    rate = r["sample_rate"]
    period = 60.0 / r["bpm"]
    seconds = r["beats"] * period
    length = int(round(seconds * rate))
    x = np.zeros(length)
    for f0, phase in zip(r["chord_hz"], uniform(r["seed"], len(r["chord_hz"]))):
        x += envelope_saw(f0, rate, length, phase)
    x *= 0.5 / np.abs(x).max()
    t = np.arange(length) / rate
    rows, reproduced = [], []
    for (depth, release), (figure, back_figure) in zip(r["rows"], ISSUE_71_PUMP):
        gain = 1.0 - (1.0 - 10.0 ** (-depth / 20.0)) * np.exp(-np.mod(t, period) / release)
        db = centred_rms_db(x * gain, rate, 110.0, 2.0, 0.002, 0.0, length / rate)
        curve, beats = pump_curve(db, period)
        read = float(-min(curve.min(), 0.0))
        back = pump_recovery(curve, period)
        rows.append({"depth_db": depth, "release_s": release, "read_depth_db": read, "back_s": back if depth > 0 else None, "beats": beats})
        ok = abs(read - figure) <= 0.5 and (back_figure is None or abs(back / back_figure - 1.0) <= 0.3)
        reproduced.append(bool(ok))
    return document(
        "71-pump",
        71,
        "The #71 pump detector on a held saw chord at 128 BPM under an exponential duck: depth and time back within 1.5 dB.",
        "pump_table",
        ["python", "numpy"],
        r,
        {"rows": rows},
        {"abs": 0.01, "rel": 0.0},
        tag="[own, synthetic]",
        tolerance_note="depth within 0.01 dB and time back within 0.01 s: both sides bin the same frames, and the saws differ only in the last bits of their sines",
        issue_figures={"figures": [{"read_depth_db": d, "back_s": b} for d, b in ISSUE_71_PUMP], "rows": reproduced, "reproduced": all(reproduced), "why": "a row reproduces within 0.5 dB of depth and 30 % of the time back; the issue does not give its chord phases"},
    )


VECTORS = {
    "window": window_vectors,
    "stft": stft_vectors,
    "hilbert": hilbert_vectors,
    "decay": decay_closed_forms,
    "stereo": stereo_delay,
    "descriptors": descriptor_vectors,
    "lombscargle": lomb_vectors,
    "midi-timing": midi_timing,
    "notes-match": match_cases,
    "pitch-track": pitch_track,
    "sub-band-line": sub_band_line,
    "prototype/48-peak-accuracy": peak_accuracy,
    "prototype/53-noise-floor": decay_noise_floor,
    "prototype/53-release-reverb": decay_release_reverb,
    "prototype/53-drop-time": decay_drop_time,
    "prototype/47-stereo-screen": stereo_screen,
    "prototype/51-two-saw-peaks": two_saw_peaks,
    "prototype/54-sawtooth": sawtooth_table,
    "prototype/54-noise-flatness": noise_flatness,
    "prototype/54-crest": crest_values,
    "prototype/59-min-note-length": min_note_length,
    "prototype/61-glide": glide_issue_table,
    "prototype/69-attack": attack_table,
    "prototype/71-pump": pump_table,
}


def main():
    if len(sys.argv) != 2:
        sys.exit("usage: generate.py <repository root>")
    root = Path(sys.argv[1])
    for name, build in VECTORS.items():
        write(root, name, build())
        print(f"wrote tests/fixtures/golden/{name}.json")


if __name__ == "__main__":
    main()
