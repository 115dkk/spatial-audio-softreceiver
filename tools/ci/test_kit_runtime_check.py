import re
import unittest
from pathlib import Path

from kit_runtime_check import KIT_SCRIPT, kit_python_files

HERE = Path(__file__).resolve().parent


class KitRuntimeCheckTests(unittest.TestCase):
    def test_reads_the_python_files_the_kit_script_copies(self):
        text = "foreach ($name in 'RootDevice.cs', 'a_b.py', 'x.ps1', 'c.py', 'a_b.py') {"
        self.assertEqual(kit_python_files(text), ["a_b.py", "c.py"])

    def test_the_kit_script_names_files_that_exist(self):
        names = kit_python_files(KIT_SCRIPT.read_text(encoding="utf-8-sig"))
        self.assertIn("driver_flow_check.py", names)
        self.assertIn("numpy_compat.py", names)
        for name in names:
            self.assertTrue((HERE / name).is_file(), name)

    def test_every_python_the_guide_allows_has_exactly_one_numpy_pin(self):
        lines = [line for line in (HERE / "requirements-kit.txt").read_text(encoding="utf-8").splitlines()
                 if line.strip() and not line.startswith("#")]
        pins = []
        for line in lines:
            match = re.fullmatch(r"numpy==[0-9.]+; (.+)", line)
            self.assertIsNotNone(match, line)
            pins.append(match.group(1).replace("python_version", "version"))
        # Markers compare version strings as versions; tuples do the same here.
        for minor in range(10, 30):
            version = (3, minor)
            chosen = [marker for marker in pins if eval(
                re.sub(r'"3\.(\d+)"', r"(3, \1)", marker), {"version": version})]
            self.assertEqual(len(chosen), 1, f"Python 3.{minor}: {chosen}")
        guide = (HERE.parent.parent / "docs/testing/test-signed-pc.md").read_text(encoding="utf-8")
        self.assertIn("| Python | 3.10 이상", guide)


if __name__ == "__main__":
    unittest.main()
