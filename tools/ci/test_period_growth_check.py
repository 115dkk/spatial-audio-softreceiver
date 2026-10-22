import json
import tempfile
import unittest
from pathlib import Path

from period_growth_check import growth_problems, main


def report(**changes):
    # 3 -> 6 ms on restarts, 6 -> 12 ms on a slow cadence, then it holds.
    value = {"output_mode": "exclusive", "output_restarts": 3, "output_reopens": 2,
             "output_period_frames": 576, "output_cadence_ratio": 0.998,
             "latency_p99_ms": 71.4}
    value.update(changes)
    return value


class GrowthTests(unittest.TestCase):
    def test_a_period_that_grew_until_it_held_passes(self):
        self.assertEqual(growth_problems(report()), [])
        self.assertEqual(growth_problems(report(output_restarts=8)), [])
        self.assertEqual(growth_problems(report(output_reopens=3, output_period_frames=960,
                                                output_cadence_ratio=1.0)), [])

    def test_a_backlog_that_stayed_after_the_reopen_fails(self):
        # windows-2022 at 69353c5: 12 ms held, but the 2.5 s the 6 ms period
        # piled up in the crossing ring stayed as latency.
        self.assertEqual(growth_problems(report(latency_p99_ms=2590.8)), [
            "latency p99 2591 ms since the last reopen, above 250 ms: the backlog of the "
            "slow periods stayed",
        ])

    def test_a_final_period_that_stays_slow_fails(self):
        # windows-2025 at 69353c5: 12 ms ran at 0.6, the guard grew only once,
        # and the ring overflowed.
        self.assertEqual(growth_problems(report(output_cadence_ratio=0.603,
                                                latency_p99_ms=5968.6)), [
            "the last cadence window ran at 0.603 of the nominal rate: the final period did "
            "not hold",
            "latency p99 5969 ms since the last reopen, above 250 ms: the backlog of the "
            "slow periods stayed",
        ])

    def test_a_cable_that_held_its_minimum_did_not_exercise_the_growth(self):
        problems = growth_problems(report(output_restarts=0, output_reopens=0,
                                          output_period_frames=144))
        self.assertEqual(len(problems), 3)
        self.assertIn("did not exercise the growth", problems[0])

    def test_restarts_ten_times_a_second_at_the_minimum_fail(self):
        # The defect: 137 restarts in 12 s and the period never grew.
        problems = growth_problems(report(output_restarts=137, output_reopens=0,
                                          output_period_frames=144))
        self.assertEqual(problems, [
            "the output ended at 144 frames a period, outside 480..=960",
            "0 reopens, expected 1..=3",
        ])

    def test_restarts_that_go_on_after_the_growth_fail(self):
        self.assertEqual(growth_problems(report(output_restarts=40)),
                         ["40 restarts for 2 reopens: the restarts went on after the period grew"])
        self.assertEqual(growth_problems(report(output_restarts=9)),
                         ["9 restarts for 2 reopens: the restarts went on after the period grew"])

    def test_more_reopens_than_the_cap_allows_fail(self):
        self.assertEqual(growth_problems(report(output_reopens=4, output_restarts=12,
                                                output_period_frames=960)),
                         ["4 reopens, expected 1..=3"])

    def test_shared_output_and_missing_fields_fail(self):
        self.assertEqual(growth_problems(report(output_mode="shared")),
                         ["the output ran 'shared', not exclusive"])
        self.assertEqual(growth_problems(report(output_period_frames=None)),
                         ["the report lacks output_restarts, output_reopens, output_period_frames as counts"])
        self.assertEqual(growth_problems(report(latency_p99_ms=None)),
                         ["the report lacks output_cadence_ratio, latency_p99_ms as numbers"])
        self.assertEqual(growth_problems(report(output_cadence_ratio=True)),
                         ["the report lacks output_cadence_ratio, latency_p99_ms as numbers"])
        self.assertEqual(growth_problems([]), ["the report must be an object"])

    def test_the_command_line_reads_the_file_and_exits_with_the_verdict(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "report.json"
            path.write_text(json.dumps(report()), encoding="utf-8")
            self.assertEqual(main([str(path)]), 0)
            path.write_text(json.dumps(report(output_reopens=0)), encoding="utf-8")
            self.assertEqual(main([str(path)]), 1)
            self.assertEqual(main([str(Path(folder) / "missing.json")]), 1)


if __name__ == "__main__":
    unittest.main()
