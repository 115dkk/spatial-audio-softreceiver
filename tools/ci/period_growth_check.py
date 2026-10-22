"""Checks that exclusive output grows its period on a device that cannot hold it.

VB-CABLE restarts over and over at its 3 ms minimum exclusive period and at
6 ms, and keeps 10 ms and longer (ADR 0026). ``ssr-live play`` without
``--period-ms`` opens the device minimum, so the engine must see restarts that
keep coming and double the period until it holds, instead of restarting ten
times a second for good (ADR 0019, 2026-10-22 addendum).

    py period_growth_check.py report.json

The report must show the restarts that start the growth, a final period of
at least MIN_PERIOD_FRAMES and at most MAX_PERIOD_FRAMES, at most MAX_REOPENS
reopens, and no more restarts than the reopens explain. Prints ``pass`` or
one problem per line; exits 0 or 1.
"""

import argparse
import json
import sys
from pathlib import Path

# The engine's storm: this many restarts within two seconds (RestartStorm).
STORM_RESTARTS = 3
# 10 ms at 48 kHz, the shortest period VB-CABLE keeps.
MIN_PERIOD_FRAMES = 480
# The engine never grows past 20 ms.
MAX_PERIOD_FRAMES = 960
# 3 -> 6 -> 12 -> 20 ms at most.
MAX_REOPENS = 3
# Restarts once the period holds: a late wake on a loaded runner.
SETTLED_RESTARTS = 2


def _count(value):
    return isinstance(value, int) and not isinstance(value, bool) and value >= 0


def growth_problems(report):
    """Problems with one ``ssr-live play --report`` JSON object."""
    if not isinstance(report, dict):
        return ["the report must be an object"]
    fields = ("output_restarts", "output_reopens", "output_period_frames")
    if not all(_count(report.get(field)) for field in fields):
        return [f"the report lacks {', '.join(fields)} as counts"]
    if report.get("output_mode") != "exclusive":
        return [f"the output ran {report.get('output_mode')!r}, not exclusive"]
    restarts = report["output_restarts"]
    reopens = report["output_reopens"]
    period = report["output_period_frames"]
    problems = []
    if restarts < STORM_RESTARTS:
        problems.append(
            f"{restarts} restarts: the cable held its minimum period, unlike ADR 0026's "
            "measurements, so this run did not exercise the growth")
    if not MIN_PERIOD_FRAMES <= period <= MAX_PERIOD_FRAMES:
        problems.append(
            f"the output ended at {period} frames a period, outside "
            f"{MIN_PERIOD_FRAMES}..={MAX_PERIOD_FRAMES}")
    if not 1 <= reopens <= MAX_REOPENS:
        problems.append(f"{reopens} reopens, expected 1..={MAX_REOPENS}")
    elif restarts > STORM_RESTARTS * reopens + SETTLED_RESTARTS:
        problems.append(
            f"{restarts} restarts for {reopens} reopens: the restarts went on after the "
            "period grew")
    return problems


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("report", type=Path)
    args = parser.parse_args(argv)
    try:
        report = json.loads(args.report.read_text(encoding="utf-8"))
    except (OSError, ValueError) as error:
        print(f"cannot read {args.report}: {error}")
        return 1
    problems = growth_problems(report)
    for problem in problems:
        print(problem)
    if not problems:
        print("pass")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
