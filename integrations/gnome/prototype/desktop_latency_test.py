"""Deterministic checks for separate first-observation latency accounting."""

import unittest

from desktop_latency import observe_ready, summarize


class LatencyTest(unittest.TestCase):
    def test_context_can_precede_input_without_being_overwritten(self):
        clock = [0.0]
        def input_ready():
            clock[0] += .010
            return clock[0] >= .030
        def context_ready():
            clock[0] += .005
            return True
        result = observe_ready(input_ready, context_ready, started=0,
                               clock=lambda: clock[0], sleep=lambda _: None)
        self.assertAlmostEqual(result["contextObservedMs"], 15)
        self.assertAlmostEqual(result["inputObservedMs"], 35)

    def test_input_can_precede_context(self):
        clock = [0.0]
        def context_ready():
            clock[0] += .020
            return clock[0] >= .040
        result = observe_ready(lambda: True, context_ready, started=0,
                               clock=lambda: clock[0], sleep=lambda _: None)
        self.assertEqual(result["inputObservedMs"], 0)
        self.assertAlmostEqual(result["contextObservedMs"], 40)

    def test_slow_positive_observation_cannot_escape_deadline(self):
        clock = [0.0]
        def input_ready():
            clock[0] = 13
            return True
        with self.assertRaises(AssertionError):
            observe_ready(input_ready, lambda: True, started=0,
                          clock=lambda: clock[0], sleep=lambda _: None)

    def test_nearest_rank_includes_outliers_and_requires_exact_sample_count(self):
        values = [1] * 94 + [251] * 6
        result = summarize(values)
        self.assertEqual(result, {"count": 100, "p50Ms": 1, "p95Ms": 251, "maxMs": 251,
                                  "within250ms": False, "requiredSamples": True})
        self.assertFalse(summarize(values[:-1])["requiredSamples"])

    def test_invalid_samples_refused(self):
        for values in ([], [-1], [float("nan")], [float("inf")]):
            with self.subTest(values=values), self.assertRaises(AssertionError):
                summarize(values)


if __name__ == "__main__":
    unittest.main()
