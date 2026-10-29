"""Validate a process-loopback .ssrdump recorded by ssr-live."""

import argparse
import json
import math
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from ssr_abi import (
    DUMP_HEADER,
    DUMP_MAGIC,
    RECORD_KIND_DATA,
    RECORD_KIND_FORMAT_CHANGE,
    RECORD_KIND_STREAM_START,
    WAVEFORMATEXTENSIBLE,
    WAVE_FORMAT_EXTENSIBLE,
    records,
)

RATE = 48_000
FLOAT_GUID = bytes.fromhex("0300000000001000800000aa00389b71")


def read_dump(path):
    data = Path(path).read_bytes()
    if len(data) < DUMP_HEADER.size:
        raise ValueError("dump header is truncated")
    magic, _abi, header_size, qpc_frequency, _version, _reserved = DUMP_HEADER.unpack_from(data)
    if magic != DUMP_MAGIC or header_size < DUMP_HEADER.size or header_size > len(data):
        raise ValueError("dump header is invalid")
    return qpc_frequency, list(records(data[header_size:]))


def is_expected_format(payload, channels):
    if len(payload) < WAVEFORMATEXTENSIBLE.size:
        return False
    values = WAVEFORMATEXTENSIBLE.unpack_from(payload)
    channel_mask = values[8]
    return (
        values[0] == WAVE_FORMAT_EXTENSIBLE
        and values[1] == channels
        and values[2] == RATE
        and values[4] == channels * 4
        and values[5] == 32
        and values[6] == 22
        and values[7] == 32
        and channel_mask != 0
        and channel_mask.bit_count() == channels
        and values[9] == FLOAT_GUID
    )


def samples_from(records_in):
    raw = b"".join(
        record["payload"]
        for record in records_in
        if record["endpoint"] == 0 and record["kind"] == RECORD_KIND_DATA
    )
    usable = len(raw) - len(raw) % 4
    return [sample[0] for sample in struct.iter_unpack("<f", raw[:usable])]


def channel_samples(samples, channels, channel, skip_frames=0):
    return samples[skip_frames * channels + channel :: channels]


def rms_dbfs(samples):
    if not samples:
        return None
    power = sum(float(sample) * float(sample) for sample in samples) / len(samples)
    return 10.0 * math.log10(power) if power > 0.0 else None


def goertzel(samples, frequency):
    omega = 2.0 * math.pi * frequency / RATE
    coefficient = 2.0 * math.cos(omega)
    before = 0.0
    previous = 0.0
    for sample in samples:
        current = sample + coefficient * previous - before
        before, previous = previous, current
    return before * before + previous * previous - coefficient * before * previous


def dominant_frequency(mono):
    # At most two seconds are enough for sub-hertz FFT bins and keep CI memory bounded.
    mono = mono[: 2 * RATE]
    if not mono:
        return None
    try:
        from numpy_compat import np

        values = np.asarray(mono, dtype=np.float64)
        values -= values.mean()
        spectrum = np.abs(np.fft.rfft(values))
        if len(spectrum) <= 1:
            return None
        index = int(np.argmax(spectrum[1:])) + 1
        return index * RATE / len(values)
    except (ImportError, ModuleNotFoundError):
        crossings = [
            index
            for index in range(1, len(mono))
            if mono[index - 1] <= 0.0 < mono[index]
        ]
        if len(crossings) < 2:
            return 0.0
        estimate = RATE * (len(crossings) - 1) / (crossings[-1] - crossings[0])
        candidates = [max(1.0, estimate - 10.0 + step * 0.25) for step in range(81)]
        return max(candidates, key=lambda frequency: goertzel(mono, frequency))


