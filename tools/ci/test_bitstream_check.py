"""Linux unit tests for bitstream_check; no Windows device or third-party modules."""

import argparse
import copy
import json
from pathlib import Path
import struct
import tempfile
import unittest
from unittest import mock

import bitstream_check as check
from ssr_abi import (
    ABI_VERSION,
    DUMP_FORMAT_VERSION,
    DUMP_HEADER,
    DUMP_MAGIC,
    FORMAT_EAC3,
    RECORD_ENDPOINT_RECORDER,
    RECORD_FLAG_DISCONTINUITY,
    RECORD_FLAG_REPEAT,
    RECORD_HEADER,
    RECORD_KIND_DATA,
    RECORD_KIND_FORMAT_CHANGE,
    RECORD_KIND_OVERRUN,
    RECORD_KIND_READ_END,
    RECORD_KIND_RECORDER_GAP,
    RECORD_KIND_STREAM_START,
    RECORD_KIND_STREAM_STOP,
    WAVE_GUID_TAIL,
)


def valid_probe():
    endpoints = []
    for endpoint in range(3):
        mask = 108 if endpoint == 2 else 0
        codecs = []
        for bit, codec in check.CODECS.items():
            enabled = endpoint == 2 and bool(mask & (1 << bit))
            codecs.append({
                "bit": bit,
                "codec": codec,
                "supported_hr": check.S_OK if enabled else "0x88890008",
                "initialize_hr": check.S_OK if enabled else None,
            })
        endpoints.append({"endpoint": endpoint, "id": f"endpoint-{endpoint}",
                          "codec_mask": mask, "codecs": codecs})
    return {"endpoints": endpoints}


def source_frames(count=10):
    return [f"frame-{index}".encode() for index in range(count)]


def burst(payload, pc=21, pd=None, period=check.EAC3_BURST_BYTES):
    pd = len(payload) if pd is None else pd
    carrier = bytearray()
    for offset in range(0, len(payload), 2):
        word = payload[offset:offset + 2]
        carrier.extend((word[1] if len(word) == 2 else 0, word[0]))
    value = bytearray(check.IEC_SYNC)
    value.extend(pc.to_bytes(2, "little"))
    value.extend(pd.to_bytes(2, "little"))
    value.extend(carrier)
    if len(value) > period:
        raise ValueError("fixture payload too large")
    value.extend(bytes(period - len(value)))
    return bytes(value)


def wave_format(codec="eac3"):
    ids = {"eac3": (0x000A, 192_000, 2, 6),
           "eac3_joc": (0x010A, 192_000, 2, 8),
           "truehd": (0x000C, 192_000, 8, 8)}
    subtype, rate, channels, encoded_channels = ids[codec]
    block = channels * 2
    return struct.pack(
        "<HHIIHHHHI", 0xFFFE, channels, rate, rate * block, block, 16, 34, 16, 0
    ) + struct.pack("<IH", subtype, 0x0CEA) + WAVE_GUID_TAIL + struct.pack(
        "<III", 48_000, encoded_channels, 0)


def pcm_format():
    return struct.pack(
        "<HHIIHHHHI", 0xFFFE, 2, 48_000, 384_000, 8, 32, 22, 32, 3
    ) + struct.pack("<IH", 3, 0) + WAVE_GUID_TAIL


def record(kind, payload=b"", endpoint=2, byte_pos=0, flags=0, epoch=1,
           reserved=0, padding=None):
    header = RECORD_HEADER.pack(kind, flags, epoch, byte_pos, 100, 0,
                                len(payload), endpoint, reserved)
    natural = (-len(header + payload)) % 8
    if padding is None:
        padding = bytes(natural)
    if len(padding) != natural:
        raise ValueError("wrong test padding")
    return header + payload + padding


def dump(*records, reserved=0, version=DUMP_FORMAT_VERSION):
    header = DUMP_HEADER.pack(DUMP_MAGIC, ABI_VERSION, DUMP_HEADER.size,
                              10_000_000, version, reserved)
    return header + b"".join(records)


