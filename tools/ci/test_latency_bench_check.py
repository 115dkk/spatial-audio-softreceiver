import json
import tempfile
import unittest
from pathlib import Path

from latency_bench_check import expected_handoff_ms, judge, main


def report(**changes):
    # pcm71-1ms at a 10 ms exclusive period through the ASRC.
    value = {"output_mode": "exclusive", "output_period_frames": 480,
             "output_sample_rate": 48000, "output_reopens": 0, "clock_strategy": "asrc",
             "handoff_latency_p50_ms": 39.2, "handoff_latency_p99_ms": 41.0,
             "device_latency_p50_ms": 14.8, "latency_p50_ms": 54.0}
    value.update(changes)
    return value


class ExpectationTests(unittest.TestCase):
    def test_the_formula_follows_adr_0044(self):
        self.assertAlmostEqual(expected_handoff_ms(10, asrc=True), 38.7)
        self.assertAlmostEqual(expected_handoff_ms(20, asrc=True), 43.7)
        self.assertAlmostEqual(expected_handoff_ms(3, asrc=False), 25.5)


class JudgeTests(unittest.TestCase):
    def test_a_run_near_the_expectation_passes_without_notes(self):
        problems, notes, figures = judge(report())
        self.assertEqual((problems, notes), ([], []))
        self.assertAlmostEqual(figures["limit_ms"], 48.7)

    def test_a_run_above_the_limit_fails(self):
        # The v0.26.0 crossing ring kind of change: seconds behind.
        problems, _, _ = judge(report(handoff_latency_p50_ms=60.0))
        self.assertEqual(problems, [
            "handoff latency p50 60.0 ms at a 10 ms period, above 48.7 ms (expected 38.7 + 10)",
        ])

    def test_a_run_off_the_formula_but_under_the_limit_is_noted(self):
        problems, notes, _ = judge(report(handoff_latency_p50_ms=34.0))
        self.assertEqual(problems, [])
        self.assertEqual(notes, [
            "note: handoff latency p50 34.0 ms is -4.7 ms from the expected 38.7 ms; amend "
            "the formula in ADR 0044",
        ])

    def test_the_period_comes_from_the_report(self):
        problems, _, figures = judge(report(output_period_frames=960,
                                            handoff_latency_p50_ms=44.0))
        self.assertEqual(problems, [])
        self.assertAlmostEqual(figures["expected_ms"], 43.7)

    def test_a_trimmed_clock_leaves_the_asrc_out(self):
        _, _, figures = judge(report(clock_strategy="trim"))
        self.assertAlmostEqual(figures["expected_ms"], 36.0)

    def test_a_report_without_handoff_fields_fails(self):
        value = report()
        del value["handoff_latency_p50_ms"]
        problems, _, figures = judge(value)
        self.assertEqual(problems, ["the report lacks handoff_latency_p50_ms as numbers"])
        self.assertIsNone(figures)

    def test_a_null_handoff_fails(self):
        # A sink that cannot tell the handoff time leaves the fields null.
        problems, _, _ = judge(report(handoff_latency_p50_ms=None))
        self.assertEqual(problems, ["the report lacks handoff_latency_p50_ms as numbers"])

    def test_a_shared_run_fails(self):
        problems, _, _ = judge(report(output_mode="shared"))
        self.assertEqual(problems, ["the output ran 'shared', not exclusive"])

    def test_a_reopen_fails(self):
        problems, _, _ = judge(report(output_reopens=1))
        self.assertEqual(problems, [
            "the output reopened 1 times: the period changed under the measurement",
        ])

    def test_impossible_figures_fail(self):
        problems, _, _ = judge(report(handoff_latency_p50_ms=-1.0, device_latency_p50_ms=-2.0))
        self.assertEqual(problems, [
            "handoff latency p50 -1.0 ms: the handoff time is not after the source time",
            "device latency p50 -2.0 ms: the device played a frame before it was handed over",
        ])


class MainTests(unittest.TestCase):
    def test_main_writes_a_summary_row_and_exits_by_the_verdict(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory, "report.json")
            summary = Path(directory, "summary.md")
            path.write_text(json.dumps(report()), encoding="utf-8")
            self.assertEqual(main([str(path), "--summary", str(summary), "--name", "10 ms"]), 0)
            path.write_text(json.dumps(report(handoff_latency_p50_ms=60.0)), encoding="utf-8")
            self.assertEqual(main([str(path), "--summary", str(summary), "--name", "10 ms"]), 1)
            self.assertEqual(summary.read_text(encoding="utf-8").splitlines(), [
                "| 10 ms | 10 ms | 39.2 / 41.0 ms | 38.7 ms | 48.7 ms | 14.8 ms | 54.0 ms | pass |",
                "| 10 ms | 10 ms | 60.0 / 41.0 ms | 38.7 ms | 48.7 ms | 14.8 ms | 54.0 ms | fail |",
            ])

    def test_main_fails_on_an_unreadable_report(self):
        self.assertEqual(main(["does-not-exist.json"]), 1)


if __name__ == "__main__":
    unittest.main()
