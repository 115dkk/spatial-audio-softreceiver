"""Check installed endpoint names and device formats through ``ssr-live``.

    py endpoint_format_check.py --ssr-live ssr-live.exe --summary endpoint-format.json

The expected shared formats are deliberately constants here.  A Linux unit
contract compares them with the serialized OEMFormat values in the INF.
"""

import argparse
import json
from pathlib import Path
import subprocess
import sys

from endpoints import ENDPOINT_NUMBERS

REG_SZ = 1
FALLBACK_NAMES = {
    0: "Spatial SoftReceiver",
    1: "Spatial SoftReceiver Music",
    2: "Spatial SoftReceiver PASS",
}
EXPECTED_FORMATS = {
    0: {
        "tag": "extensible",
        "subformat": "ieee_float",
        "rate": 48_000,
        "channels": 8,
        "bits": 32,
        "valid_bits": 32,
        "channel_mask": 0x63F,
    },
    1: {
        "tag": "extensible",
        "subformat": "ieee_float",
        "rate": 48_000,
        "channels": 2,
        "bits": 32,
        "valid_bits": 32,
        "channel_mask": 0x3,
    },
    2: {
        "tag": "extensible",
        "subformat": "ieee_float",
        "rate": 48_000,
        "channels": 2,
        "bits": 32,
        "valid_bits": 32,
        "channel_mask": 0x3,
    },
}
REGISTRY_PATH = r"SYSTEM\CurrentControlSet\Services\SpatialSoftReceiver\Parameters"


def mask_argument(text):
    try:
        value = int(text, 0)
    except ValueError:
        raise argparse.ArgumentTypeError(f"invalid endpoint mask {text!r}") from None
    available = sum(1 << endpoint for endpoint in ENDPOINT_NUMBERS)
    if not value or value & ~available:
        raise argparse.ArgumentTypeError(
            f"endpoint mask must select endpoints 0-{max(ENDPOINT_NUMBERS)}")
    return value


def utf16_units(value):
    return len(value.encode("utf-16-le")) // 2


def effective_name(snapshot, fallback):
    """The name the driver uses for a captured registry value."""
    if not snapshot.get("present"):
        return fallback
    value = snapshot.get("value")
    if snapshot.get("type") != REG_SZ or not isinstance(value, str):
        return fallback
    if not value or utf16_units(value) > 31:
        return fallback
    return value


def json_registry_snapshot(snapshots):
    """Make arbitrary registry data diagnostic JSON without changing raw values."""
    result = {}
    for endpoint, snapshot in snapshots.items():
        item = dict(snapshot)
        if isinstance(item.get("value"), bytes):
            item["value"] = {"bytes_hex": item["value"].hex()}
        result[endpoint] = item
    return result


def registry_names():
    """Read endpoint display data without importing ``winreg`` on Linux."""
    try:
        import winreg
    except ImportError as error:
        raise RuntimeError("the Windows registry is unavailable") from error

    snapshots = {}
    try:
        key = winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, REGISTRY_PATH, 0, winreg.KEY_READ)
    except OSError as error:
        raise RuntimeError(f"cannot open HKLM\\{REGISTRY_PATH}: {error}") from error
    with key:
        for endpoint in ENDPOINT_NUMBERS:
            value_name = f"EndpointName{endpoint}"
            try:
                value, value_type = winreg.QueryValueEx(key, value_name)
                snapshots[endpoint] = {"present": True, "type": value_type, "value": value}
            except FileNotFoundError:
                snapshots[endpoint] = {"present": False, "type": None, "value": None}
            except OSError as error:
                raise RuntimeError(f"cannot read {value_name}: {error}") from error
    names = {endpoint: effective_name(snapshots[endpoint], FALLBACK_NAMES[endpoint])
             for endpoint in ENDPOINT_NUMBERS}
    return names, snapshots


def parse_host_formats(value):
    """Validate the outer JSON shape while retaining diagnostic row fields."""
    if not isinstance(value, dict):
        raise ValueError("host formats JSON must be an object")
    rows = value.get("endpoints")
    if not isinstance(rows, list):
        raise ValueError("host formats JSON must contain an endpoints array")
    for index, row in enumerate(rows):
        if not isinstance(row, dict):
            raise ValueError(f"endpoint row {index} must be an object")
        endpoint = row.get("endpoint")
        if type(endpoint) is not int or endpoint not in ENDPOINT_NUMBERS:
            raise ValueError(f"endpoint row {index} has invalid endpoint {endpoint!r}")
    return rows


