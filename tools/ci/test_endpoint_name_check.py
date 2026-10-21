import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

from endpoint_format_check import FALLBACK_NAMES, REG_SZ
import endpoint_name_check as check
from endpoint_name_check import (
    LONG_NAME,
    SHORT_NAME,
    changed_name_problems,
    parse_sink_lines,
    restore_values,
    restored_name_problems,
)

INITIAL = {0: FALLBACK_NAMES[0], 1: FALLBACK_NAMES[1], 2: FALLBACK_NAMES[2]}


def sink_line(endpoint, name):
    return (f"ks topologyhdmi{endpoint} pin 1 JACK_SINK_INFO: ConnType=0 ManufacturerId=4445 "
            f"ProductId=1 AudioLatency=0 HDCPCapable=0 AICapable=1 "
            f"SinkDescriptionLength={len(name)} SinkDescription={name} "
            f"PortId=LUID(0x00000000:0x00000000)")


def row(endpoint, desc, state="active", friendly_name=None):
    return {
        "endpoint": endpoint,
        "state": state,
        "device_desc": desc,
        "friendly_name": friendly_name if friendly_name is not None else f"alias {endpoint}",
    }


class RestorationTests(unittest.TestCase):
    def setUp(self):
        self.values = {
            1: {"present": True, "type": REG_SZ, "value": "one"},
            2: {"present": False, "type": None, "value": None},
        }

    def test_restore_attempts_both_endpoints_and_reports_first_failure(self):
        calls = []

        def write(values):
            calls.append(values)
            if 1 in values:
                raise RuntimeError("first failed")

        self.assertEqual(restore_values(self.values, write), ["endpoint 1: first failed"])
        self.assertEqual(calls, [{1: self.values[1]}, {2: self.values[2]}])

    def test_restore_reports_both_failures(self):
        def write(values):
            endpoint = next(iter(values))
            raise RuntimeError(f"write {endpoint} failed")

        self.assertEqual(restore_values(self.values, write), [
            "endpoint 1: write 1 failed", "endpoint 2: write 2 failed"])

    def test_main_restarts_after_restore_failures_and_stays_failed(self):
        for failed in ({1}, {1, 2}):
            with self.subTest(failed=failed), tempfile.TemporaryDirectory() as directory:
                summary = Path(directory) / "summary.json"
                restarts = []
                restored = {}
                writes = []

                def write(values):
                    writes.append(values)
                    if len(values) == 2:
                        return
                    endpoint = next(iter(values))
                    if endpoint in failed:
                        raise RuntimeError(f"endpoint {endpoint} failed")
                    restored.update(values)

                with (mock.patch.object(check, "snapshot_values", return_value=self.values),
                      mock.patch.object(check, "write_values", side_effect=write),
                      mock.patch.object(check, "restart_device",
                                        side_effect=lambda value: restarts.append(value) or ["restart"]),
                      mock.patch.object(check, "read_formats",
                                        return_value=([], {"endpoints": []})),
                      mock.patch.object(check, "poll_names",
                                        return_value=([], {"endpoints": []}, {}, 1)) as poll,
                      mock.patch("builtins.print")):
                    result = check.main([
                        "--ssr-live", "live", "--summary", str(summary)])
                document = json.loads(summary.read_text(encoding="utf-8"))
                self.assertEqual(result, 1)
                self.assertFalse(document["passed"])
                self.assertEqual(len(restarts), 2)
                self.assertEqual(writes[1:], [{1: self.values[1]}, {2: self.values[2]}])
                self.assertEqual(restored, {endpoint: value for endpoint, value in self.values.items()
                                            if endpoint not in failed})
                poll.assert_called_once()
                for endpoint in failed:
                    self.assertTrue(any(
                        f"endpoint {endpoint}" in problem
                        for problem in document["problems"]))


