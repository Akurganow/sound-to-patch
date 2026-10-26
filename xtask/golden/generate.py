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
import numpy as np
import scipy
import scipy.signal

GENERATOR = "xtask/golden/generate.py"
TOOLS = {
    "python": ".".join(str(part) for part in sys.version_info[:3]),
    "numpy": np.__version__,
    "scipy": scipy.__version__,
    "librosa": librosa.__version__,
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
    "|L conj R|; then the mean over frames whose mid RMS is within 40 dB of the loudest",
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
    frames_mid = librosa.util.frame(np.pad(mid, n_fft // 2), frame_length=n_fft, hop_length=n_fft // 4)
    frame_rms = np.sqrt(np.mean(frames_mid**2, axis=0))
    active = frame_rms >= frame_rms.max() * 10 ** (-40.0 / 20.0)
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


def stereo_closed_forms():
    return document(
        "stereo",
        47,
        "Closed forms of the stereo screen: rho of equal and opposite channels, the phase "
        "of a pure delay, and side over mid against rho for channels of equal RMS.",
        "stereo_closed_forms",
        ["python"],
        {"kind": "closed_form"},
        {
            "equal_channels": {"rho": 1.0, "ipd_deg": 0.0},
            "opposite_channels": {"rho": -1.0},
            "pure_delay": {"frequency_hz": 200.0, "delay_s": 0.001, "ipd_deg": 360.0 * 200.0 * 0.001},
            "equal_rms_identity": "(rms_S / rms_M)^2 = (1 - rho) / (1 + rho)",
        },
        {"abs": 1e-9, "rel": 1e-6},
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
    "centring": "y - mean(y) before the call; normalize=False",
}


def lomb_times_values():
    t = np.arange(int(8.0 * 200)) / 200.0
    keep = np.mod(t, 1.0) < 0.5
    t = t[keep]
    return t, 1.5 + np.sin(2.0 * np.pi * 4.0 * t + 0.4)


def lomb_vectors():
    t, y = lomb_times_values()
    freqs = np.arange(20, 2001) * 0.01
    power = scipy.signal.lombscargle(t, y - y.mean(), 2.0 * np.pi * freqs, normalize=False)
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


VECTORS = {
    "window": window_vectors,
    "stft": stft_vectors,
    "hilbert": hilbert_vectors,
    "decay": decay_closed_forms,
    "stereo": stereo_closed_forms,
    "descriptors": descriptor_vectors,
    "lombscargle": lomb_vectors,
    "prototype/48-peak-accuracy": peak_accuracy,
    "prototype/53-noise-floor": decay_noise_floor,
    "prototype/53-release-reverb": decay_release_reverb,
    "prototype/53-drop-time": decay_drop_time,
    "prototype/47-stereo-screen": stereo_screen,
    "prototype/51-two-saw-peaks": two_saw_peaks,
    "prototype/54-sawtooth": sawtooth_table,
    "prototype/54-noise-flatness": noise_flatness,
    "prototype/54-crest": crest_values,
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
