"""Exercise driver endpoint-name reload and restore the registry afterward.

The driver reads Parameters\\EndpointName0..3 when the device starts and puts
each name in its topology pin's KSJACK_SINK_INFORMATION.SinkDescription; the
endpoint builder copies that into the endpoint's description only when it
first creates the endpoint. Windows keeps an existing endpoint's description
across device restarts (a rename in the Sound control panel lives in the same
property), so after a restart the check reads the sink descriptions straight
from the KS topology filters (KsProbe.cs) and requires the endpoints to stay
active with their descriptions unchanged.

    py endpoint_name_check.py --ssr-live ssr-live.exe --summary endpoint-name.json
"""

import argparse
import json
from pathlib import Path
import re
import subprocess
import sys
import time

from endpoint_format_check import (
    FALLBACK_NAMES,
    REGISTRY_PATH,
    REG_SZ,
    active_problems,
    effective_name,
    json_registry_snapshot,
    parse_host_formats,
)
from endpoints import ENDPOINT_NUMBERS

SHORT_NAME = "SSR name check 1"
LONG_NAME = "SSR name check 2 " + "x" * 16
POLL_SECONDS = 60
# KsProbe.Run prints one line per jack property; the topology filters' reference
# strings TopologyHdmi0..2 are the endpoint numbers (endpoints.ps1).
SINK_LINE = re.compile(
    r"^ks topologyhdmi([0-2]) pin \d+ JACK_SINK_INFO: .*? SinkDescription=(.*) PortId=LUID\(",
    re.IGNORECASE)


class PollError(RuntimeError):
    def __init__(self, message, rows, document, sinks, attempts):
        super().__init__(message)
        self.rows = rows
        self.document = document
        self.sinks = sinks
        self.attempts = attempts


def parse_sink_lines(lines):
    """Sink description per endpoint number from KsProbe.Run's output lines."""
    sinks = {}
    for line in lines:
        match = SINK_LINE.match(line.strip())
        if not match:
            continue
        endpoint, name = int(match.group(1)), match.group(2)
        if sinks.get(endpoint, name) != name:
            raise ValueError(f"endpoint {endpoint}: two different sink descriptions")
        sinks[endpoint] = name
    return sinks


def _descriptions(rows):
    return {row.get("endpoint"): row.get("device_desc")
            for row in rows if row.get("state") == "active"}


def _unchanged_problems(rows, initial):
    problems = []
    now = _descriptions(rows)
    for endpoint in (1, 2):
        if endpoint in initial and endpoint in now and now[endpoint] != initial[endpoint]:
            problems.append(
                f"endpoint {endpoint}: device_desc changed from {initial[endpoint]!r} "
                f"to {now[endpoint]!r}")
    return problems


def changed_name_problems(rows, sinks, initial):
    expected = {1: SHORT_NAME, 2: FALLBACK_NAMES[2]}
    problems = active_problems(rows, 0x7)
    for endpoint, name in expected.items():
        if sinks.get(endpoint) != name:
            problems.append(
                f"endpoint {endpoint}: sink description {sinks.get(endpoint)!r}, "
                f"expected {name!r} after restart")
    return problems + _unchanged_problems(rows, initial)


def restored_name_problems(rows, sinks, original, initial):
    problems = active_problems(rows, 0x7)
    for endpoint in (1, 2):
        expected = effective_name(original[endpoint], FALLBACK_NAMES[endpoint])
        if sinks.get(endpoint) != expected:
            problems.append(
                f"endpoint {endpoint}: restored sink description {sinks.get(endpoint)!r}, "
                f"expected {expected!r}")
    return problems + _unchanged_problems(rows, initial)


def snapshot_values():
    try:
        import winreg
    except ImportError as error:
        raise RuntimeError("the Windows registry is unavailable") from error
    try:
        key = winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, REGISTRY_PATH, 0, winreg.KEY_READ)
    except OSError as error:
        raise RuntimeError(f"cannot open HKLM\\{REGISTRY_PATH}: {error}") from error
    result = {}
    with key:
        for endpoint in (1, 2):
            name = f"EndpointName{endpoint}"
            try:
                value, value_type = winreg.QueryValueEx(key, name)
                result[endpoint] = {"present": True, "type": value_type, "value": value}
            except FileNotFoundError:
                result[endpoint] = {"present": False, "type": None, "value": None}
            except OSError as error:
                raise RuntimeError(f"cannot read {name}: {error}") from error
    return result


