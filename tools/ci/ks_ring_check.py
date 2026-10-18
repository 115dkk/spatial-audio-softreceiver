"""Kernel streaming into our own WaveRT endpoint, checked on the ring (ADR 0037).

VB-CABLE is WaveCyclic, so the WaveRT transport of `ssr-live ks-probe` is
checked against our driver: endpoint 1 (Spatial SoftReceiver Music) is
disabled, its render pin is opened through kernel streaming, and the probe's
1 kHz tone at -12 dBFS must arrive in the ring as endpoint 1's PCM, without
byte gaps or overruns, at the format's byte rate. The probe must report the
WaveRT transport and give the endpoint back.

    py ks_ring_check.py --ssr-live ssr-live.exe --summary ks-ring.json

The endpoint is found by its fixed number through endpoints.ps1, never by
name. Exits 0 when every check passes.
"""

import argparse
import json
import subprocess
import sys
import threading
import time
from pathlib import Path

from ks_probe_check import TONE_DBFS, TONE_HZ, TONE_TOLERANCE_DB, probe_problems

KS_ENDPOINT = 1
MIN_TONE_SECONDS = 1.0


def ring_problems(records, endpoint, qpc_frequency):
    """Problems with one endpoint's records of the probe tone, and a summary."""
    from driver_flow_check import KIND_DATA, KIND_FORMAT_CHANGE, decode, parse_wave_format
    from ring_stats import endpoint_stats

    mine = [r for r in records if r["endpoint"] == endpoint]
    formats = [r for r in mine if r["kind"] == KIND_FORMAT_CHANGE]
    data = [r for r in mine if r["kind"] == KIND_DATA]
    summary = {"endpoint": endpoint, "records": len(mine), "data_records": len(data)}
    if not formats:
        return summary, [f"endpoint {endpoint}: no format record"]
    if not data:
        return summary, [f"endpoint {endpoint}: no data records"]
    fmt = parse_wave_format(formats[-1]["payload"])
    summary["format"] = fmt
    stats = endpoint_stats(records, endpoint, qpc_frequency, fmt, decode, TONE_HZ)
    summary.update(stats)
    problems = []
    if stats["gaps"]:
        problems.append(f"endpoint {endpoint}: {stats['gaps']} gaps in byte_pos")
    if stats["overruns"]:
        problems.append(f"endpoint {endpoint}: {stats['overruns']} overrun records")
    if stats.get("seconds", 0.0) < MIN_TONE_SECONDS:
        problems.append(f"endpoint {endpoint}: only {stats.get('seconds', 0.0):.2f} s of data")
    level = stats.get("tone_db")
    if level is None:
        problems.append(f"endpoint {endpoint}: the format {fmt} could not be decoded")
    elif abs(level - TONE_DBFS) > TONE_TOLERANCE_DB:
        problems.append(f"endpoint {endpoint}: the probe tone is at {level:.1f} dBFS, expected {TONE_DBFS}")
    return summary, problems


def endpoint_id(endpoint):
    from endpoints import enumerate_endpoints

    rows = [row for row in enumerate_endpoints() if row["endpoint"] == endpoint and row["state"] == 1]
    if len(rows) != 1:
        raise SystemExit(f"expected one active endpoint {endpoint}, found {len(rows)}")
    return rows[0]["id"]


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--ssr-live", required=True)
    parser.add_argument("--seconds", type=float, default=2.0)
    parser.add_argument("--summary", required=True)
    args = parser.parse_args()

    from driver_flow_check import parse_records
    from ssr_control import ControlDevice

    target = endpoint_id(KS_ENDPOINT)
    probe_json = Path(args.summary).with_name("ks-ring-probe.json")
    device = ControlDevice()
    info = device.query_info()
    reads, errors = [], []
    stop = threading.Event()

    def reader():
        try:
            while not stop.is_set():
                reads.append(device.read(4096, 50))
        except OSError as error:
            errors.append(str(error))

    thread = threading.Thread(target=reader)
    thread.start()
    time.sleep(0.5)
    probe = subprocess.run([args.ssr_live, "ks-probe", "--output", target, "--seconds", str(args.seconds),
                            "--json", str(probe_json)], timeout=120)
    time.sleep(1.0)
    stop.set()
    thread.join()
    device.close()

    problems = [f"READ failed: {e}" for e in errors]
    report = json.loads(probe_json.read_text(encoding="utf-8-sig")) if probe_json.exists() else None
    if report is None:
        problems.append("ks-probe wrote no report")
    else:
        problems += probe_problems(report, "wavert")
    if probe.returncode:
        problems.append(f"ks-probe exited with {probe.returncode}")
    summary, ring = ring_problems(parse_records(b"".join(reads)), KS_ENDPOINT, info["qpc_frequency"])
    problems += ring
    Path(args.summary).write_text(json.dumps({"probe": report, "ring": summary, "problems": problems},
                                             indent=2, default=str), encoding="utf-8")
    for problem in problems:
        print(problem)
    if problems:
        return 1
    print("pass")
    return 0


if __name__ == "__main__":
    sys.exit(main())
