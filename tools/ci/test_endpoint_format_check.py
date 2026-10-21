import json
import struct
import tempfile
import unittest
from unittest import mock
from contextlib import redirect_stderr, redirect_stdout
from io import StringIO
from pathlib import Path

from driver_flow_check import parse_wave_format
from endpoint_format_check import (
    EXPECTED_FORMATS,
    FALLBACK_NAMES,
    REG_SZ,
    effective_name,
    format_problems,
    json_registry_snapshot,
    main,
    name_problems,
    parse_host_formats,
)

try:
    import tomllib
except ImportError:  # pragma: no cover - Python 3.10 on older Windows runners
    import tomli as tomllib

ROOT = Path(__file__).resolve().parents[2]


def read(path, encoding="utf-8-sig"):
    return (ROOT / path).read_text(encoding=encoding)


def section(text, name):
    return text.split(f"[{name}]", 1)[1].split("\n[", 1)[0]


def endpoint_row(endpoint, **changes):
    row = {
        "endpoint": endpoint,
        "state": "active",
        "device_desc": FALLBACK_NAMES[endpoint],
        "friendly_name": f"alias {endpoint}",
        "device_format_error": None,
        "device_format": dict(EXPECTED_FORMATS[endpoint]),
    }
    row.update(changes)
    return row


class EndpointFormatContractTests(unittest.TestCase):
    def test_expected_formats_match_inf_oem_format_blobs(self):
        inf = read("driver/spatialsoftreceiver/spatialsoftreceiver.inx", "utf-16")
        for endpoint, expected in EXPECTED_FORMATS.items():
            topology = section(inf, f"SpatialSoftReceiver.I.TopologyHdmi{endpoint}.AddReg")
            line = next(line for line in topology.splitlines()
                        if line.startswith("HKR,EP\\0,%PKEY_AudioEngine_OEMFormat%"))
            blob = bytes(int(part, 16) for part in line.split(",")[4:])
            self.assertEqual(struct.unpack_from("<II", blob), (65, 40))
            payload = blob[8:]
            parsed = parse_wave_format(payload)
            tag, _, _, _, _, bits, extension = struct.unpack_from("<HHIIHHH", payload)
            valid_bits, channel_mask = struct.unpack_from("<HI", payload, 18)
            subformat_data1 = struct.unpack_from("<I", payload, 24)[0]
            self.assertEqual(tag, 0xFFFE)
            self.assertEqual(extension, 22)
            self.assertEqual(parsed["sample"], "float")
            self.assertEqual(subformat_data1, 3)
            self.assertEqual(
                {
                    "tag": "extensible",
                    "subformat": "ieee_float",
                    "rate": parsed["rate"],
                    "channels": parsed["channels"],
                    "bits": bits,
                    "valid_bits": valid_bits,
                    "channel_mask": channel_mask,
                },
                expected,
            )

    def test_fallback_names_match_english_catalog(self):
        catalog = tomllib.loads(read("i18n/en.toml"))["names"]
        self.assertEqual(
            FALLBACK_NAMES,
            {0: catalog["endpoint_main"], 1: catalog["endpoint_music"],
             2: catalog["endpoint_pass"]},
        )

    def test_registry_summary_preserves_binary_data_as_hex(self):
        snapshot = {1: {"present": True, "type": 3, "value": b"\x00\xff"}}
        self.assertEqual(
            json_registry_snapshot(snapshot)[1]["value"], {"bytes_hex": "00ff"})
        self.assertEqual(snapshot[1]["value"], b"\x00\xff")

    def test_effective_name_uses_only_valid_short_reg_sz(self):
        self.assertEqual(effective_name(
            {"present": True, "type": REG_SZ, "value": "custom"}, "fallback"), "custom")
        for snapshot in (
            {"present": False, "type": None, "value": None},
            {"present": True, "type": REG_SZ, "value": ""},
            {"present": True, "type": 4, "value": 1},
            {"present": True, "type": REG_SZ, "value": "x" * 32},
            {"present": True, "type": REG_SZ, "value": "😀" * 16},
        ):
            with self.subTest(snapshot=snapshot):
                self.assertEqual(effective_name(snapshot, "fallback"), "fallback")

    def test_format_problems_accept_exact_active_rows(self):
        rows = [endpoint_row(endpoint) for endpoint in range(3)]
        self.assertEqual(format_problems(rows, 0x7), [])
        self.assertEqual(format_problems(rows[:2], 0x3), [])

    def test_format_problems_reject_duplicates_state_error_and_each_field(self):
        base = [endpoint_row(endpoint) for endpoint in range(3)]
        cases = [
            base + [endpoint_row(1)],
            [endpoint_row(0), endpoint_row(1, state="disabled"), endpoint_row(2)],
            [endpoint_row(0), endpoint_row(1, device_format_error="E_FAIL"), endpoint_row(2)],
            [endpoint_row(0), endpoint_row(1, device_format=None), endpoint_row(2)],
        ]
        for field in EXPECTED_FORMATS[0]:
            wrong = dict(EXPECTED_FORMATS[0])
            wrong[field] = "wrong"
            cases.append([endpoint_row(0, device_format=wrong), endpoint_row(1), endpoint_row(2)])
        for rows in cases:
            with self.subTest(rows=rows):
                self.assertTrue(format_problems(rows, 0x7))
        self.assertTrue(format_problems(base, 0x3))

    def test_name_problems_compare_device_desc_not_friendly_name(self):
        names = dict(FALLBACK_NAMES)
        rows = [endpoint_row(endpoint) for endpoint in range(3)]
        rows[1]["friendly_name"] = "user alias"
        self.assertEqual(name_problems(rows, names, 0x7), [])
        rows[1]["device_desc"] = "stale builder name"
        self.assertIn("device_desc", name_problems(rows, names, 0x7)[0])

    def test_main_reports_empty_endpoint_array(self):
        with tempfile.TemporaryDirectory() as folder:
            folder = Path(folder)
            live = folder / "ssr-live"
            live.write_text("#!/bin/sh\nprintf '%s' '{\"endpoints\":[]}'\n")
            live.chmod(0o755)
            summary = folder / "summary.json"
            with (mock.patch(
                    "endpoint_format_check.registry_names",
                    return_value=(dict(FALLBACK_NAMES), {})),
                    redirect_stdout(StringIO()), redirect_stderr(StringIO())):
                self.assertEqual(main([
                    "--ssr-live", str(live), "--summary", str(summary)]), 1)
            result = json.loads(summary.read_text())
            self.assertFalse(result["passed"])
            self.assertTrue(any("found 0 rows" in problem for problem in result["problems"]))

    def test_parse_host_formats_rejects_malformed_boundary(self):
        rows = [endpoint_row(0)]
        self.assertIs(parse_host_formats({"endpoints": rows}), rows)
        for value in ([], {}, {"endpoints": {}}, {"endpoints": [None]},
                      {"endpoints": [{"endpoint": "0"}]},
                      {"endpoints": [{"endpoint": 3}]}):
            with self.subTest(value=value):
                with self.assertRaises(ValueError):
                    parse_host_formats(value)


if __name__ == "__main__":
    unittest.main()
