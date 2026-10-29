"""Checks our side's latency in a `ssr-live play --report` run (ADR 0044).

The handoff latency runs from a block's source QPC to the moment its first
frame went into the output device's buffer, so the device's own buffer is
not in it. From the code it lies within ten milliseconds (one block) below

    9 ms (the block's other nine 1 ms records) + 10 ms (the target)
    + min(P, 10 ms) (the period the target keeps) + P (the ring level runs
    between the target and the target plus one period, as the output reads
    a period at a time) + 2.7 ms (the ASRC, when the clock is not trimmed)

for an output period P. Where in that range a run falls depends on the phase
between the 10 ms blocks and the output periods, which stays put for a whole
run once the clocks agree. A run fails when its handoff p50 is more than
MARGIN_MS above the top of the range, which is how a change that adds latency
(like the v0.26.0 crossing ring) shows up before it merges. A p50 more than
NOTE_MS outside the range is noted, not failed: then the model in ADR 0044 is
out of date and gets an amendment.

    py latency_bench_check.py report.json [--summary SUMMARY.md --name NAME]

Prints `pass` or one problem per line, then any notes; exits 0 or 1.
"""

import argparse
import json
import sys
from pathlib import Path

# The source QPC is the end of the record that holds the block's first frame,
# so the block is complete nine 1 ms records later.
BLOCK_REST_MS = 9.0
BLOCK_MS = 10.0
TARGET_MS = 10.0
# The target keeps one output period on top, at most 10 ms (TargetLatency).
KEPT_PERIOD_CAP_MS = 10.0
# Half the ASRC's 256-tap sinc at 48 kHz.
ASRC_MS = 2.7
MARGIN_MS = 5.0
NOTE_MS = 1.0


def _number(value):
    return isinstance(value, (int, float)) and not isinstance(value, bool)


def handoff_range_ms(period_ms, asrc):
    """(lowest, highest) handoff latency p50 the code allows for a period."""
    highest = BLOCK_REST_MS + TARGET_MS + min(period_ms, KEPT_PERIOD_CAP_MS) + period_ms
    if asrc:
        highest += ASRC_MS
    return highest - BLOCK_MS, highest


def judge(report):
    """(problems, notes, figures) for one report object."""
    if not isinstance(report, dict):
        return ["the report must be an object"], [], None
    needed = ("handoff_latency_p50_ms", "handoff_latency_p99_ms", "device_latency_p50_ms",
              "latency_p50_ms", "output_period_frames", "output_sample_rate")
    missing = [field for field in needed if not _number(report.get(field))]
    if missing:
        return [f"the report lacks {', '.join(missing)} as numbers"], [], None
    if report.get("output_mode") != "exclusive":
        return [f"the output ran {report.get('output_mode')!r}, not exclusive"], [], None
    if report["output_sample_rate"] <= 0 or report["output_period_frames"] <= 0:
        return ["the report has no output period"], [], None
    period_ms = report["output_period_frames"] * 1000.0 / report["output_sample_rate"]
    asrc = report.get("clock_strategy") != "trim"
    lowest, highest = handoff_range_ms(period_ms, asrc)
    limit = highest + MARGIN_MS
    handoff = report["handoff_latency_p50_ms"]
    device = report["device_latency_p50_ms"]
    figures = {"period_ms": period_ms, "lowest_ms": lowest, "highest_ms": highest,
               "limit_ms": limit, "handoff_p50_ms": handoff,
               "handoff_p99_ms": report["handoff_latency_p99_ms"],
               "device_p50_ms": device, "total_p50_ms": report["latency_p50_ms"]}
    problems = []
    if report.get("output_reopens", 0) not in (0, None):
        problems.append(
            f"the output reopened {report['output_reopens']} times: the period changed "
            "under the measurement")
    if handoff <= 0:
        problems.append(f"handoff latency p50 {handoff:.1f} ms: the handoff time is not "
                        "after the source time")
    if device < 0:
        problems.append(f"device latency p50 {device:.1f} ms: the device played a frame "
                        "before it was handed over")
    if handoff > limit:
        problems.append(
            f"handoff latency p50 {handoff:.1f} ms at a {period_ms:g} ms period, above "
            f"{limit:.1f} ms (the range tops out at {highest:.1f}, + {MARGIN_MS:g})")
    notes = []
    if not problems and not lowest - NOTE_MS <= handoff <= highest + NOTE_MS:
        notes.append(
            f"note: handoff latency p50 {handoff:.1f} ms is outside the "
            f"{lowest:.1f}-{highest:.1f} ms the code allows; amend the model in ADR 0044")
    return problems, notes, figures


def summary_row(name, problems, figures):
    if figures is None:
        return f"| {name} | | | | | | fail |"
    verdict = "fail" if problems else "pass"
    return (f"| {name} | {figures['period_ms']:g} ms | {figures['handoff_p50_ms']:.1f} / "
            f"{figures['handoff_p99_ms']:.1f} ms | {figures['lowest_ms']:.1f}-"
            f"{figures['highest_ms']:.1f} ms | {figures['limit_ms']:.1f} ms | "
            f"{figures['device_p50_ms']:.1f} ms | {figures['total_p50_ms']:.1f} ms | "
            f"{verdict} |")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("report", type=Path)
    parser.add_argument("--summary", type=Path, help="append a table row to this file")
    parser.add_argument("--name", default="run", help="the row's name")
    args = parser.parse_args(argv)
    try:
        report = json.loads(args.report.read_text(encoding="utf-8"))
    except (OSError, ValueError) as error:
        print(f"cannot read {args.report}: {error}")
        return 1
    problems, notes, figures = judge(report)
    for line in problems:
        print(line)
    if not problems:
        print("pass")
    for line in notes:
        print(line)
    if args.summary is not None:
        with args.summary.open("a", encoding="utf-8") as summary:
            summary.write(summary_row(args.name, problems, figures) + "\n")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
