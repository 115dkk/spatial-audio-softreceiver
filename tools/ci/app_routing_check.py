"""Per-app entrances through Windows' per-app output setting (ADR 0038).

A helper process plays silence on the default render endpoint so that it has
an audio session. `ssr-live host apps` must list it; `ssr-live host set-app`
then sends it to the passthrough entrance (endpoint 2), the music entrance
(1), the main entrance (0) and back to no setting, and each call must read
its own write back (the tool checks that and exits non-zero otherwise). The
endpoint ids come from `ssr-live host show`, which finds our endpoints by
their KS filter reference, never by name. The helper's setting is cleared at
the end whatever happened.

    py app_routing_check.py --ssr-live ssr-live.exe --summary app-routing.json

Exits 0 when every step passes.
"""

import argparse
import json
import subprocess
import sys
import time
from pathlib import Path

# Windows keeps the per-app setting per executable path, so the helper is the
# interpreter running this check; it is cleared in `finally`.
HELPER = """
import sys, time
import numpy as np
from numpy_compat import binary_fromstring
np.fromstring = binary_fromstring
import soundcard as sc
speaker = sc.default_speaker()
with speaker.player(samplerate=48000, channels=2, blocksize=480) as out:
    deadline = time.monotonic() + float(sys.argv[1])
    while time.monotonic() < deadline:
        out.play(np.zeros((4800, 2), dtype=np.float32))
"""

ENTRANCES = (2, 1, 0)


def endpoint_ids(show):
    """Endpoint number -> active device id, from `host show`."""
    ids = {}
    for output in show.get("outputs", []):
        if output.get("endpoint") is not None and output.get("state") == "active":
            ids.setdefault(output["endpoint"], []).append(output["id"])
    return ids


def set_problems(endpoint, result, returncode, ids):
    """Problems with one `host set-app` run (`endpoint` is 0..2 or None)."""
    label = "none" if endpoint is None else str(endpoint)
    if returncode != 0:
        return [f"set-app --endpoint {label} exited with {returncode}: {result}"]
    if not isinstance(result, dict) or result.get("ok") is not True:
        return [f"set-app --endpoint {label} did not report ok: {result}"]
    expected = None if endpoint is None else ids[endpoint][0]
    if result.get("app_entrance") != expected:
        return [f"set-app --endpoint {label} read back {result.get('app_entrance')}, expected {expected}"]
    return []


def helper_row(apps, pid):
    rows = [row for row in apps.get("audio_apps", []) if row.get("pid") == pid]
    return rows[0] if rows else None


def run_json(command):
    completed = subprocess.run(command, capture_output=True, text=True, encoding="utf-8", timeout=60)
    try:
        value = json.loads(completed.stdout) if completed.stdout.strip() else None
    except json.JSONDecodeError:
        value = completed.stdout.strip()
    return completed.returncode, value, completed.stderr.strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--ssr-live", required=True)
    parser.add_argument("--summary", required=True)
    args = parser.parse_args()
    live = args.ssr_live
    summary = {"steps": [], "problems": []}
    problems = summary["problems"]

    code, show, err = run_json([live, "host", "show"])
    if code != 0 or not isinstance(show, dict):
        problems.append(f"host show exited with {code}: {err}")
        show = {}
    ids = endpoint_ids(show)
    for endpoint in ENTRANCES:
        if len(ids.get(endpoint, [])) != 1:
            problems.append(f"expected one active endpoint {endpoint}, found {ids.get(endpoint, [])}")

    helper = subprocess.Popen([sys.executable, "-c", HELPER, "40"], cwd=Path(__file__).parent)
    try:
        time.sleep(3)
        row = None
        for _ in range(10):
            code, apps, err = run_json([live, "host", "apps"])
            row = helper_row(apps if isinstance(apps, dict) else {}, helper.pid)
            if row is not None:
                break
            time.sleep(1)
        summary["helper"] = row
        if row is None:
            problems.append(f"host apps did not list the helper process {helper.pid}")
        if not problems:
            for endpoint in (*ENTRANCES, None):
                label = "none" if endpoint is None else str(endpoint)
                code, result, err = run_json([live, "host", "set-app", "--pid", str(helper.pid),
                                              "--endpoint", label])
                summary["steps"].append({"endpoint": label, "exit": code, "result": result, "stderr": err})
                problems += set_problems(endpoint, result, code, ids)
                if endpoint == 2 and code == 0:
                    code, apps, err = run_json([live, "host", "apps"])
                    listed = helper_row(apps if isinstance(apps, dict) else {}, helper.pid)
                    if listed is None or listed.get("app_entrance") != ids[2][0]:
                        problems.append(f"host apps shows {listed} after set-app 2, expected app_entrance {ids[2][0]}")
    finally:
        subprocess.run([live, "host", "set-app", "--pid", str(helper.pid), "--endpoint", "none"],
                       capture_output=True, timeout=60)
        helper.kill()
    Path(args.summary).write_text(json.dumps(summary, indent=2, ensure_ascii=False), encoding="utf-8")
    for problem in problems:
        print(problem)
    if problems:
        return 1
    print("pass")
    return 0


if __name__ == "__main__":
    sys.exit(main())