class ProbeTests(unittest.TestCase):
    def test_exact_probe_contract_passes(self):
        self.assertEqual(check.probe_problems(valid_probe()), [])

    def test_missing_and_duplicate_endpoints_are_rejected(self):
        document = valid_probe()
        document["endpoints"][1] = copy.deepcopy(document["endpoints"][0])
        problems = check.probe_problems(document)
        self.assertTrue(any("duplicate row" in problem for problem in problems), problems)
        self.assertTrue(any("endpoint 1: missing" in problem for problem in problems), problems)

    def test_missing_and_duplicate_codec_bits_are_rejected(self):
        document = valid_probe()
        codecs = document["endpoints"][2]["codecs"]
        codecs[1] = copy.deepcopy(codecs[0])
        problems = check.probe_problems(document)
        self.assertTrue(any("duplicate codec bit 1" in problem for problem in problems), problems)
        self.assertTrue(any("missing codec bit 2" in problem for problem in problems), problems)

    def test_wrong_name_unknown_mask_and_malformed_rows_are_rejected(self):
        document = valid_probe()
        document["endpoints"][2]["codec_mask"] |= 1 << 20
        document["endpoints"][2]["codecs"][0]["codec"] = "not-ac3"
        document["endpoints"][1]["extra"] = True
        problems = check.probe_problems(document)
        self.assertTrue(any("unknown bits" in problem for problem in problems), problems)
        self.assertTrue(any("expected 'ac3'" in problem for problem in problems), problems)
        self.assertTrue(any("malformed row" in problem for problem in problems), problems)

    def test_unexpected_shared_endpoint_hresult_is_rejected(self):
        document = valid_probe()
        document["endpoints"][0]["codecs"][0]["supported_hr"] = "0x88890004"
        problems = check.probe_problems(document)
        self.assertTrue(any("unexpected rejection" in problem for problem in problems), problems)

    def test_enabled_and_masked_endpoint_two_results_are_strict(self):
        for mutation, phrase in [
            ((2, "supported_hr", "0x88890008"), "enabled codec support"),
            ((2, "initialize_hr", "0x88890008"), "enabled codec initialize"),
            ((1, "supported_hr", "0x88890004"), "masked codec returned"),
            ((1, "initialize_hr", check.S_OK), "masked codec initialize_hr must be null"),
        ]:
            with self.subTest(mutation=mutation):
                document = valid_probe()
                bit, field, value = mutation
                document["endpoints"][2]["codecs"][bit - 1][field] = value
                self.assertTrue(any(phrase in problem for problem in
                                    check.probe_problems(document)))

    def test_eac3_disabled_and_bad_hresult_shape_are_rejected(self):
        document = valid_probe()
        document["endpoints"][2]["codec_mask"] &= ~FORMAT_EAC3
        codec = document["endpoints"][2]["codecs"][1]
        codec["supported_hr"] = "failure"
        codec["initialize_hr"] = None
        problems = check.probe_problems(document)
        self.assertTrue(any("E-AC-3 is disabled" in problem for problem in problems), problems)
        self.assertTrue(any("eight hexadecimal" in problem for problem in problems), problems)

    def test_corrupt_outer_report_never_passes(self):
        for value in (None, [], {}, {"endpoints": {}},
                      {"endpoints": [], "ignored": True}):
            with self.subTest(value=value):
                self.assertTrue(check.probe_problems(value))


class FixtureTests(unittest.TestCase):
    def test_committed_fixture_hash_and_frame_shape(self):
        path, data, frames = check.load_fixture()
        self.assertEqual(path.name, "tones-5.1.eac3")
        self.assertEqual(len(data), 81_920)
        self.assertEqual(len(frames), 32)
        self.assertEqual({len(frame) for frame in frames}, {2560})

    def test_packaged_fixture_is_preferred_and_validated(self):
        _, data, _ = check.load_fixture()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            script = root / "tools" / "ci" / "bitstream_check.py"
            packaged = script.parent / "fixtures" / check.FIXTURE_RELATIVE.name
            checkout = root / check.FIXTURE_RELATIVE
            packaged.parent.mkdir(parents=True)
            checkout.parent.mkdir(parents=True)
            packaged.write_bytes(data)
            checkout.write_bytes(b"corrupt checkout")
            self.assertEqual(check.load_fixture(script)[0], packaged)

            packaged.write_bytes(b"corrupt packaged")
            with self.assertRaisesRegex(ValueError, "SHA-256"):
                check.load_fixture(script)

    def test_fixture_in_kit_root_layout(self):
        _, data, frames = check.load_fixture()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            packaged = root / "fixtures" / "tones-5.1.eac3"
            packaged.parent.mkdir()
            packaged.write_bytes(data)
            self.assertEqual(check.load_fixture(root / "bitstream_check.py"),
                             (packaged, data, frames))

    def test_checkout_fixture_is_fallback_and_missing_is_reported(self):
        _, data, _ = check.load_fixture()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            script = root / "tools" / "ci" / "bitstream_check.py"
            checkout = root / check.FIXTURE_RELATIVE
            checkout.parent.mkdir(parents=True)
            checkout.write_bytes(data)
            self.assertEqual(check.load_fixture(script)[0], checkout)
            checkout.unlink()
            with self.assertRaises(FileNotFoundError):
                check.load_fixture(script)

    def test_truncation_junk_and_wrong_parameters_are_rejected(self):
        _, data, _ = check.load_fixture()
        with self.assertRaisesRegex(ValueError, "truncated"):
            check.parse_eac3_frames(data[:-1])
        with self.assertRaisesRegex(ValueError, "sync missing"):
            check.parse_eac3_frames(b"junk" + data)
        changed = bytearray(data)
        changed[4] ^= 0x40
        with self.assertRaisesRegex(ValueError, "unexpected E-AC-3 parameters"):
            check.parse_eac3_frames(changed)


