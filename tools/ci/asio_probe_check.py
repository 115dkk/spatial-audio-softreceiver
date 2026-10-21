"""Checks an `asio_probe` run against the ASIO entrance (ADR 0025).

The probe loads ssrasio.dll the way a DAW does (DllGetClassObject, IASIO),
asks for the buffers, starts, counts the callbacks for the given seconds and
stops. Its JSON already says whether the call order and the callback counters
were right (`accepted`); this adds what the run around it expects:

* `--engine attached`: an engine (ssr-live) took the DAW's frames: at least
  `--min-pace` of what the callbacks produced was loaded, and no more frames
  completed than were loaded. The counters restart at every new generation,
  so an engine that keeps restarting its output leaves only the last
  generation's few frames here.
* `--engine absent`: nothing took them, so no frame was loaded.
* `--no-clock-fault`: the callbacks ran on the output device's clock the
  whole time, never on the DLL's stand-in clock.
* `--engine-report`: ssr-live's report for the same run never restarted its
  output. A restart starts a new generation, which drops the DAW's frames.

Whatever the engine does, the callbacks keep the pace the sample rate owes:
seconds × rate ÷ buffer frames, at least `--min-pace` of it (the run's start
and stop eat a little) and no more than MAX_PACE of it (a stand-in clock that
spins instead of waiting would overshoot).

    py asio_probe_check.py probe.json --engine attached --no-clock-fault --engine-report live.json

Prints `pass` or one problem per line; exits 0 or 1.
"""

import argparse
import json
import sys
from pathlib import Path

MAX_PACE = 1.25


def _count(value):
    return isinstance(value, int) and not isinstance(value, bool) and value >= 0


def probe_problems(report, engine="any", no_clock_fault=False, min_pace=0.8):
    """Problems with the probe's JSON report."""
    if not isinstance(report, dict):
        return ["the probe report must be an object"]
    problems = []
    if report.get("accepted") is not True:
        problems.append(f"the probe did not accept the DLL: call order {report.get('call_order')}")
    seconds, rate, frames = report.get("seconds"), report.get("sample_rate"), report.get("buffer_frames")
    callbacks = report.get("callbacks")
    if not (_count(seconds) and seconds > 0 and isinstance(rate, (int, float)) and rate > 0
            and _count(frames) and frames > 0 and _count(callbacks)):
        problems.append("the report lacks seconds, sample_rate, buffer_frames or callbacks")
    else:
        owed = seconds * rate / frames
        if callbacks < min_pace * owed:
            problems.append(f"{callbacks} callbacks in {seconds} s at {rate:g} Hz and {frames} frames; "
                            f"the clock owed {owed:.0f} (at least {min_pace:.0%} of it)")
        elif callbacks > MAX_PACE * owed:
            problems.append(f"{callbacks} callbacks in {seconds} s at {rate:g} Hz and {frames} frames; "
                            f"the clock owed {owed:.0f}, so it ran faster than the rate")
    loaded, completed = report.get("loaded_frames"), report.get("completed_frames")
    if engine == "attached":
        if not (_count(loaded) and loaded > 0 and _count(completed) and completed > 0):
            problems.append(f"no engine took the frames: loaded {loaded}, completed {completed}")
        elif completed > loaded:
            problems.append(f"{completed} frames completed but only {loaded} were loaded")
        elif _count(callbacks) and _count(frames) and loaded < min_pace * callbacks * frames:
            problems.append(f"the engine took {loaded} of the {callbacks * frames} frames the callbacks "
                            f"produced (at least {min_pace:.0%} of them)")
    elif engine == "absent" and loaded not in (None, 0):
        problems.append(f"{loaded} frames were loaded with no engine running")
    if no_clock_fault and report.get("clock_fault_observed") is not False:
        problems.append("the callbacks fell back to the DLL's stand-in clock")
    return problems


def engine_report_problems(report):
    """Problems with ssr-live's report for the run the probe attached to."""
    if not isinstance(report, dict):
        return ["the engine report must be an object"]
    restarts = report.get("output_restarts")
    if not _count(restarts):
        return ["the engine report lacks output_restarts"]
    if restarts:
        return [f"the engine restarted its output {restarts} times; each restart starts a new "
                "generation and drops the DAW's frames"]
    return []


def _read(path):
    try:
        return json.loads(path.read_text(encoding="utf-8")), None
    except (OSError, ValueError) as error:
        return None, f"cannot read {path}: {error}"


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("report", type=Path)
    parser.add_argument("--engine", choices=["attached", "absent", "any"], default="any")
    parser.add_argument("--no-clock-fault", action="store_true")
    parser.add_argument("--min-pace", type=float, default=0.8)
    parser.add_argument("--engine-report", type=Path)
    args = parser.parse_args(argv)
    report, error = _read(args.report)
    if error:
        print(error)
        return 1
    problems = probe_problems(report, args.engine, args.no_clock_fault, args.min_pace)
    if args.engine_report:
        engine, error = _read(args.engine_report)
        problems += [error] if error else engine_report_problems(engine)
    for problem in problems:
        print(problem)
    if not problems:
        print("pass")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
