"""Checks a `ssr-live ks-probe` run (ADR 0037).

The probe disables a render endpoint, opens its KS pin, plays a 1 kHz sine
at -12 dBFS on every channel, closes the pin and enables the endpoint again.
This checks the probe's JSON and, when given the far end of a cable recorded
while the probe played, that the tone came through: on every channel, at
1 kHz, within a dB and a half of -12 dBFS.

    py ks_probe_check.py probe.json --interface standard --captured cable.wav

Prints `pass` or one problem per line; exits 0 or 1.
"""

import argparse
import json
import math
import sys
from pathlib import Path

TONE_HZ = 1000.0
TONE_DBFS = -12.0
TONE_TOLERANCE_DB = 1.5
# The middle of the sounding part is judged, away from the start and end ramps.
MIN_SOUNDING_S = 0.5


def probe_problems(report, interface=None):
    """Problems with the probe's JSON report."""
    if not isinstance(report, dict):
        return ["the probe report must be an object"]
    problems = []
    if report.get("opened") is not True:
        problems.append(f"the KS pin did not open: {report.get('error')}")
    if report.get("endpoint_restored") is not True:
        problems.append("the render endpoint was not active again after the probe")
    if report.get("error") is not None and report.get("opened") is True:
        problems.append(f"the probe failed after opening: {report['error']}")
    if interface is not None and report.get("opened") is True and report.get("interface") != interface:
        problems.append(f"the pin streamed through {report.get('interface')}, expected {interface}")
    return problems


def tone_problems(channels, rate):
    """Problems with a recording (one list of floats per channel) of the tone."""
    problems = []
    if not channels:
        return ["the recording has no channels"]
    for index, samples in enumerate(channels):
        sounding = [i for i, value in enumerate(samples) if abs(value) > 0.01]
        if not sounding or (sounding[-1] - sounding[0]) < MIN_SOUNDING_S * rate:
            problems.append(f"channel {index}: no tone of at least {MIN_SOUNDING_S} s")
            continue
        start, end = sounding[0], sounding[-1]
        middle = samples[start + (end - start) // 4: end - (end - start) // 4]
        rms = math.sqrt(sum(value * value for value in middle) / len(middle))
        level = 20 * math.log10(rms * math.sqrt(2)) if rms > 0 else -math.inf
        if abs(level - TONE_DBFS) > TONE_TOLERANCE_DB:
            problems.append(f"channel {index}: tone at {level:.1f} dBFS, expected {TONE_DBFS} ± {TONE_TOLERANCE_DB}")
        crossings = sum(1 for a, b in zip(middle, middle[1:]) if a < 0 <= b)
        frequency = crossings * rate / len(middle)
        if abs(frequency - TONE_HZ) > 0.02 * TONE_HZ:
            problems.append(f"channel {index}: tone at {frequency:.0f} Hz, expected {TONE_HZ:.0f}")
    return problems


def read_wav(path):
    import soundfile as sf

    data, rate = sf.read(path, dtype="float32", always_2d=True)
    return [data[:, c].tolist() for c in range(data.shape[1])], rate


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("report")
    parser.add_argument("--interface", choices=("wavert", "standard"))
    parser.add_argument("--captured", help="a WAV of the cable's far end recorded during the probe")
    args = parser.parse_args(argv)
    try:
        report = json.loads(Path(args.report).read_text(encoding="utf-8-sig"))
        found = probe_problems(report, args.interface)
        if args.captured:
            channels, rate = read_wav(args.captured)
            found += tone_problems(channels, rate)
    except Exception as error:
        found = [str(error)]
    for problem in found:
        print(problem)
    if found:
        return 1
    print("pass")
    return 0


if __name__ == "__main__":
    sys.exit(main())
