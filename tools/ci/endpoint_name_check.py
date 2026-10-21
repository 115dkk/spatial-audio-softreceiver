"""Exercise driver endpoint-name reload and restore the registry afterward.

    py endpoint_name_check.py --ssr-live ssr-live.exe --summary endpoint-name.json
"""

import argparse
import json
from pathlib import Path
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


class PollError(RuntimeError):
    def __init__(self, message, rows, document, attempts):
        super().__init__(message)
        self.rows = rows
        self.document = document
        self.attempts = attempts


def changed_name_problems(rows):
    expected = {1: SHORT_NAME, 2: FALLBACK_NAMES[2]}
    problems = active_problems(rows, 0x7)
    for endpoint, name in expected.items():
        matches = [row for row in rows
                   if row.get("endpoint") == endpoint and row.get("state") == "active"]
        if len(matches) == 1 and matches[0].get("device_desc") != name:
            problems.append(
                f"endpoint {endpoint}: device_desc {matches[0].get('device_desc')!r}, "
                f"expected {name!r} after restart")
    return problems


def restored_name_problems(rows, original):
    problems = active_problems(rows, 0x7)
    for endpoint in (1, 2):
        matches = [row for row in rows
                   if row.get("endpoint") == endpoint and row.get("state") == "active"]
        expected = effective_name(original[endpoint], FALLBACK_NAMES[endpoint])
        if len(matches) == 1 and matches[0].get("device_desc") != expected:
            problems.append(
                f"endpoint {endpoint}: restored device_desc {matches[0].get('device_desc')!r}, "
                f"expected {expected!r}")
    return problems


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


def poll_formats(ssr_live, judge, timeout=POLL_SECONDS, interval=1.0):
    deadline = time.monotonic() + timeout
    attempts, last_rows, last_document, last_error = 0, [], None, None
    while time.monotonic() < deadline:
        attempts += 1
        try:
            document = run_json([ssr_live, "host", "formats"], timeout=30)
            rows = parse_host_formats(document)
            last_rows, last_document, last_error = rows, document, None
            problems = judge(rows)
            if not problems:
                return rows, document, attempts
            last_error = "; ".join(problems)
        except (RuntimeError, ValueError) as error:
            last_error = str(error)
        time.sleep(interval)
    detail = last_error or "no endpoint data"
    raise PollError(
        f"endpoints did not reach the expected names within {timeout} seconds: {detail}",
        last_rows, last_document, attempts)


def phase(rows, document, attempts=None):
    return {
        "attempts": attempts,
        "endpoints": [
            {"endpoint": row.get("endpoint"), "state": row.get("state"),
             "device_desc": row.get("device_desc"), "friendly_name": row.get("friendly_name")}
            for row in rows
        ],
        "host_formats": document,
    }


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--ssr-live", required=True, type=Path)
    parser.add_argument("--summary", required=True, type=Path)
    args = parser.parse_args(argv)

    empty_phase = phase([], None, 0)
    result = {
        "phases": {"changed": dict(empty_phase), "restored": dict(empty_phase)},
        "original_registry": {},
        "problems": [],
    }
    problems = result["problems"]
    original = None
    try:
        original = snapshot_values()
        result["original_registry"] = json_registry_snapshot(original)
        write_values({
            1: {"present": True, "type": REG_SZ, "value": SHORT_NAME},
            2: {"present": True, "type": REG_SZ, "value": LONG_NAME},
        })
        result["restart_changed"] = restart_device(args.ssr_live)
        rows, document, attempts = poll_formats(args.ssr_live, changed_name_problems)
        result["phases"]["changed"] = phase(rows, document, attempts)
    except PollError as error:
        result["phases"]["changed"] = phase(error.rows, error.document, error.attempts)
        problems.append(f"change phase: {error}")
    except RuntimeError as error:
        problems.append(f"change phase: {error}")
    finally:
        if original is not None:
            restoration_errors = restore_values(original)
            problems.extend(f"restoration phase: {error}" for error in restoration_errors)
            try:
                result["restart_restored"] = restart_device(args.ssr_live)
                if not restoration_errors:
                    rows, document, attempts = poll_formats(
                        args.ssr_live, lambda value: restored_name_problems(value, original))
                    result["phases"]["restored"] = phase(rows, document, attempts)
            except PollError as error:
                result["phases"]["restored"] = phase(
                    error.rows, error.document, error.attempts)
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