def write_values(values):
    """Write exact snapshots; callers preserve absence and registry type."""
    try:
        import winreg
    except ImportError as error:
        raise RuntimeError("the Windows registry is unavailable") from error
    access = winreg.KEY_QUERY_VALUE | winreg.KEY_SET_VALUE
    try:
        key = winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, REGISTRY_PATH, 0, access)
    except PermissionError as error:
        raise RuntimeError(
            f"administrator access is required for HKLM\\{REGISTRY_PATH}: {error}") from error
    except OSError as error:
        raise RuntimeError(f"cannot open HKLM\\{REGISTRY_PATH} for writing: {error}") from error
    with key:
        for endpoint, snapshot in values.items():
            name = f"EndpointName{endpoint}"
            try:
                if snapshot["present"]:
                    winreg.SetValueEx(key, name, 0, snapshot["type"], snapshot["value"])
                else:
                    try:
                        winreg.DeleteValue(key, name)
                    except FileNotFoundError:
                        pass
            except (OSError, TypeError, ValueError) as error:
                raise RuntimeError(f"cannot write {name}: {error}") from error


def restore_values(values, write=None):
    """Best-effort exact restoration, reporting each endpoint independently."""
    if write is None:
        write = write_values
    errors = []
    for endpoint in (1, 2):
        try:
            write({endpoint: values[endpoint]})
        except (RuntimeError, OSError, TypeError, ValueError) as error:
            errors.append(f"endpoint {endpoint}: {error}")
    return errors


def run_json(command, timeout=120):
    try:
        completed = subprocess.run(
            [str(part) for part in command], check=False, capture_output=True, text=True,
            encoding="utf-8", timeout=timeout)
    except (OSError, subprocess.SubprocessError) as error:
        raise RuntimeError(f"cannot run {' '.join(map(str, command))}: {error}") from error
    if completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip() or "no diagnostic output"
        raise RuntimeError(
            f"{' '.join(map(str, command))} exited with {completed.returncode}: {detail}")
    if not completed.stdout.strip():
        return None
    try:
        return json.loads(completed.stdout.lstrip("﻿"))
    except json.JSONDecodeError as error:
        raise RuntimeError(f"{' '.join(map(str, command))} returned invalid JSON: {error}") from error


def restart_device(ssr_live):
    # The installed ssr-setup is selected beside ssr-live when package-dir is
    # that directory.  This is the same elevated mechanism used by host setup.
    command = [ssr_live, "host", "setup", "restart-device",
               "--package-dir", str(Path(ssr_live).resolve().parent)]
    run_json(command)
    return [str(part) for part in command]


def read_formats(ssr_live):
    document = run_json([ssr_live, "host", "formats"], timeout=30)
    return parse_host_formats(document), document


def _quote(path):
    return "'" + str(path).replace("'", "''") + "'"


def read_sinks(timeout=120):
    """Run KsProbe.Run on every device with the package's hardware ID."""
    here = Path(__file__).resolve().parent
    script = (
        "$ErrorActionPreference = 'Stop'; "
        "[Console]::OutputEncoding = New-Object Text.UTF8Encoding($false); "
        ". " + _quote(here / "package-identities.ps1") + "; "
        "Add-Type -TypeDefinition (Get-Content -Raw " + _quote(here / "KsProbe.cs") + "); "
        "$ids = @($SpatialPackage | ForEach-Object HardwareId); "
        "Get-PnpDevice -PresentOnly | Where-Object { @($_.HardwareID | Where-Object { $_ -in $ids }).Count -gt 0 } | "
        "ForEach-Object { [KsProbe]::Run(($_.InstanceId -replace '\\\\', '#').ToLowerInvariant()) }")
    try:
        completed = subprocess.run(
            ["powershell.exe", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass",
             "-Command", script],
            check=False, capture_output=True, text=True, encoding="utf-8", timeout=timeout)
    except (OSError, subprocess.SubprocessError) as error:
        raise RuntimeError(f"cannot run KsProbe: {error}") from error
    if completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip() or "no diagnostic output"
        raise RuntimeError(f"KsProbe exited with {completed.returncode}: {detail}")
    return parse_sink_lines(completed.stdout.splitlines())