class DeframerTests(unittest.TestCase):
    def test_word_swap_and_fragmented_preamble_and_payload(self):
        payloads = [b"\x0b\x77\x01", b"\x0b\x77\x02\x03"]
        stream = b"".join(burst(payload) for payload in payloads)
        chunks = [stream[index:index + 1] for index in range(len(stream))]
        self.assertEqual(check.deframe_eac3_chunks(chunks), payloads)

    def test_terminal_zeros_are_allowed_but_truncated_nonzero_data_is_not(self):
        payload = b"\x0b\x77\x01"
        self.assertEqual(check.deframe_eac3_chunks(
            [burst(payload) + bytes(check.EAC3_BURST_BYTES * 2 + 17)]), [payload])
        with self.assertRaisesRegex(ValueError, "truncated"):
            check.deframe_eac3_chunks([burst(payload) + check.IEC_SYNC])
        with self.assertRaisesRegex(ValueError, "preamble missing"):
            check.deframe_eac3_chunks([
                burst(payload) + bytes(check.EAC3_BURST_BYTES) + burst(payload)])

    def test_wrong_pc_pd_padding_and_preamble_are_rejected(self):
        with self.assertRaisesRegex(ValueError, "unexpected IEC 61937 Pc"):
            check.deframe_eac3_chunks([burst(b"abc", pc=1)])
        broken = bytearray(burst(b"abc"))
        broken[6:8] = (check.EAC3_BURST_BYTES).to_bytes(2, "little")
        with self.assertRaisesRegex(ValueError, "invalid IEC 61937 Pd"):
            check.deframe_eac3_chunks([broken])
        broken = bytearray(burst(b"abc"))
        broken[-1] = 1
        with self.assertRaisesRegex(ValueError, "nonzero IEC 61937 padding"):
            check.deframe_eac3_chunks([broken])
        with self.assertRaisesRegex(ValueError, "no IEC 61937 preamble"):
            check.deframe_eac3_chunks([b"not a burst"])

    def test_nonzero_truncated_payload_is_rejected(self):
        partial = burst(b"abcdef")[:100]
        with self.assertRaisesRegex(ValueError, "truncated IEC 61937 burst"):
            check.deframe_eac3_chunks([partial])


class ComparisonTests(unittest.TestCase):
    def setUp(self):
        self.source = source_frames(10)
        self.sent = [self.source[index % len(self.source)] for index in range(100)]

    def test_equal_and_exactly_ninety_percent_pass(self):
        equal = check.compare_payloads(self.source, self.sent, 100)
        self.assertEqual(equal["received_to_sent"], 1.0)
        ninety = check.compare_payloads(self.source, self.sent[5:95], 100)
        self.assertEqual(ninety["missing_prefix"], 5)
        self.assertEqual(ninety["missing_suffix"], 5)

    def test_eighty_nine_percent_fails(self):
        with self.assertRaisesRegex(ValueError, "below"):
            check.compare_payloads(self.source, self.sent[:89], 100)

    def test_reorder_corruption_extra_and_nonpositive_sent_fail(self):
        reordered = list(self.sent)
        reordered[40], reordered[41] = reordered[41], reordered[40]
        corrupted = list(self.sent)
        corrupted[40] = b"corrupt"
        for received in (reordered, corrupted):
            with self.subTest(received=received[40:42]):
                with self.assertRaisesRegex(ValueError, "contiguous finite slice"):
                    check.compare_payloads(self.source, received, 100)
        with self.assertRaisesRegex(ValueError, "more than"):
            check.compare_payloads(self.source, self.sent + [self.source[0]], 100)
        with self.assertRaisesRegex(ValueError, "positive"):
            check.compare_payloads(self.source, [], 0)

    def test_interior_skip_fails_even_with_repeated_source(self):
        received = self.sent[:45] + self.sent[46:96]
        with self.assertRaisesRegex(ValueError, "contiguous finite slice"):
            check.compare_payloads(self.source, received, 100)


