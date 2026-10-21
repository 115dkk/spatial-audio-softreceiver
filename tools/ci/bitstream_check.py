"""Probe and verify E-AC-3 bitstream capture through the installed driver.

    py bitstream_check.py --ssr-live ssr-live.exe --ssr-replay ssr-replay.exe \
        --summary bitstream.json

The executable-facing JSON is deliberately validated here rather than trusted.
Parsing, IEC 61937 deframing, and sequence comparison are pure and covered by
Linux unit tests in test_bitstream_check.py.
"""

import argparse
import hashlib
import json
import math
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time

from ssr_abi import (
    ABI_VERSION,
    COMPILED_CODECS,
    DUMP_FORMAT_VERSION,
    DUMP_HEADER,
    DUMP_MAGIC,
    FORMAT_EAC3,
    IEC61937_FAMILY,
    RECORD_ALIGN,
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
    RECORD_KIND_VOLUME,
    VOLUME_PAYLOAD,
    WAVEFORMATEX,
    WAVEFORMATEXTENSIBLE,
    WAVE_FORMAT_EXTENSIBLE,
    WAVE_FORMAT_IEEE_FLOAT,
    WAVE_FORMAT_PCM,
    WAVE_FAMILY,
    WAVE_GUID_TAIL,
)

CODECS = {
    1: "ac3",
    2: "eac3",
    3: "eac3_joc",
    4: "truehd",
    5: "mat20",
    6: "mat21",
    7: "dts",
    8: "dts_hd",
}
S_OK = "0x00000000"
FORMAT_REJECTIONS = {"0x88890008", "0x8889000E"}
MASKED_FORMAT_REJECTIONS = {"0x88890008"}
FIXTURE_SHA256 = "8cb8efb7e99d5dd698a5b7f79a010b0ba5ac43aeca8f49d56f077af7d6ecda54"
FIXTURE_RELATIVE = Path("crates/decode/tests/fixtures/ddp/tones-5.1.eac3")
EAC3_BURST_BYTES = 24_576
EAC3_FRAME_SECONDS = 0.032
IEC_SYNC = b"\x72\xf8\x1f\x4e"
KNOWN_RECORD_KINDS = {
    RECORD_KIND_DATA,
    RECORD_KIND_FORMAT_CHANGE,
    RECORD_KIND_STREAM_START,
    RECORD_KIND_STREAM_STOP,
    RECORD_KIND_OVERRUN,
    RECORD_KIND_VOLUME,
    RECORD_KIND_READ_END,
    RECORD_KIND_RECORDER_GAP,
}
HRESULT = re.compile(r"0x[0-9A-Fa-f]{8}\Z")


def _integer(value):
    return type(value) is int


def _hresult(value, where, problems):
    if not isinstance(value, str) or not HRESULT.fullmatch(value):
        problems.append(f"{where}: HRESULT must be 0x followed by eight hexadecimal digits")
        return None
    return value.upper().replace("X", "x", 1)


