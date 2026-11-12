"""Tests of generate.py. `cargo xtask golden` runs them before it writes a
vector, with the same pinned tools."""

import math
import sys
import unittest
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))

import generate  # noqa: E402


class CramerRao(unittest.TestCase):
    def test_the_bound_matches_the_fisher_information_of_the_recipe(self):
        # The inverse Fisher information of A sin(w n + phi) in white noise,
        # with w, phi and A unknown, at the 48-peak-accuracy recipe.
        r = generate.PEAK_RECIPE
        n, rate, a = r["frame_len"], r["sample_rate"], r["amplitude"]
        sigma2 = a**2 / (2.0 * 10 ** (r["snr_db"] / 10.0))
        k = np.arange(n)
        w = 2.0 * np.pi * 100.3 / rate
        d = np.stack([a * k * np.cos(w * k + 0.7), a * np.cos(w * k + 0.7), np.sin(w * k + 0.7)])
        fisher = d @ d.T / sigma2
        exact_hz = math.sqrt(np.linalg.inv(fisher)[0, 0]) * rate / (2.0 * np.pi)
        snr = a**2 / (2.0 * sigma2)
        bound_hz = generate.cramer_rao_cycles(snr, n) * rate
        self.assertLess(abs(bound_hz / exact_hz - 1.0), 0.02)


class Rise(unittest.TestCase):
    R = {"start_hz": 110.0, "semitones": 7.0}

    def track(self, semitones):
        return 110.0 * 2.0 ** (np.asarray(semitones, dtype=float) / 12.0)

    def test_a_glide_that_never_reaches_90_percent_has_no_rise(self):
        f0 = self.track(np.linspace(0.0, 5.0, 40))
        self.assertIsNone(generate.rise_ms(f0, self.R, 0.001))

    def test_an_unvoiced_glide_has_no_rise(self):
        f0 = np.concatenate([self.track([0.0] * 10), np.zeros(10), self.track([7.0] * 10)])
        self.assertIsNone(generate.rise_ms(f0, self.R, 0.001))

    def test_a_full_glide_rises_from_10_to_90_percent(self):
        # 0.7 semitones (10 %) is first passed at frame 5, 6.3 (90 %) at frame 10.
        f0 = self.track([0.0] * 5 + [3.0] * 5 + [7.0] * 5)
        self.assertAlmostEqual(generate.rise_ms(f0, self.R, 0.001), 5.0)


class Sources(unittest.TestCase):
    def test_every_vector_names_the_hashes_of_the_generator_and_the_pins(self):
        doc = generate.document("x", 1, "d", "f", ["python"], {}, [], {"abs": 0.0, "rel": 0.0})
        self.assertEqual(set(doc["sources"]), {generate.GENERATOR, generate.REQUIREMENTS})
        self.assertTrue(all(len(h) == 64 for h in doc["sources"].values()))


if __name__ == "__main__":
    unittest.main()