class FormatAndDumpTests(unittest.TestCase):
    def test_eac3_and_pcm_formats_parse(self):
        fmt = check.parse_wave_format(wave_format())
        self.assertEqual(fmt["codec"], "eac3")
        self.assertEqual((fmt["carrier_rate"], fmt["carrier_channels"], fmt["carrier_bits"]),
                         (192_000, 2, 16))
        self.assertEqual((fmt["encoded_rate"], fmt["encoded_channels"]), (48_000, 6))
        self.assertEqual(check.parse_wave_format(pcm_format())["kind"], "pcm")

    def test_wrong_eac3_carrier_and_truncated_format_fail(self):
        wrong = bytearray(wave_format())
        wrong[4:8] = (48_000).to_bytes(4, "little")
        wrong[8:12] = (192_000).to_bytes(4, "little")
        with self.assertRaisesRegex(ValueError, "unexpected E-AC-3 carrier"):
            check.parse_wave_format(wrong)
        with self.assertRaisesRegex(ValueError, "truncated"):
            check.parse_wave_format(b"short")

    def test_dump_header_records_and_padding_parse(self):
        payload = wave_format()
        header, records = check.parse_dump(dump(
            record(RECORD_KIND_FORMAT_CHANGE, payload),
            record(RECORD_KIND_STREAM_START),
            record(RECORD_KIND_DATA, burst(b"abc"), byte_pos=0),
            record(RECORD_KIND_STREAM_STOP),
            record(RECORD_KIND_READ_END, endpoint=RECORD_ENDPOINT_RECORDER),
        ))
        self.assertEqual(header["records"], 5)
        self.assertEqual(records[0]["format"]["codec"], "eac3")

    def test_malformed_dump_header_record_payload_and_padding_fail(self):
        cases = [
            (dump(reserved=1), "reserved"),
            (dump(version=DUMP_FORMAT_VERSION + 1), "format version"),
            (DUMP_HEADER.pack(DUMP_MAGIC, ABI_VERSION, DUMP_HEADER.size,
                              10_000_000, DUMP_FORMAT_VERSION, 0) + b"short", "truncated record"),
            (dump(record(99)), "unknown record kind"),
            (dump(record(RECORD_KIND_STREAM_START, b"x")), "event record has a payload"),
            (dump(record(RECORD_KIND_DATA, b"x", padding=b"\x01" * 7)), "nonzero record padding"),
        ]
        for value, phrase in cases:
            with self.subTest(phrase=phrase):
                with self.assertRaisesRegex(ValueError, phrase):
                    check.parse_dump(value)

    def test_pcm_prelude_and_eac3_segment_are_selected(self):
        frame = b"\x0b\x77\x01"
        records = check.parse_dump(dump(
            record(RECORD_KIND_FORMAT_CHANGE, pcm_format()),
            record(RECORD_KIND_STREAM_START),
            record(RECORD_KIND_DATA, bytes(128), byte_pos=0),
            record(RECORD_KIND_STREAM_STOP),
            record(RECORD_KIND_FORMAT_CHANGE, wave_format(), epoch=2),
            record(RECORD_KIND_STREAM_START, epoch=2),
            record(RECORD_KIND_DATA, burst(frame), byte_pos=0, epoch=2),
            record(RECORD_KIND_STREAM_STOP, epoch=2),
        ))[1]
        segments, summary, problems = check.endpoint_two_segments(records)
        self.assertEqual(problems, [])
        self.assertEqual(summary["segments"], 1)
        self.assertEqual(summary["formats"][0]["codec"], "eac3")
        self.assertEqual(check.deframe_eac3_chunks(segments[0]), [frame])

    def test_start_allows_only_first_matching_data_discontinuity(self):
        records = check.parse_dump(dump(
            record(RECORD_KIND_FORMAT_CHANGE, wave_format()),
            record(RECORD_KIND_STREAM_START),
            record(RECORD_KIND_DATA, b"first", byte_pos=0,
                   flags=RECORD_FLAG_DISCONTINUITY),
            record(RECORD_KIND_DATA, b"later", byte_pos=5,
                   flags=RECORD_FLAG_DISCONTINUITY),
        ))[1]
        segments, _, problems = check.endpoint_two_segments(records)
        self.assertEqual(segments, [[b"first"], [b"later"]])
        self.assertEqual(sum("discontinuity" in problem for problem in problems), 1, problems)
        self.assertIn("byte_pos 5", problems[0])

    def test_normal_and_repeat_start_allow_first_discontinuity(self):
        for flags in (0, RECORD_FLAG_REPEAT):
            with self.subTest(flags=flags):
                payload = burst(b"\x0b\x77\x01")
                records = check.parse_dump(dump(
                    record(RECORD_KIND_FORMAT_CHANGE, wave_format(), flags=flags),
                    record(RECORD_KIND_STREAM_START, flags=flags),
                    record(RECORD_KIND_DATA, payload[:100], flags=RECORD_FLAG_DISCONTINUITY),
                    record(RECORD_KIND_DATA, payload[100:], byte_pos=100),
                ))[1]
                segments, _, problems = check.endpoint_two_segments(records)
                self.assertEqual(problems, [])
                self.assertEqual(len(segments), 1)
                self.assertEqual(check.deframe_eac3_chunks(segments[0]), [b"\x0b\x77\x01"])

    def test_boundaries_revoke_unused_start_permission(self):
        boundaries = [
            record(RECORD_KIND_FORMAT_CHANGE, wave_format()),
            record(RECORD_KIND_STREAM_STOP),
            record(RECORD_KIND_OVERRUN, (0).to_bytes(8, "little")),
            record(RECORD_KIND_RECORDER_GAP, (0).to_bytes(8, "little"),
                   endpoint=RECORD_ENDPOINT_RECORDER),
            record(RECORD_KIND_DATA, b"wrong epoch", epoch=2),
        ]
        for boundary in boundaries:
            with self.subTest(boundary=boundary):
                records = check.parse_dump(dump(
                    record(RECORD_KIND_FORMAT_CHANGE, wave_format()),
                    record(RECORD_KIND_STREAM_START),
                    boundary,
                    record(RECORD_KIND_DATA, b"data", flags=RECORD_FLAG_DISCONTINUITY),
                ))[1]
                _, _, problems = check.endpoint_two_segments(records)
                self.assertTrue(any("discontinuity" in problem for problem in problems), problems)

    def test_permission_is_consumed_without_flag_and_isolated_by_endpoint_and_epoch(self):
        records = check.parse_dump(dump(
            record(RECORD_KIND_FORMAT_CHANGE, wave_format(), epoch=3),
            record(RECORD_KIND_STREAM_START, endpoint=1, epoch=3),
            record(RECORD_KIND_DATA, b"a", byte_pos=0, epoch=3,
                   flags=RECORD_FLAG_DISCONTINUITY),
            record(RECORD_KIND_STREAM_START, epoch=3),
            record(RECORD_KIND_DATA, b"b", byte_pos=1, epoch=3),
            record(RECORD_KIND_DATA, b"c", byte_pos=2, epoch=3,
                   flags=RECORD_FLAG_DISCONTINUITY),
            record(RECORD_KIND_STREAM_START, epoch=4),
            record(RECORD_KIND_DATA, b"d", byte_pos=3, epoch=3,
                   flags=RECORD_FLAG_DISCONTINUITY),
        ))[1]
        _, _, problems = check.endpoint_two_segments(records)
        self.assertEqual(sum("discontinuity" in problem for problem in problems), 3, problems)

    def test_format_change_does_not_grant_discontinuity_permission(self):
        records = check.parse_dump(dump(
            record(RECORD_KIND_FORMAT_CHANGE, wave_format()),
            record(RECORD_KIND_DATA, b"data", flags=RECORD_FLAG_DISCONTINUITY),
        ))[1]
        _, _, problems = check.endpoint_two_segments(records)
        self.assertTrue(any("discontinuity" in problem for problem in problems), problems)

    def test_gap_discontinuity_overrun_and_other_codec_are_reported(self):
        records = check.parse_dump(dump(
            record(RECORD_KIND_FORMAT_CHANGE, wave_format()),
            record(RECORD_KIND_DATA, bytes(8), byte_pos=100),
            record(RECORD_KIND_DATA, bytes(8), byte_pos=200,
                   flags=RECORD_FLAG_DISCONTINUITY),
            record(RECORD_KIND_OVERRUN, (7).to_bytes(8, "little")),
            record(RECORD_KIND_FORMAT_CHANGE, wave_format("truehd"), epoch=2),
            record(RECORD_KIND_DATA, b"x", byte_pos=0, epoch=2),
            record(RECORD_KIND_RECORDER_GAP, (9).to_bytes(8, "little"),
                   endpoint=RECORD_ENDPOINT_RECORDER),
        ))[1]
        _, _, problems = check.endpoint_two_segments(records)
        for phrase in ("byte_pos jumped", "discontinuity", "overrun", "unexpected codec",
                       "recorder gap"):
            self.assertTrue(any(phrase in problem for problem in problems), problems)


