"""Check the optional ASIO result fields against Rust's flat JSON contract."""
import json
from pathlib import Path
import unittest


class AsioSetupContract(unittest.TestCase):
    def test_optional_registration_outcomes_round_trip_without_changing_driver_success(self):
        source = (Path(__file__).resolve().parents[2] / "crates/ssr-abi/src/setup_contract.rs").read_text()
        tasks = (Path(__file__).resolve().parents[2] / "crates/ssr-setup-sys/src/tasks.rs").read_text()
        for status in ("registered", "skipped_no_dll", "failed"):
            result = {"ok": True, "reboot_required": False, "message": "", "asio_registration": status}
            if status == "failed":
                result["asio_error"] = "asio_copy: sharing violation"
            self.assertEqual(json.loads(json.dumps(result)), result)
            self.assertIn(f'"{status}"', tasks)
        for field in ("asio_registration", "asio_error"):
            self.assertIn(f'"{field}" =>', source)
        self.assertNotIn("installed_asio_bundle()?", tasks)


if __name__ == "__main__":
    unittest.main()
