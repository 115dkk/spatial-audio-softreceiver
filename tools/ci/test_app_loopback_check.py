import contextlib
import io
import json
import math
import struct
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import app_loopback_check
from ssr_abi import (
    ABI_VERSION,
    DUMP_FORMAT_VERSION,
    DUMP_HEADER,
    DUMP_MAGIC,
    RECORD_ALIGN,
    RECORD_HEADER,
    RECORD_KIND_DATA,
    RECORD_KIND_FORMAT_CHANGE,
    RECORD_KIND_STREAM_START,
    WAVEFORMATEXTENSIBLE,
    WAVE_FORMAT_EXTENSIBLE,
)

QPC_FREQUENCY = 10_000_000


def record(kind, payload=b"", qpc=0, endpoint=0, epoch=1, position=0):
    header = RECORD_HEADER.pack(kind, 0, epoch, position, qpc, 0, len(payload), endpoint, 0)
    result = header + payload
    return result + bytes((-len(result)) % RECORD_ALIGN)


def wave(rate=48_000, channels=2, mask=None):
    block = channels * 4
    guid = bytes.fromhex("0300000000001000800000aa00389b71")
    if mask is None:
        mask = (1 << channels) - 1
    return WAVEFORMATEXTENSIBLE.pack(
        WAVE_FORMAT_EXTENSIBLE,
        channels,
        rate,
        rate * block,
        block,
        32,
        22,
        32,
        mask,
        guid,
    )


def make_dump(path, *, silent=False, wrong_format=False, channels=2, other_amplitude=0.0):
    chunks = [
        record(RECORD_KIND_STREAM_START, qpc=100),
        record(
            RECORD_KIND_FORMAT_CHANGE,
            wave(rate=44_100, channels=channels) if wrong_format else wave(channels=channels),
            qpc=100,
        ),
    ]
    position = 0
    frames_per_packet = 480
    packet_count = 350
    amplitude = 10.0 ** (-20.0 / 20.0)
    for packet in range(packet_count):
        samples = bytearray()
        for offset in range(frames_per_packet):
            frame = packet * frames_per_packet + offset
            value = 0.0 if silent else amplitude * math.sin(2.0 * math.pi * 997.0 * frame / 48_000)
            for channel in range(channels):
                channel_value = value if channel < 2 else other_amplitude
                samples.extend(struct.pack("<f", channel_value))
        qpc = 100 + packet * QPC_FREQUENCY // 100
        chunks.append(record(RECORD_KIND_DATA, samples, qpc=qpc, position=position))
        position += len(samples)
    header = DUMP_HEADER.pack(
        DUMP_MAGIC,
        ABI_VERSION,
        DUMP_HEADER.size,
        QPC_FREQUENCY,
        DUMP_FORMAT_VERSION,
        0,
    )
    path.write_bytes(header + b"".join(chunks))


class AppLoopbackCheckTests(unittest.TestCase):
    def run_main(self, path, *extra):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            status = app_loopback_check.main([str(path), *extra])
        return status, json.loads(output.getvalue())

    def test_good_sine_passes(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "good.ssrdump"
            make_dump(path)
            status, result = self.run_main(path)
        self.assertEqual(status, 0, result)
        self.assertTrue(result["ok"])
        self.assertAlmostEqual(result["frequency_hz"], 997.0, delta=2.0)
        self.assertAlmostEqual(result["level_dbfs"], -23.0103, delta=0.1)
        self.assertGreater(result["seconds"], 3.0)
        self.assertAlmostEqual(result["rate_ratio"], 1.0, delta=1e-6)
        self.assertIsNone(result["other_channels_dbfs"])

    def test_multichannel_checks_only_front_left_and_right_for_tone(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "eight.ssrdump"
            make_dump(path, channels=8)
            status, result = self.run_main(path, "--channels", "8")
        self.assertEqual(status, 0, result)
        self.assertTrue(result["ok"])
        self.assertAlmostEqual(result["frequency_hz"], 997.0, delta=2.0)
        self.assertAlmostEqual(result["level_dbfs"], -23.0103, delta=0.1)
        self.assertIsNone(result["other_channels_dbfs"])

    def test_other_channel_leak_fails_except_in_report_only_mode(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "leak.ssrdump"
            make_dump(path, channels=8, other_amplitude=0.01)
            status, result = self.run_main(path, "--channels", "8")
            report_status, reported = self.run_main(
                path, "--channels", "8", "--report-only"
            )
        self.assertEqual(status, 1)
        self.assertTrue(any("channel above index 1" in problem for problem in result["problems"]))
        self.assertAlmostEqual(result["other_channels_dbfs"], -40.0, delta=0.01)
        self.assertEqual(report_status, 0)
        self.assertTrue(reported["ok"])

    def test_silence_fails_but_report_only_returns_success(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "silent.ssrdump"
            make_dump(path, silent=True)
            status, result = self.run_main(path)
            report_status, reported = self.run_main(path, "--report-only")
        self.assertEqual(status, 1)
        self.assertFalse(result["ok"])
        self.assertEqual(report_status, 0)
        self.assertFalse(reported["ok"])
        self.assertIsNone(result["level_dbfs"])

    def test_wrong_format_fails(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "wrong.ssrdump"
            make_dump(path, wrong_format=True)
            status, result = self.run_main(path)
        self.assertEqual(status, 1)
        self.assertFalse(result["ok"])
        self.assertTrue(any("format" in problem for problem in result["problems"]))


if __name__ == "__main__":
    unittest.main()