class AttemptPolicyTests(unittest.TestCase):
    @staticmethod
    def attempt(passed, late_periods, error="capture mismatch"):
        return {
            "artifacts": {}, "play": {"late_periods": late_periods},
            "period_frames": 1920, "late_periods": late_periods,
            "inspect": {}, "capture": {}, "comparison": None,
            "subprocesses": {}, "errors": [] if passed else [error],
            "passed": passed,
        }

    def run_main(self, attempts, period_ms=10.0):
        with tempfile.TemporaryDirectory() as directory:
            summary = Path(directory) / "summary.json"
            fixture = Path(directory) / "fixture.eac3"
            fixture.write_bytes(b"fixture")
            fixture_result = (fixture, b"fixture", [b"frame"])
            with (mock.patch.object(check, "load_fixture", return_value=fixture_result),
                  mock.patch.object(check, "_run_logged", return_value={
                      "command": ["live", "bitstream", "probe"], "returncode": 0}),
                  mock.patch.object(check, "_decode_json_file", return_value=valid_probe()),
                  mock.patch.object(check, "_run_attempt", side_effect=attempts) as run_attempt,
                  mock.patch("builtins.print")):
                returncode = check.main([
                    "--ssr-live", "live", "--ssr-replay", "replay",
                    "--summary", str(summary), "--period-ms", str(period_ms)])
            return returncode, json.loads(summary.read_text(encoding="utf-8")), run_attempt

    def test_failed_late_attempt_retries_once_and_second_success_passes(self):
        returncode, document, run_attempt = self.run_main([
            self.attempt(False, 1), self.attempt(True, 0)])
        self.assertEqual(returncode, 0)
        self.assertEqual(run_attempt.call_count, 2)
        self.assertTrue(document["passed"])
        self.assertEqual(len(document["attempts"]), 2)
        self.assertEqual(document["late_periods"], 0)

    def test_failed_on_time_attempt_does_not_retry(self):
        returncode, document, run_attempt = self.run_main([self.attempt(False, 0)])
        self.assertEqual(returncode, 1)
        self.assertEqual(run_attempt.call_count, 1)
        self.assertEqual(document["errors"], ["capture mismatch"])

    def test_two_failed_late_attempts_report_stale_buffer_failure(self):
        returncode, document, run_attempt = self.run_main([
            self.attempt(False, 1), self.attempt(False, 2)])
        self.assertEqual(returncode, 1)
        self.assertEqual(run_attempt.call_count, 2)
        self.assertTrue(any("late by 2 periods" in error and "stale buffer" in error
                            for error in document["errors"]), document["errors"])

    def test_success_with_late_periods_passes_without_retry(self):
        returncode, document, run_attempt = self.run_main([self.attempt(True, 3)])
        self.assertEqual(returncode, 0)
        self.assertEqual(run_attempt.call_count, 1)
        self.assertEqual(document["late_periods"], 3)

    def test_zero_period_omits_player_period_argument(self):
        args = argparse.Namespace(ssr_live=Path("live"), seconds=8.0, period_ms=0.0)
        command = check._play_command(args, Path("fixture.eac3"))
        self.assertNotIn("--period-ms", command)
        args.period_ms = 10.0
        self.assertEqual(check._play_command(args, Path("fixture.eac3"))[-2:],
                         ["--period-ms", "10.0"])


