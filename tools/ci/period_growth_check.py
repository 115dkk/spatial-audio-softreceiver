"""Checks that exclusive output grows its period on a device that cannot hold it.

VB-CABLE restarts over and over at its 3 ms minimum exclusive period. At 6 ms
it stops restarting but takes only 0.6 of the nominal rate, and on some
runners it does the same at 12 ms; 10 and 20 ms hold (ADR 0026). ``ssr-live
play`` without ``--period-ms`` opens the device minimum, so the engine must
double the period on restarts that keep coming and on a cadence that stays
slow until one holds, and drop the backlog the slow periods left, instead of
restarting ten times a second or playing seconds late for good (ADR 0019,
2026-10-22 addenda).

    py period_growth_check.py report.json

The report must show a growth (a reopen, which restarts that keep coming or a
cadence that stays slow start; a run without either held the minimum), a final
period of at least MIN_PERIOD_FRAMES and at most MAX_PERIOD_FRAMES, at most
MAX_REOPENS reopens, no more restarts than the reopens explain, a last cadence window
within the guard's range, and a latency since the last reopen of at most
MAX_LATENCY_P99_MS. Prints ``pass`` or one problem per line; exits 0 or 1.
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
# The cadence guard's range (ADR 0012): outside it the period did not hold.
# The engine reports 1.0 from a reopen until the new period's first 2 s
# window ends, so a run that stops sooner is judged on its latency alone.
CADENCE_RANGE = (0.9, 1.1)
# The live path sits near 70 ms on these runners; seconds mean the backlog of
# the slow periods stayed after the reopen. The report's latency counts only
# since the last reopen.
MAX_LATENCY_P99_MS = 250.0


def _count(value):
    return isinstance(value, int) and not isinstance(value, bool) and value >= 0


def _number(value):
    return isinstance(value, (int, float)) and not isinstance(value, bool) and value >= 0


def growth_problems(report):
    """Problems with one ``ssr-live play --report`` JSON object."""
    if not isinstance(report, dict):
        return ["the report must be an object"]
    fields = ("output_restarts", "output_reopens", "output_period_frames")
    if not all(_count(report.get(field)) for field in fields):
        return [f"the report lacks {', '.join(fields)} as counts"]
    measures = ("output_cadence_ratio", "latency_p99_ms")
    if not all(_number(report.get(field)) for field in measures):
        return [f"the report lacks {', '.join(measures)} as numbers"]
    if report.get("output_mode") != "exclusive":
        return [f"the output ran {report.get('output_mode')!r}, not exclusive"]
    restarts = report["output_restarts"]
    reopens = report["output_reopens"]
    period = report["output_period_frames"]
    problems = []
    # Either trigger starts the growth: windows-2022 at #92 restarted twice,
    # then its cadence ran at 0.301 and 0.603 and grew it to 6 and 12 ms.
    if restarts < STORM_RESTARTS and reopens == 0:
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
    ratio = report["output_cadence_ratio"]
    low, high = CADENCE_RANGE
    if not low <= ratio <= high:
        problems.append(
            f"the last cadence window ran at {ratio:.3f} of the nominal rate: the final "
            "period did not hold")
    latency = report["latency_p99_ms"]
    if latency > MAX_LATENCY_P99_MS:
        problems.append(
            f"latency p99 {latency:.0f} ms since the last reopen, above "
            f"{MAX_LATENCY_P99_MS:.0f} ms: the backlog of the slow periods stayed")
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