class EndpointNameJudgmentTests(unittest.TestCase):
    def test_test_values_cover_valid_and_overlong_utf16_limits(self):
        self.assertLessEqual(len(SHORT_NAME.encode("utf-16-le")) // 2, 31)
        self.assertGreaterEqual(len(LONG_NAME.encode("utf-16-le")) // 2, 32)

    def test_sink_lines_map_topology_filters_to_endpoints(self):
        lines = [
            "ks topologyhdmi0 pin 1 JACK_SINK_INFO: OK bytes=96",
            sink_line(0, FALLBACK_NAMES[0]),
            sink_line(1, SHORT_NAME),
            "ks wavehdmi1 pin 0 JACK_SINK_INFO: ERROR_NOT_FOUND",
            sink_line(2, FALLBACK_NAMES[2]),
        ]
        self.assertEqual(parse_sink_lines(lines),
                         {0: FALLBACK_NAMES[0], 1: SHORT_NAME, 2: FALLBACK_NAMES[2]})

    def test_two_different_sinks_for_one_endpoint_are_refused(self):
        with self.assertRaises(ValueError):
            parse_sink_lines([sink_line(1, "one"), sink_line(1, "two")])
        self.assertEqual(parse_sink_lines([sink_line(1, "one"), sink_line(1, "one")]), {1: "one"})

    def test_changed_names_come_from_the_sinks_while_windows_keeps_its_descriptions(self):
        rows = [row(0, FALLBACK_NAMES[0]), row(1, FALLBACK_NAMES[1]), row(2, FALLBACK_NAMES[2])]
        sinks = {0: FALLBACK_NAMES[0], 1: SHORT_NAME, 2: FALLBACK_NAMES[2]}
        self.assertEqual(changed_name_problems(rows, sinks, INITIAL), [])

    def test_changed_names_detect_a_stale_sink_a_missing_fallback_and_an_inactive_endpoint(self):
        rows = [row(0, FALLBACK_NAMES[0]), row(1, FALLBACK_NAMES[1]), row(2, FALLBACK_NAMES[2])]
        problems = changed_name_problems(rows, {1: FALLBACK_NAMES[1], 2: LONG_NAME}, INITIAL)
        self.assertTrue(any("endpoint 1: sink description" in problem for problem in problems))
        self.assertTrue(any("endpoint 2: sink description" in problem for problem in problems))
        rows[2]["state"] = "disabled"
        self.assertTrue(changed_name_problems(rows, {1: SHORT_NAME, 2: FALLBACK_NAMES[2]}, INITIAL))

    def test_an_endpoint_description_that_changes_is_reported(self):
        rows = [row(0, FALLBACK_NAMES[0]), row(1, SHORT_NAME), row(2, FALLBACK_NAMES[2])]
        problems = changed_name_problems(rows, {1: SHORT_NAME, 2: FALLBACK_NAMES[2]}, INITIAL)
        self.assertEqual(problems, [
            f"endpoint 1: device_desc changed from {FALLBACK_NAMES[1]!r} to {SHORT_NAME!r}"])

    def test_restored_names_follow_exact_original_registry_semantics(self):
        original = {
            1: {"present": True, "type": REG_SZ, "value": "original one"},
            2: {"present": False, "type": None, "value": None},
        }
        initial = {0: FALLBACK_NAMES[0], 1: "original one", 2: FALLBACK_NAMES[2]}
        rows = [row(0, FALLBACK_NAMES[0]), row(1, "original one"), row(2, FALLBACK_NAMES[2])]
        sinks = {1: "original one", 2: FALLBACK_NAMES[2]}
        self.assertEqual(restored_name_problems(rows, sinks, original, initial), [])
        sinks[1] = SHORT_NAME
        self.assertTrue(restored_name_problems(rows, sinks, original, initial))

    def test_restored_invalid_original_type_and_long_value_use_fallback(self):
        originals = (
            {1: {"present": True, "type": 4, "value": 7},
             2: {"present": True, "type": REG_SZ, "value": "x" * 32}},
            {1: {"present": True, "type": REG_SZ, "value": ""},
             2: {"present": True, "type": REG_SZ, "value": "😀" * 16}},
        )
        rows = [row(0, FALLBACK_NAMES[0]), row(1, FALLBACK_NAMES[1]), row(2, FALLBACK_NAMES[2])]
        sinks = {1: FALLBACK_NAMES[1], 2: FALLBACK_NAMES[2]}
        for original in originals:
            with self.subTest(original=original):
                self.assertEqual(restored_name_problems(rows, sinks, original, INITIAL), [])

    def test_duplicate_rows_never_resolve_by_name_or_order(self):
        original = {
            1: {"present": True, "type": REG_SZ, "value": "one"},
            2: {"present": True, "type": REG_SZ, "value": "two"},
        }
        rows = [row(0, FALLBACK_NAMES[0]), row(1, "one"), row(1, "one"), row(2, "two")]
        self.assertTrue(restored_name_problems(rows, {1: "one", 2: "two"}, original, INITIAL))


if __name__ == "__main__":
    unittest.main()