class MainFailureTests(unittest.TestCase):
    def test_play_failure_kills_recorder_and_preserves_summary(self):
        with tempfile.TemporaryDirectory() as directory:
            summary = Path(directory) / "summary.json"
            process = mock.Mock()
            process.poll.return_value = None
            process.wait.return_value = -9

            def fake_run(command, stdout_path, _stderr_path, _timeout):
                if command[2] == "probe":
                    stdout_path.write_text(json.dumps(valid_probe()), encoding="utf-8")
                    return {"command": command, "returncode": 0}
                raise RuntimeError("play timed out")

            with (mock.patch.object(check, "_run_logged", side_effect=fake_run),
                  mock.patch.object(check.subprocess, "Popen", return_value=process),
                  mock.patch.object(check, "_wait_record_ready"),
                  mock.patch("builtins.print")):
                result = check.main([
                    "--ssr-live", "live", "--ssr-replay", "replay",
                    "--summary", str(summary)])
            self.assertEqual(result, 1)
            process.kill.assert_called_once_with()
            process.wait.assert_called_once_with(timeout=10)
            document = json.loads(summary.read_text(encoding="utf-8"))
            self.assertFalse(document["passed"])
            self.assertIn("play timed out", document["errors"])

    def test_probe_failure_still_writes_summary(self):
        with tempfile.TemporaryDirectory() as directory:
            summary = Path(directory) / "summary.json"
            completed = {"command": ["live", "bitstream", "probe", "--json"],
                         "returncode": 0}

            def fake_run(_command, stdout_path, _stderr_path, _timeout):
                stdout_path.write_text("not json", encoding="utf-8")
                return completed

            with mock.patch.object(check, "_run_logged", side_effect=fake_run):
                result = check.main([
                    "--ssr-live", "live", "--ssr-replay", "replay",
                    "--summary", str(summary)])
            self.assertEqual(result, 1)
            document = json.loads(summary.read_text(encoding="utf-8"))
            self.assertFalse(document["passed"])
            self.assertTrue(any("invalid JSON" in error for error in document["errors"]))
            artifacts = Path(document["artifacts"]["probe_stdout"]).parent
            self.assertTrue(artifacts.is_dir())
            self.assertNotEqual(artifacts, summary.parent / "summary-artifacts")