def probe_problems(document):
    """Validate the complete ``bitstream probe --json`` contract."""
    problems = []
    if not isinstance(document, dict) or set(document) != {"endpoints"}:
        return ["probe JSON must be an object containing only endpoints"]
    endpoints = document.get("endpoints")
    if not isinstance(endpoints, list):
        return ["probe endpoints must be an array"]

    rows = {}
    for index, row in enumerate(endpoints):
        where = f"probe endpoint row {index}"
        if not isinstance(row, dict) or set(row) != {"endpoint", "id", "codec_mask", "codecs"}:
            problems.append(f"{where}: malformed row")
            continue
        endpoint = row.get("endpoint")
        if not _integer(endpoint) or endpoint not in (0, 1, 2):
            problems.append(f"{where}: invalid endpoint {endpoint!r}")
            continue
        if endpoint in rows:
            problems.append(f"endpoint {endpoint}: duplicate row")
            continue
        rows[endpoint] = row

    for endpoint in (0, 1, 2):
        if endpoint not in rows:
            problems.append(f"endpoint {endpoint}: missing row")
            continue
        row = rows[endpoint]
        if not isinstance(row["id"], str) or not row["id"]:
            problems.append(f"endpoint {endpoint}: id must be a nonempty string")
        mask = row["codec_mask"]
        if not _integer(mask) or mask < 0:
            problems.append(f"endpoint {endpoint}: codec_mask must be a nonnegative integer")
            mask = 0
        elif mask & ~COMPILED_CODECS:
            problems.append(
                f"endpoint {endpoint}: codec_mask has unknown bits {mask & ~COMPILED_CODECS:#x}")
        codecs = row["codecs"]
        if not isinstance(codecs, list):
            problems.append(f"endpoint {endpoint}: codecs must be an array")
            continue
        by_bit = {}
        for index, codec in enumerate(codecs):
            where = f"endpoint {endpoint} codec row {index}"
            if not isinstance(codec, dict) or set(codec) != {
                    "bit", "codec", "supported_hr", "initialize_hr"}:
                problems.append(f"{where}: malformed row")
                continue
            bit = codec.get("bit")
            if not _integer(bit) or bit not in CODECS:
                problems.append(f"{where}: invalid bit {bit!r}")
                continue
            if bit in by_bit:
                problems.append(f"endpoint {endpoint}: duplicate codec bit {bit}")
                continue
            by_bit[bit] = codec
            if codec.get("codec") != CODECS[bit]:
                problems.append(
                    f"endpoint {endpoint} bit {bit}: codec is {codec.get('codec')!r}, "
                    f"expected {CODECS[bit]!r}")
        for bit in CODECS:
            if bit not in by_bit:
                problems.append(f"endpoint {endpoint}: missing codec bit {bit}")
                continue
            codec = by_bit[bit]
            supported = _hresult(codec.get("supported_hr"),
                                 f"endpoint {endpoint} bit {bit} supported_hr", problems)
            initialize_value = codec.get("initialize_hr")
            initialize = None
            if initialize_value is not None:
                initialize = _hresult(initialize_value,
                                      f"endpoint {endpoint} bit {bit} initialize_hr", problems)
            enabled = bool(mask & (1 << bit))
            if endpoint in (0, 1):
                if supported is not None and supported not in FORMAT_REJECTIONS:
                    problems.append(
                        f"endpoint {endpoint} bit {bit}: unexpected rejection {supported}")
                if initialize_value is not None:
                    problems.append(
                        f"endpoint {endpoint} bit {bit}: initialize_hr must be null")
            elif enabled:
                if supported != S_OK:
                    problems.append(
                        f"endpoint 2 bit {bit}: enabled codec support is {supported!r}, expected S_OK")
                if initialize != S_OK:
                    problems.append(
                        f"endpoint 2 bit {bit}: enabled codec initialize is {initialize!r}, expected S_OK")
            else:
                if supported is not None and supported not in MASKED_FORMAT_REJECTIONS:
                    problems.append(
                        f"endpoint 2 bit {bit}: masked codec returned {supported}")
                if initialize_value is not None:
                    problems.append(
                        f"endpoint 2 bit {bit}: masked codec initialize_hr must be null")

    endpoint_two = rows.get(2)
    if endpoint_two is not None and _integer(endpoint_two["codec_mask"]):
        if not endpoint_two["codec_mask"] & FORMAT_EAC3:
            problems.append("endpoint 2: E-AC-3 is disabled in codec_mask")
    return problems


