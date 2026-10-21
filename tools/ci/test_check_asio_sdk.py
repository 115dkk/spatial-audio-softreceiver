#!/usr/bin/env python3
"""Small synthetic headers test the SDK checker without downloading an SDK."""
import hashlib
import importlib.util
from pathlib import Path
import tempfile
import subprocess
import sys
import unittest
import zipfile

SPEC = importlib.util.spec_from_file_location("check_asio_sdk", Path(__file__).with_name("check-asio-sdk.py"))
CHECK = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECK)
HEADER = "enum { ASIOSTFloat32LSB=19, Next, Bad=-1000, Mask=1<<3 };"
INTERFACE = "interface IASIO { virtual ASIOBool init(void*)=0; virtual ASIOError start()=0; };"
RUST = '''pub const ASIOSTFloat32LSB: i32 = 19;
pub const Next: i32 = 20;
pub const Bad: i32 = -1000;
pub const Mask: i32 = 8;
pub struct AsioVtable {
 pub init: unsafe extern "system" fn(),
 pub start: unsafe extern "system" fn(),
}
'''


class SdkComparison(unittest.TestCase):
    def test_values_not_just_names(self):
        CHECK.compare(HEADER, INTERFACE, RUST)
        with self.assertRaisesRegex(ValueError, "enum mismatch"):
            CHECK.compare(HEADER, INTERFACE, RUST.replace("= 19", "= 18"))

    def test_implicit_negative_and_shift(self):
        self.assertEqual(CHECK.enums("enum { A=-1000,B,C=1<<4,D=C+1 };"),
                         {"A": -1000, "B": -999, "C": 16, "D": 17})

    def test_method_order(self):
        with self.assertRaisesRegex(ValueError, "method order"):
            CHECK.compare(HEADER, INTERFACE, RUST.replace("pub init:", "pub stop:"))

    def test_missing_enum(self):
        with self.assertRaisesRegex(ValueError, "enum mismatch"):
            CHECK.compare(HEADER + "enum { Added=7 };", INTERFACE, RUST)

    def test_no_expression_execution(self):
        with self.assertRaises(ValueError):
            CHECK.integer("__import__('os').system('false')", {})

    def test_digest_before_headers(self):
        with tempfile.TemporaryDirectory() as directory:
            archive, rust = Path(directory) / "sdk.zip", Path(directory) / "sdk.rs"
            rust.write_text(RUST)
            with zipfile.ZipFile(archive, "w") as sdk:
                sdk.writestr("ASIOSDK/common/asio.h", HEADER)
                sdk.writestr("ASIOSDK/common/iasiodrv.h", INTERFACE)
            with self.assertRaisesRegex(ValueError, "SHA-256 mismatch"):
                CHECK.verify(archive, rust)
            digest = hashlib.sha256(archive.read_bytes()).hexdigest()
            CHECK.verify(archive, rust, digest)
            command = [sys.executable, '-I', str(Path(__file__).with_name('check-asio-sdk.py')),
                       '--archive', str(archive), '--rust', str(rust), '--sha256', digest]
            self.assertEqual(subprocess.run(command, capture_output=True).returncode, 0)
            command[-1] = '0' * 64
            self.assertNotEqual(subprocess.run(command, capture_output=True).returncode, 0)


if __name__ == "__main__":
    unittest.main()