class ReportTests(unittest.TestCase):
    def test_play_report_requires_exact_counts(self):
        report = {"endpoint": 2, "codec": "eac3", "sent_bursts": 250,
                  "sent_frames": 250, "period_frames": 1920, "late_periods": 0}
        self.assertEqual(check.play_report_problems(report, 8), [])
        report["sent_bursts"] = 248
        report["sent_frames"] = 249
        report["error_hr"] = "0x88890008"
        problems = check.play_report_problems(report, 8)
        self.assertTrue(any("!=" in problem for problem in problems), problems)
        self.assertTrue(any("expected 250" in problem for problem in problems), problems)
        self.assertTrue(any("error_hr" in problem for problem in problems), problems)

    def test_play_report_requires_nonnegative_integer_timing_fields(self):
        report = {"endpoint": 2, "codec": "eac3", "sent_bursts": 250,
                  "sent_frames": 250, "period_frames": 1920, "late_periods": 0}
        for field, value in (("period_frames", -1), ("period_frames", 1.0),
                             ("late_periods", -1), ("late_periods", True)):
            with self.subTest(field=field, value=value):
                changed = dict(report)
                changed[field] = value
                self.assertTrue(any(field in problem for problem in
                                    check.play_report_problems(changed, 8)))

    def test_inspect_requires_empty_errors_and_zero_loss(self):
        report = {"errors": [], "lost_bytes": 0, "recorder_lost_bytes": 0,
                  "recorder_gap_markers": 0}
        self.assertEqual(check.inspect_problems(report), [])
        for field in ("lost_bytes", "recorder_lost_bytes", "recorder_gap_markers"):
            changed = dict(report)
            changed[field] = 1
            self.assertTrue(check.inspect_problems(changed))
        changed = dict(report)
        changed["errors"] = ["bad"]
        self.assertTrue(check.inspect_problems(changed))


if __name__ == "__main__":
    unittest.main()