def parse_eac3_frames(data):
    """Split the repository's strict 48 kHz, 5.1, six-block E-AC-3 stream."""
    frames = []
    offset = 0
    while offset < len(data):
        if len(data) - offset < 8:
            raise ValueError(f"truncated E-AC-3 header at byte {offset}")
        if data[offset:offset + 2] != b"\x0b\x77":
            raise ValueError(f"E-AC-3 sync missing at byte {offset}")
        if data[offset + 2] >> 6 != 0 or (data[offset + 2] >> 3) & 0x7 != 0:
            raise ValueError(f"unexpected E-AC-3 stream/substream at byte {offset}")
        frame_bytes = ((((data[offset + 2] & 0x07) << 8) | data[offset + 3]) + 1) * 2
        if frame_bytes < 8 or offset + frame_bytes > len(data):
            raise ValueError(f"truncated E-AC-3 frame at byte {offset}")
        fscod = data[offset + 4] >> 6
        blocks = (data[offset + 4] >> 4) & 0x03
        acmod = (data[offset + 4] >> 1) & 0x07
        lfe = data[offset + 4] & 1
        bsid = data[offset + 5] >> 3
        if (fscod, blocks, acmod, lfe, bsid) != (0, 3, 7, 1, 16):
            raise ValueError(
                f"unexpected E-AC-3 parameters at byte {offset}: "
                f"fscod={fscod}, numblkscod={blocks}, acmod={acmod}, lfe={lfe}, bsid={bsid}")
        frames.append(bytes(data[offset:offset + frame_bytes]))
        offset += frame_bytes
    if not frames:
        raise ValueError("E-AC-3 fixture contains no frames")
    return frames


def load_fixture(script_path=__file__):
    script = Path(script_path).resolve()
    packaged = script.parent / "fixtures" / FIXTURE_RELATIVE.name
    path = packaged if packaged.exists() else script.parents[2] / FIXTURE_RELATIVE
    data = path.read_bytes()
    digest = hashlib.sha256(data).hexdigest()
    if digest != FIXTURE_SHA256:
        raise ValueError(
            f"fixture SHA-256 is {digest}, expected committed synthetic fixture {FIXTURE_SHA256}")
    frames = parse_eac3_frames(data)
    return path, data, frames