def active_problems(rows, expect_mask):
    problems = []
    for endpoint in ENDPOINT_NUMBERS:
        matches = [row for row in rows if row.get("endpoint") == endpoint]
        expected = bool(expect_mask & (1 << endpoint))
        active = [row for row in matches if row.get("state") == "active"]
        if expected:
            if len(matches) != 1:
                problems.append(
                    f"endpoint {endpoint}: found {len(matches)} rows, expected exactly one")
            if len(active) != 1:
                problems.append(
                    f"endpoint {endpoint}: found {len(active)} active rows, expected exactly one")
        elif active:
            problems.append(f"endpoint {endpoint}: active outside expected mask {expect_mask:#x}")
    return problems


def name_problems(rows, expected_names, expect_mask):
    problems = []
    for endpoint in ENDPOINT_NUMBERS:
        if not expect_mask & (1 << endpoint):
            continue
        matches = [row for row in rows
                   if row.get("endpoint") == endpoint and row.get("state") == "active"]
        if len(matches) != 1:
            continue
        actual = matches[0].get("device_desc")
        expected = expected_names[endpoint]
        if actual != expected:
            problems.append(
                f"endpoint {endpoint}: device_desc {actual!r}, expected registry name {expected!r}")
    return problems


def format_problems(rows, expect_mask):
    """Pure judgment of endpoint multiplicity, state and exact device format."""
    problems = active_problems(rows, expect_mask)
    for endpoint in ENDPOINT_NUMBERS:
        if not expect_mask & (1 << endpoint):
            continue
        matches = [row for row in rows
                   if row.get("endpoint") == endpoint and row.get("state") == "active"]
        if len(matches) != 1:
            continue
        row = matches[0]
        error = row.get("device_format_error")
        if error is not None:
            problems.append(f"endpoint {endpoint}: device_format_error is {error!r}")
        actual = row.get("device_format")
        if not isinstance(actual, dict):
            problems.append(f"endpoint {endpoint}: device_format is not an object")
            continue
        for field, expected in EXPECTED_FORMATS[endpoint].items():
            if actual.get(field) != expected:
                problems.append(
                    f"endpoint {endpoint}: device_format.{field} is {actual.get(field)!r}, "
                    f"expected {expected!r}")
    return problems


def run_host_formats(ssr_live, timeout=30):
    command = [str(ssr_live), "host", "formats"]
    try:
        completed = subprocess.run(
            command, check=False, capture_output=True, text=True, encoding="utf-8", timeout=timeout)
    except (OSError, subprocess.SubprocessError) as error:
        raise RuntimeError(f"cannot run {' '.join(command)}: {error}") from error
    if completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip() or "no diagnostic output"
        raise RuntimeError(f"host formats exited with {completed.returncode}: {detail}")
    try:
        value = json.loads(completed.stdout.lstrip("﻿"))
    except json.JSONDecodeError as error:
        raise RuntimeError(f"host formats returned invalid JSON: {error}") from error
    try:
        return parse_host_formats(value), value
    except ValueError as error:
        raise RuntimeError(str(error)) from error


def write_summary(path, result):
    text = json.dumps(result, indent=2, ensure_ascii=True)
    Path(path).write_text(text, encoding="utf-8")
    print(text)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--ssr-live", required=True, type=Path)
    parser.add_argument("--summary", required=True, type=Path)
    parser.add_argument("--expect-mask", type=mask_argument, default=0x7)
    args = parser.parse_args(argv)

    rows, document, snapshots, expected_names = [], None, {}, {}
    problems = []
    try:
        expected_names, snapshots = registry_names()
    except RuntimeError as error:
        problems.append(str(error))
    try:
        rows, document = run_host_formats(args.ssr_live)
    except RuntimeError as error:
        problems.append(str(error))
    if document is not None:
        problems += format_problems(rows, args.expect_mask)
        if expected_names:
            problems += name_problems(rows, expected_names, args.expect_mask)

    result = {
        "command": [str(args.ssr_live), "host", "formats"],
        "expect_mask": args.expect_mask,
        "expected_formats": EXPECTED_FORMATS,
        "registry": json_registry_snapshot(snapshots),
        "expected_device_desc": expected_names,
        "endpoints": rows,
        "host_formats": document,
        "problems": problems,
        "passed": not problems,
    }
    try:
        write_summary(args.summary, result)
    except OSError as error:
        print(f"cannot write summary {args.summary}: {error}", file=sys.stderr)
        return 1
    for problem in problems:
        print(problem, file=sys.stderr)
    return 1 if problems else 0


if __name__ == "__main__":
    raise SystemExit(main())
