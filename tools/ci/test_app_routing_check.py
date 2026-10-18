"""Unit tests for app_routing_check's pure checks; no device."""

import unittest

from app_routing_check import endpoint_ids, helper_row, set_problems

SHOW = {"outputs": [
    {"id": "{0.0.0.00000000}.{main}", "name": "Spatial SoftReceiver", "endpoint": 0, "state": "active"},
    {"id": "{0.0.0.00000000}.{music}", "name": "x", "endpoint": 1, "state": "active"},
    {"id": "{0.0.0.00000000}.{pass}", "name": "y", "endpoint": 2, "state": "active"},
    {"id": "{0.0.0.00000000}.{old}", "name": "z", "endpoint": 2, "state": "not_present"},
    {"id": "{0.0.0.00000000}.{dac}", "name": "DAC", "endpoint": None, "state": "active"},
]}


class AppRoutingTests(unittest.TestCase):
    def test_endpoint_ids_keep_only_active_endpoints_of_ours(self):
        ids = endpoint_ids(SHOW)
        self.assertEqual(ids, {0: ["{0.0.0.00000000}.{main}"], 1: ["{0.0.0.00000000}.{music}"],
                               2: ["{0.0.0.00000000}.{pass}"]})

    def test_a_set_that_reads_its_entrance_back_passes(self):
        ids = endpoint_ids(SHOW)
        result = {"ok": True, "pid": 7, "app_entrance": "{0.0.0.00000000}.{pass}"}
        self.assertEqual(set_problems(2, result, 0, ids), [])
        self.assertEqual(set_problems(None, {"ok": True, "pid": 7, "app_entrance": None}, 0, ids), [])

    def test_a_failed_or_wrong_set_is_a_problem(self):
        ids = endpoint_ids(SHOW)
        self.assertTrue(set_problems(1, "unsupported", 1, ids)[0].startswith("set-app --endpoint 1 exited with 1"))
        wrong = {"ok": True, "pid": 7, "app_entrance": "{0.0.0.00000000}.{main}"}
        self.assertEqual(set_problems(1, wrong, 0, ids),
                         ["set-app --endpoint 1 read back {0.0.0.00000000}.{main}, expected {0.0.0.00000000}.{music}"])

    def test_the_helper_row_is_found_by_pid(self):
        apps = {"audio_apps": [{"pid": 3, "exe": "a.exe"}, {"pid": 9, "exe": "python.exe"}]}
        self.assertEqual(helper_row(apps, 9)["exe"], "python.exe")
        self.assertIsNone(helper_row(apps, 4))


if __name__ == "__main__":
    unittest.main()