def parse_wave_format(payload):
    """Strictly parse only PCM or the WAVEFORMATEXTENSIBLE IEC format needed here."""
    if len(payload) < WAVEFORMATEX.size:
        raise ValueError("truncated WAVEFORMATEX")
    tag, channels, rate, avg, block, bits, extra = WAVEFORMATEX.unpack_from(payload)
    required = WAVEFORMATEX.size + extra
    if required != len(payload):
        raise ValueError(
            f"WAVEFORMATEX size is {len(payload)}, but cbSize requires {required}")
    if not channels or not rate or not block or not bits or avg != rate * block:
        raise ValueError("inconsistent WAVEFORMATEX carrier fields")
    if tag in (WAVE_FORMAT_PCM, WAVE_FORMAT_IEEE_FLOAT):
        if extra != 0 or block != channels * ((bits + 7) // 8):
            raise ValueError("invalid plain PCM WAVEFORMATEX")
        return {"kind": "pcm", "tag": tag, "channels": channels,
                "rate": rate, "bits": bits}
    if tag != WAVE_FORMAT_EXTENSIBLE or len(payload) < WAVEFORMATEXTENSIBLE.size:
        raise ValueError(f"unsupported WAVEFORMATEX tag {tag:#x}")
    values = WAVEFORMATEXTENSIBLE.unpack_from(payload)
    valid_bits, channel_mask, subformat = values[7:]
    data1 = int.from_bytes(subformat[:4], "little")
    family = int.from_bytes(subformat[4:6], "little")
    if subformat[6:] != WAVE_GUID_TAIL:
        raise ValueError("unknown WAVEFORMATEXTENSIBLE GUID tail")
    if family == WAVE_FAMILY and data1 in (WAVE_FORMAT_PCM, WAVE_FORMAT_IEEE_FLOAT):
        if len(payload) != WAVEFORMATEXTENSIBLE.size or extra != 22:
            raise ValueError("invalid extensible PCM size")
        if block != channels * ((bits + 7) // 8) or not 0 < valid_bits <= bits:
            raise ValueError("invalid extensible PCM fields")
        return {"kind": "pcm", "tag": tag, "subformat": data1,
                "channels": channels, "rate": rate, "bits": bits,
                "valid_bits": valid_bits, "channel_mask": channel_mask}
    if family != IEC61937_FAMILY or len(payload) != 52 or extra != 34:
        raise ValueError("unsupported WAVEFORMATEXTENSIBLE bitstream subtype")
    encoded_rate = int.from_bytes(payload[40:44], "little")
    encoded_channels = int.from_bytes(payload[44:48], "little")
    encoded_avg = int.from_bytes(payload[48:52], "little")
    codec = {0x000A: "eac3", 0x010A: "eac3_joc", 0x000C: "truehd",
             0x010C: "mat20", 0x030C: "mat21", 0x070C: "mat21",
             0x000B: "dts_hd", 0x010B: "dts_hd", 0x030B: "dts_hd"}.get(data1)
    result = {"kind": "bitstream", "codec": codec or f"unknown_{data1:#x}",
              "subtype": data1, "carrier_rate": rate, "carrier_channels": channels,
              "carrier_bits": bits, "valid_bits": valid_bits,
              "channel_mask": channel_mask, "encoded_rate": encoded_rate,
              "encoded_channels": encoded_channels, "encoded_avg_bytes": encoded_avg}
    if codec == "eac3" and (rate, channels, bits, valid_bits, block, encoded_rate,
                            encoded_channels) != (192_000, 2, 16, 16, 4, 48_000, 6):
        raise ValueError(f"unexpected E-AC-3 carrier: {result}")
    return result


def parse_dump(data):
    """Parse a current dump with strict header, record, alignment, and padding checks."""
    if len(data) < DUMP_HEADER.size:
        raise ValueError("dump is shorter than its header")
    magic, abi, header_size, qpc, format_version, reserved = DUMP_HEADER.unpack_from(data)
    if magic != DUMP_MAGIC:
        raise ValueError("dump magic is not SSRDUMP")
    if abi != ABI_VERSION or header_size != DUMP_HEADER.size:
        raise ValueError(f"unexpected dump ABI/header size {abi}/{header_size}")
    if qpc <= 0 or format_version != DUMP_FORMAT_VERSION or reserved != 0:
        raise ValueError("invalid dump frequency, format version, or reserved field")
    records = []
    offset = header_size
    while offset < len(data):
        if len(data) - offset < RECORD_HEADER.size:
            raise ValueError(f"truncated record header at byte {offset}")
        values = RECORD_HEADER.unpack_from(data, offset)
        record = dict(zip(("kind", "flags", "epoch", "byte_pos", "qpc", "qpc_written",
                           "len", "endpoint", "reserved"), values))
        if record["kind"] not in KNOWN_RECORD_KINDS:
            raise ValueError(f"unknown record kind {record['kind']} at byte {offset}")
        if record["reserved"] != 0 or record["flags"] & ~(RECORD_FLAG_DISCONTINUITY | RECORD_FLAG_REPEAT):
            raise ValueError(f"invalid record reserved/flags at byte {offset}")
        payload_start = offset + RECORD_HEADER.size
        payload_end = payload_start + record["len"]
        aligned_end = offset + (RECORD_HEADER.size + record["len"] + RECORD_ALIGN - 1) // RECORD_ALIGN * RECORD_ALIGN
        if payload_end > len(data) or aligned_end > len(data):
            raise ValueError(f"truncated record payload at byte {offset}")
        if any(data[payload_end:aligned_end]):
            raise ValueError(f"nonzero record padding at byte {offset}")
        record["payload"] = bytes(data[payload_start:payload_end])
        record["offset"] = offset
        recorder_kind = record["kind"] in (RECORD_KIND_READ_END, RECORD_KIND_RECORDER_GAP)
        if recorder_kind != (record["endpoint"] == RECORD_ENDPOINT_RECORDER):
            raise ValueError(f"record endpoint does not match kind at byte {offset}")
        if not recorder_kind and record["endpoint"] not in (0, 1, 2, 3):
            raise ValueError(f"invalid endpoint at byte {offset}")
        if record["kind"] in (RECORD_KIND_STREAM_START, RECORD_KIND_STREAM_STOP,
                              RECORD_KIND_READ_END) and record["len"] != 0:
            raise ValueError(f"event record has a payload at byte {offset}")
        if record["kind"] in (RECORD_KIND_OVERRUN, RECORD_KIND_RECORDER_GAP) and record["len"] != 8:
            raise ValueError(f"loss record payload is not eight bytes at byte {offset}")
        if record["kind"] == RECORD_KIND_VOLUME and record["len"] != VOLUME_PAYLOAD.size:
            raise ValueError(f"volume payload has wrong size at byte {offset}")
        if record["kind"] == RECORD_KIND_FORMAT_CHANGE:
            record["format"] = parse_wave_format(record["payload"])
        records.append(record)
        offset = aligned_end
    return {
        "magic": magic.decode("ascii", "strict").rstrip("\0"),
        "abi_version": abi,
        "header_size": header_size,
        "qpc_frequency": qpc,
        "format_version": format_version,
        "records": len(records),
    }, records


def endpoint_two_segments(records):
    """Return E-AC-3 data segments, resetting at every stream integrity boundary."""
    current_format = None
    current_epoch = None
    eac3_formats = []
    segments = []
    current = None
    previous_end = None
    problems = []
    eac3_data_bytes = 0
    start_epoch = None

    def reset():
        nonlocal current, previous_end
        if current:
            segments.append(current)
        current = None
        previous_end = None

    for record in records:
        kind = record["kind"]
        if kind == RECORD_KIND_RECORDER_GAP:
            reset()
            start_epoch = None
            problems.append("recorder gap marker in dump")
            continue
        if record["endpoint"] != 2:
            continue
        if kind == RECORD_KIND_FORMAT_CHANGE:
            reset()
            start_epoch = None
            current_format = record["format"]
            current_epoch = record["epoch"]
            if current_format.get("codec") == "eac3" and current_format not in eac3_formats:
                eac3_formats.append(current_format)
        elif kind == RECORD_KIND_STREAM_START:
            reset()
            start_epoch = record["epoch"] if record["epoch"] == current_epoch else None
        elif kind == RECORD_KIND_STREAM_STOP:
            reset()
            start_epoch = None
        elif kind == RECORD_KIND_OVERRUN:
            reset()
            start_epoch = None
            lost = int.from_bytes(record["payload"], "little")
            problems.append(f"endpoint 2 overrun lost {lost} bytes")
        elif kind == RECORD_KIND_DATA:
            allowed_discontinuity = start_epoch == record["epoch"]
            start_epoch = None
            if current_format is None:
                if record["flags"] & RECORD_FLAG_DISCONTINUITY:
                    problems.append(
                        f"endpoint 2 discontinuity at byte_pos {record['byte_pos']}")
                problems.append("endpoint 2 data before a format record")
                continue
            if record["epoch"] != current_epoch:
                reset()
                problems.append(
                    f"endpoint 2 data epoch {record['epoch']} does not match format epoch {current_epoch}")
                continue
            if previous_end is not None and record["byte_pos"] != previous_end:
                expected_position = previous_end
                reset()
                problems.append(
                    f"endpoint 2 E-AC-3 byte_pos jumped from {expected_position} to {record['byte_pos']}")
            if record["flags"] & RECORD_FLAG_DISCONTINUITY and not allowed_discontinuity:
                reset()
                problems.append(f"endpoint 2 discontinuity at byte_pos {record['byte_pos']}")
            if current_format["kind"] == "pcm":
                reset()
                continue
            if current_format.get("codec") != "eac3":
                reset()
                problems.append(
                    f"endpoint 2 data used unexpected codec {current_format.get('codec')}")
                continue
            if current is None:
                current = []
            current.append(record["payload"])
            previous_end = record["byte_pos"] + record["len"]
            eac3_data_bytes += record["len"]
    reset()
    return segments, {
        "segments": len(segments),
        "data_bytes": eac3_data_bytes,
        "formats": eac3_formats,
    }, problems


def deframe_eac3_chunks(chunks):
    """Deframe one continuity segment; arbitrary chunk boundaries are harmless."""
    stream = b"".join(chunks)
    first = stream.find(IEC_SYNC)
    if first < 0:
        if any(stream):
            raise ValueError("E-AC-3 segment has nonzero data but no IEC 61937 preamble")
        return []
    payloads = []
    offset = first
    view = memoryview(stream)
    while len(stream) - offset >= EAC3_BURST_BYTES:
        burst = stream[offset:offset + EAC3_BURST_BYTES]
        if burst[:4] != IEC_SYNC:
            if not any(burst) and not any(view[offset + EAC3_BURST_BYTES:]):
                break
            raise ValueError(f"IEC 61937 preamble missing at segment byte {offset}")
        pc = int.from_bytes(burst[4:6], "little")
        pd = int.from_bytes(burst[6:8], "little")
        if pc != 21:
            raise ValueError(f"unexpected IEC 61937 Pc {pc} at segment byte {offset}")
        occupied = 8 + ((pd + 1) // 2) * 2
        if pd <= 0 or occupied > EAC3_BURST_BYTES:
            raise ValueError(f"invalid IEC 61937 Pd {pd} at segment byte {offset}")
        carrier = burst[8:occupied]
        decoded = bytearray()
        for index in range(0, len(carrier), 2):
            decoded.extend((carrier[index + 1], carrier[index]))
        if pd & 1 and decoded[pd] != 0:
            raise ValueError(f"nonzero unused payload byte at segment byte {offset}")
        if any(burst[occupied:]):
            raise ValueError(f"nonzero IEC 61937 padding at segment byte {offset}")
        payloads.append(bytes(decoded[:pd]))
        offset += EAC3_BURST_BYTES
    trailing = stream[offset:]
    if any(trailing):
        raise ValueError(f"truncated IEC 61937 burst at segment byte {offset}")
    return payloads


def compare_payloads(source_frames, received, sent_count, minimum_ratio=0.90):
    """Require received payloads to be one finite contiguous slice of sent playback."""
    if not _integer(sent_count) or sent_count <= 0:
        raise ValueError("sent frame count must be positive")
    expected = [source_frames[index % len(source_frames)] for index in range(sent_count)]
    count = len(received)
    ratio = count / sent_count
    if count > sent_count:
        raise ValueError(f"received {count} frames, more than {sent_count} sent")
    if ratio + 1e-12 < minimum_ratio:
        raise ValueError(f"received/sent ratio {ratio:.3f} is below {minimum_ratio:.3f}")
    alignments = []
    for start in range(sent_count - count + 1):
        if expected[start:start + count] == received:
            alignments.append(start)
    if not alignments:
        raise ValueError("received payloads are not a contiguous finite slice of sent frames")
    start = alignments[0]
    return {
        "sent_frames": sent_count,
        "received_frames": count,
        "received_to_sent": ratio,
        "missing_prefix": start,
        "missing_suffix": sent_count - start - count,
        "source_alignment": start % len(source_frames),
        "possible_alignments": len(alignments),
    }


def play_report_problems(document, seconds):
    problems = []
    if not isinstance(document, dict):
        return ["play JSON must be an object"]
    required = {"endpoint", "codec", "sent_bursts", "sent_frames"}
    if not required <= set(document):
        return [f"play JSON is missing {sorted(required - set(document))}"]
    if document.get("endpoint") != 2 or document.get("codec") != "eac3":
        problems.append("play JSON endpoint/codec is not 2/eac3")
    for field in ("error", "error_hr"):
        if field in document and document[field] is not None:
            problems.append(f"play JSON {field} is {document[field]!r}, expected null")
    bursts = document.get("sent_bursts")
    frames = document.get("sent_frames")
    expected = math.ceil(seconds / EAC3_FRAME_SECONDS)
    if not _integer(bursts) or not _integer(frames) or frames <= 0:
        problems.append("play JSON sent counts must be positive integers")
    elif bursts != frames:
        problems.append(f"play JSON sent_bursts {bursts} != sent_frames {frames}")
    if frames != expected:
        problems.append(f"play JSON sent_frames {frames!r}, expected {expected}")
    return problems


def inspect_problems(document):
    if not isinstance(document, dict):
        return ["inspect JSON must be an object"]
    problems = []
    errors = document.get("errors")
    if not isinstance(errors, list):
        problems.append("inspect errors must be an array")
    elif errors:
        problems.append(f"inspect reported errors: {errors!r}")
    for field in ("lost_bytes", "recorder_lost_bytes", "recorder_gap_markers"):
        value = document.get(field)
        if not _integer(value):
            problems.append(f"inspect {field} must be an integer")
        elif value != 0:
            problems.append(f"inspect {field} is {value}, expected zero")
    return problems


def _decode_json_file(path, label):
    try:
        return json.loads(path.read_text(encoding="utf-8-sig"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise RuntimeError(f"{label} returned invalid JSON: {error}") from error


def _run_logged(command, stdout_path, stderr_path, timeout):
    with stdout_path.open("wb") as stdout, stderr_path.open("wb") as stderr:
        try:
            completed = subprocess.run(command, stdout=stdout, stderr=stderr,
                                       check=False, timeout=timeout)
        except (OSError, subprocess.SubprocessError) as error:
            raise RuntimeError(f"cannot run {' '.join(map(str, command))}: {error}") from error
    if completed.returncode != 0:
        raise RuntimeError(f"{' '.join(map(str, command))} exited with {completed.returncode}")
    return {"command": [str(item) for item in command], "returncode": completed.returncode}


def _wait_record_ready(process, dump_path, timeout=15.0):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        returncode = process.poll()
        if returncode is not None:
            raise RuntimeError(f"record exited before playback with {returncode}")
        # File::create follows a successful DriverRing open and Recorder::start.
        if dump_path.exists():
            return
        time.sleep(0.05)
    raise RuntimeError("record did not create its dump before the readiness timeout")


def _write_summary(path, result):
    text = json.dumps(result, indent=2, ensure_ascii=True)
    path.write_text(text + "\n", encoding="utf-8")
    print(text)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--ssr-live", required=True, type=Path)
    parser.add_argument("--ssr-replay", required=True, type=Path)
    parser.add_argument("--summary", required=True, type=Path)
    parser.add_argument("--seconds", type=float, default=8.0)
    args = parser.parse_args(argv)
    if not math.isfinite(args.seconds) or not 0 < args.seconds <= 3600:
        parser.error("--seconds must be a positive finite number no greater than 3600")

    summary_path = args.summary.resolve()
    summary_path.parent.mkdir(parents=True, exist_ok=True)
    artifacts = Path(tempfile.mkdtemp(prefix=summary_path.stem + "-", dir=summary_path.parent))
    paths = {
        "dump": artifacts / "capture.ssrdump",
        "probe_stdout": artifacts / "probe.stdout.json",
        "probe_stderr": artifacts / "probe.stderr.log",
        "record_stdout": artifacts / "record.stdout.log",
        "record_stderr": artifacts / "record.stderr.log",
        "play_stdout": artifacts / "play.stdout.json",
        "play_stderr": artifacts / "play.stderr.log",
        "inspect_stdout": artifacts / "inspect.stdout.json",
        "inspect_stderr": artifacts / "inspect.stderr.log",
    }
    result = {
        "seconds": args.seconds,
        "artifacts": {key: str(path) for key, path in paths.items()},
        "fixture": None,
        "probe": None,
        "play": None,
        "inspect": None,
        "capture": None,
        "comparison": None,
        "subprocesses": {},
        "errors": [],
        "passed": False,
    }
    record = None
    record_stdout = None
    record_stderr = None
    try:
        fixture_path, fixture_data, source_frames = load_fixture()
        result["fixture"] = {
            "path": str(fixture_path),
            "sha256": FIXTURE_SHA256,
            "bytes": len(fixture_data),
            "frames": len(source_frames),
            "frame_bytes": sorted(set(map(len, source_frames))),
            "sample_rate": 48_000,
            "channels": 6,
            "samples_per_frame": 1536,
        }

        probe_command = [str(args.ssr_live), "bitstream", "probe", "--json"]
        result["subprocesses"]["probe"] = _run_logged(
            probe_command, paths["probe_stdout"], paths["probe_stderr"], 60)
        result["probe"] = _decode_json_file(paths["probe_stdout"], "bitstream probe")
        probe_errors = probe_problems(result["probe"])
        result["errors"].extend(probe_errors)
        if probe_errors:
            raise RuntimeError("probe validation failed")

        try:
            paths["dump"].unlink()
        except FileNotFoundError:
            pass
        capture_seconds = args.seconds + 4.0
        record_command = [str(args.ssr_live), "record", "--source", "driver", "--out",
                          str(paths["dump"]), "--seconds", str(capture_seconds)]
        record_stdout = paths["record_stdout"].open("wb")
        record_stderr = paths["record_stderr"].open("wb")
        try:
            record = subprocess.Popen(record_command, stdout=record_stdout, stderr=record_stderr)
        except OSError as error:
            raise RuntimeError(f"cannot start {' '.join(record_command)}: {error}") from error
        result["subprocesses"]["record"] = {
            "command": record_command,
            "capture_seconds": capture_seconds,
        }
        _wait_record_ready(record, paths["dump"])

        play_command = [str(args.ssr_live), "bitstream", "play", "--endpoint", "2",
                        "--codec", "eac3", "--input", str(fixture_path), "--seconds",
                        str(args.seconds), "--json"]
        result["subprocesses"]["play"] = _run_logged(
            play_command, paths["play_stdout"], paths["play_stderr"], args.seconds + 30)
        result["play"] = _decode_json_file(paths["play_stdout"], "bitstream play")
        play_errors = play_report_problems(result["play"], args.seconds)
        result["errors"].extend(play_errors)
        if record.poll() is not None:
            result["errors"].append(
                f"record exited before playback completed with {record.returncode}")
        if play_errors or result["errors"]:
            raise RuntimeError("playback validation failed")

        try:
            record_returncode = record.wait(timeout=capture_seconds + 15)
        except subprocess.TimeoutExpired as error:
            raise RuntimeError("record did not finish before its timeout") from error
        result["subprocesses"]["record"]["returncode"] = record_returncode
        if record_returncode != 0:
            raise RuntimeError(f"record exited with {record_returncode}")
        record = None
        record_stdout.close()
        record_stderr.close()
        record_stdout = record_stderr = None

        inspect_command = [str(args.ssr_replay), "inspect", str(paths["dump"])]
        result["subprocesses"]["inspect"] = _run_logged(
            inspect_command, paths["inspect_stdout"], paths["inspect_stderr"], 60)
        result["inspect"] = _decode_json_file(paths["inspect_stdout"], "ssr-replay inspect")
        result["errors"].extend(inspect_problems(result["inspect"]))

        header, records = parse_dump(paths["dump"].read_bytes())
        segments, capture, segment_errors = endpoint_two_segments(records)
        result["capture"] = {"header": header, **capture, "bursts": 0}
        result["errors"].extend(segment_errors)
        payloads = []
        for index, segment in enumerate(segments):
            try:
                payloads.extend(deframe_eac3_chunks(segment))
            except ValueError as error:
                result["errors"].append(f"E-AC-3 segment {index}: {error}")
        result["capture"]["bursts"] = len(payloads)
        if not capture["data_bytes"]:
            result["errors"].append("capture has no endpoint 2 data recorded under E-AC-3 format")
        if not payloads:
            result["errors"].append("capture has no complete E-AC-3 IEC 61937 bursts")
        if not result["errors"]:
            try:
                result["comparison"] = compare_payloads(
                    source_frames, payloads, result["play"]["sent_frames"])
            except ValueError as error:
                result["errors"].append(str(error))
    except Exception as error:
        message = str(error) or type(error).__name__
        if message not in result["errors"] and message not in (
                "probe validation failed", "playback validation failed"):
            result["errors"].append(message)
    finally:
        if record is not None and record.poll() is None:
            record.kill()
            try:
                record.wait(timeout=10)
            except subprocess.TimeoutExpired:
                result["errors"].append("record could not be reaped after kill")
        if record_stdout is not None:
            record_stdout.close()
        if record_stderr is not None:
            record_stderr.close()
        result["passed"] = not result["errors"]
        try:
            _write_summary(summary_path, result)
        except OSError as error:
            print(f"cannot write summary {summary_path}: {error}", file=sys.stderr)
            return 1
    for error in result["errors"]:
        print(error, file=sys.stderr)
    return 0 if result["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