def poll_names(ssr_live, judge, timeout=POLL_SECONDS, interval=1.0):
    deadline = time.monotonic() + timeout
    attempts, last_rows, last_document, last_sinks, last_error = 0, [], None, {}, None
    while time.monotonic() < deadline:
        attempts += 1
        try:
            rows, document = read_formats(ssr_live)
            last_rows, last_document = rows, document
            sinks = read_sinks()
            last_sinks, last_error = sinks, None
            problems = judge(rows, sinks)
            if not problems:
                return rows, document, sinks, attempts
            last_error = "; ".join(problems)
        except (RuntimeError, ValueError) as error:
            last_error = str(error)
        time.sleep(interval)
    detail = last_error or "no endpoint data"
    raise PollError(
        f"endpoints did not reach the expected names within {timeout} seconds: {detail}",
        last_rows, last_document, last_sinks, attempts)


def phase(rows, document, sinks=None, attempts=None):
    return {
        "attempts": attempts,
        "endpoints": [
            {"endpoint": row.get("endpoint"), "state": row.get("state"),
             "device_desc": row.get("device_desc"), "friendly_name": row.get("friendly_name")}
            for row in rows
        ],
        "sink_descriptions": {str(endpoint): name for endpoint, name in sorted((sinks or {}).items())},
        "host_formats": document,
    }


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--ssr-live", required=True, type=Path)
    parser.add_argument("--summary", required=True, type=Path)
    args = parser.parse_args(argv)

    empty_phase = phase([], None, {}, 0)
    result = {
        "phases": {"changed": dict(empty_phase), "restored": dict(empty_phase)},
        "initial_device_desc": {},
        "original_registry": {},
        "problems": [],
    }
    problems = result["problems"]
    original = None
    initial = {}
    try:
        rows, _ = read_formats(args.ssr_live)
        initial = _descriptions(rows)
        result["initial_device_desc"] = {str(endpoint): name for endpoint, name in sorted(initial.items())}
        original = snapshot_values()
        result["original_registry"] = json_registry_snapshot(original)
        write_values({
            1: {"present": True, "type": REG_SZ, "value": SHORT_NAME},
            2: {"present": True, "type": REG_SZ, "value": LONG_NAME},
        })
        result["restart_changed"] = restart_device(args.ssr_live)
        rows, document, sinks, attempts = poll_names(
            args.ssr_live, lambda value, found: changed_name_problems(value, found, initial))
        result["phases"]["changed"] = phase(rows, document, sinks, attempts)
    except PollError as error:
        result["phases"]["changed"] = phase(error.rows, error.document, error.sinks, error.attempts)
        problems.append(f"change phase: {error}")
    except (RuntimeError, ValueError) as error:
        problems.append(f"change phase: {error}")
    finally:
        if original is not None:
            restoration_errors = restore_values(original)
            problems.extend(f"restoration phase: {error}" for error in restoration_errors)
            try:
                result["restart_restored"] = restart_device(args.ssr_live)
                if not restoration_errors:
                    rows, document, sinks, attempts = poll_names(
                        args.ssr_live,
                        lambda value, found: restored_name_problems(value, found, original, initial))
                    result["phases"]["restored"] = phase(rows, document, sinks, attempts)
            except PollError as error:
                result["phases"]["restored"] = phase(
                    error.rows, error.document, error.sinks, error.attempts)
                problems.append(f"restoration phase: {error}")
            except RuntimeError as error:
                problems.append(f"restoration phase: {error}")

    result["passed"] = not problems
    text = json.dumps(result, indent=2, ensure_ascii=True)
    try:
        args.summary.write_text(text, encoding="utf-8")
    except OSError as error:
        print(f"cannot write summary {args.summary}: {error}", file=sys.stderr)
        return 1
    print(text)
    for problem in problems:
        print(problem, file=sys.stderr)
    return 1 if problems else 0


if __name__ == "__main__":
    raise SystemExit(main())