def analyze(path, frequency, level_db, min_seconds, tolerance_db, channels=2, report_only=False):
    problems = []
    try:
        qpc_frequency, parsed = read_dump(path)
    except (OSError, ValueError) as error:
        return {
            "ok": False,
            "frequency_hz": None,
            "level_dbfs": None,
            "other_channels_dbfs": None,
            "seconds": 0.0,
            "rate_ratio": None,
            "problems": [str(error)],
        }
    endpoint = [record for record in parsed if record["endpoint"] == 0]
    formats = [record for record in endpoint if record["kind"] == RECORD_KIND_FORMAT_CHANGE]
    if not any(is_expected_format(record["payload"], channels) for record in formats):
        problems.append(f"endpoint 0 has no 48 kHz f32 {channels}-channel valid-mask format")
    if not any(record["kind"] == RECORD_KIND_STREAM_START for record in endpoint):
        problems.append("endpoint 0 has no stream start")
    data = [record for record in endpoint if record["kind"] == RECORD_KIND_DATA]
    bytes_per_frame = channels * 4
    frame_count = sum(record["len"] for record in data) // bytes_per_frame
    seconds = frame_count / RATE
    if seconds < min_seconds:
        problems.append(f"endpoint 0 has {seconds:.3f} seconds of data; need {min_seconds:.3f}")
    samples = samples_from(endpoint)
    available_frames = len(samples) // channels
    skipped_frames = min(available_frames, int(0.5 * RATE))
    tone_channels = min(channels, 2)
    measured_tone = []
    for frame in range(skipped_frames, available_frames):
        first = frame * channels
        measured_tone.extend(samples[first : first + tone_channels])
    level_dbfs = rms_dbfs(measured_tone)
    frequency_hz = dominant_frequency(channel_samples(samples, channels, 0, skipped_frames))
    other_levels = [
        rms_dbfs(channel_samples(samples, channels, channel, skipped_frames))
        for channel in range(2, channels)
    ]
    finite_other_levels = [level for level in other_levels if level is not None]
    other_channels_dbfs = max(finite_other_levels, default=None)
    if frequency_hz is None or abs(frequency_hz - frequency) > 2.0:
        problems.append(f"dominant frequency is {frequency_hz}; expected {frequency} Hz within 2 Hz")
    expected_rms = level_db - 3.01029995664
    if level_dbfs is None or not math.isfinite(level_dbfs) or abs(level_dbfs - expected_rms) > tolerance_db:
        problems.append(
            f"RMS level is {level_dbfs}; expected {expected_rms:.2f} dBFS within {tolerance_db} dB"
        )
    if not report_only and other_channels_dbfs is not None and other_channels_dbfs > -60.0:
        problems.append(
            f"loudest channel above index 1 is {other_channels_dbfs:.2f} dBFS; need at most -60 dBFS"
        )
    rate_ratio = None
    if len(data) >= 2 and qpc_frequency > 0 and data[-1]["qpc"] > data[0]["qpc"]:
        expected_frames = (data[-1]["qpc"] - data[0]["qpc"]) / qpc_frequency * RATE
        last_frames = data[-1]["len"] // bytes_per_frame
        spanned_frames = frame_count - last_frames
        if expected_frames > 0.0:
            rate_ratio = spanned_frames / expected_frames
    return {
        "ok": not problems,
        "frequency_hz": frequency_hz,
        "level_dbfs": level_dbfs,
        "other_channels_dbfs": other_channels_dbfs,
        "seconds": seconds,
        "rate_ratio": rate_ratio,
        "problems": problems,
    }


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dump")
    parser.add_argument("--frequency", type=float, default=997.0)
    parser.add_argument("--level-db", type=float, default=-20.0)
    parser.add_argument("--min-seconds", type=float, default=3.0)
    parser.add_argument("--tolerance-db", type=float, default=3.0)
    parser.add_argument("--channels", type=int, choices=range(1, 9), default=2)
    parser.add_argument("--report-only", action="store_true")
    parser.add_argument("--summary")
    args = parser.parse_args(argv)
    result = analyze(
        args.dump,
        args.frequency,
        args.level_db,
        args.min_seconds,
        args.tolerance_db,
        args.channels,
        args.report_only,
    )
    print(json.dumps(result, separators=(",", ":"), allow_nan=False))
    if args.summary:
        with open(args.summary, "a", encoding="utf-8") as summary:
            summary.write(
                "- Process loopback: {status}; {seconds:.3f} s, {frequency} Hz, "
                "{level} dBFS, other channels {other} dBFS, rate ratio {ratio}\n".format(
                    status="pass" if result["ok"] else "fail",
                    seconds=result["seconds"],
                    frequency=result["frequency_hz"],
                    level=result["level_dbfs"],
                    other=result["other_channels_dbfs"],
                    ratio=result["rate_ratio"],
                )
            )
    return 0 if result["ok"] or args.report_only else 1


if __name__ == "__main__":
    sys.exit(main())
