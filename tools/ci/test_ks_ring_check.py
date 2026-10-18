"""Unit tests for ks_ring_check's ring check; numpy only, no device."""

import unittest

import numpy as np

from driver_flow_check import parse_records
from ks_probe_check import TONE_DBFS
from ks_ring_check import ring_problems
from test_driver_flow_check import QPC, float_extensible, record
from driver_flow_check import KIND_DATA, KIND_FORMAT_CHANGE, KIND_STREAM_START

RATE = 48_000


def probe_stream(endpoint=1, seconds=2.0, dbfs=TONE_DBFS, gap=False):
    fmt = float_extensible(channels=2)
    frames = int(seconds * RATE)
    t = np.arange(frames) / RATE
    mono = (10 ** (dbfs / 20)) * np.sin(2 * np.pi * 1000.0 * t)
    samples = np.repeat(mono[:, None], 2, axis=1).astype("<f4").tobytes()
    out = [record(KIND_FORMAT_CHANGE, endpoint, fmt), record(KIND_STREAM_START, endpoint)]
    step = RATE * 8 // 1000
    position = 0
    for index, start in enumerate(range(0, len(samples), step)):
        chunk = samples[start:start + step]
        if gap and index == 100:
            position += step
        out.append(record(KIND_DATA, endpoint, chunk, byte_pos=position, qpc=index * QPC // 1000))
        position += len(chunk)
    return b"".join(out)


class RingTests(unittest.TestCase):
    def test_the_probe_tone_on_endpoint_one_passes(self):
        summary, problems = ring_problems(parse_records(probe_stream()), 1, QPC)
        self.assertEqual(problems, [])
        self.assertAlmostEqual(summary["tone_db"], TONE_DBFS, delta=0.5)

    def test_a_quiet_tone_and_a_byte_gap_fail(self):
        _, problems = ring_problems(parse_records(probe_stream(dbfs=-20.0, gap=True)), 1, QPC)
        self.assertTrue(any("gaps in byte_pos" in p for p in problems), problems)
        self.assertTrue(any("the probe tone is at" in p for p in problems), problems)

    def test_another_endpoint_has_nothing_to_check(self):
        _, problems = ring_problems(parse_records(probe_stream(endpoint=0)), 1, QPC)
        self.assertEqual(problems, ["endpoint 1: no format record"])


if __name__ == "__main__":
    unittest.main()
