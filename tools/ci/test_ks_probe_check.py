import math
import unittest

from ks_probe_check import probe_problems, tone_problems


def good_report(interface="standard"):
    return {"filter": r"\\?\ROOT#MEDIA#0000#{x}\wave", "pin": 0, "interface": interface,
            "rate": 48000, "channels": 2, "channel_mask": 3, "sample_format": "f32",
            "opened": True, "error": None, "endpoint_restored": True}


def tone(seconds=2.0, rate=48000, dbfs=-12.0, hz=1000.0, lead=0.3):
    amplitude = 10 ** (dbfs / 20)
    silence = [0.0] * int(lead * rate)
    body = [amplitude * math.sin(2 * math.pi * hz * n / rate) for n in range(int(seconds * rate))]
    return silence + body + silence


class ProbeTests(unittest.TestCase):
    def test_a_good_probe_passes(self):
        self.assertEqual(probe_problems(good_report(), "standard"), [])

    def test_a_pin_that_did_not_open_is_named_with_its_error(self):
        report = good_report()
        report.update(opened=False, interface=None, error="no sink pin")
        self.assertEqual(probe_problems(report, "standard"), ["the KS pin did not open: no sink pin"])

    def test_an_endpoint_left_disabled_fails_even_when_the_pin_played(self):
        report = good_report()
        report["endpoint_restored"] = False
        self.assertIn("the render endpoint was not active again after the probe",
                      probe_problems(report))

    def test_the_wrong_transport_fails(self):
        self.assertEqual(probe_problems(good_report("wavert"), "standard"),
                         ["the pin streamed through wavert, expected standard"])


class ToneTests(unittest.TestCase):
    def test_the_probe_tone_passes_on_every_channel(self):
        self.assertEqual(tone_problems([tone(), tone()], 48000), [])

    def test_a_quiet_or_wrong_tone_fails(self):
        found = tone_problems([tone(dbfs=-20.0), tone(hz=500.0)], 48000)
        self.assertTrue(any(p.startswith("channel 0: tone at -20.0 dBFS") for p in found), found)
        self.assertTrue(any(p.startswith("channel 1: tone at ") and p.endswith("Hz, expected 1000")
                            for p in found), found)

    def test_silence_fails(self):
        self.assertEqual(tone_problems([[0.0] * 48000], 48000),
                         ["channel 0: no tone of at least 0.5 s"])


if __name__ == "__main__":
    unittest.main()
