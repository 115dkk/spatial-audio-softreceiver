import json
import tempfile
import unittest
from pathlib import Path

from asio_probe_check import engine_report_problems, main, probe_problems

ORDER = ["DllGetClassObject", "CreateInstance", "init", "getChannels", "getBufferSize",
         "getSampleRate", "canSampleRate", "createBuffers", "getLatencies", "start", "stop",
         "disposeBuffers"]


def good_report(**changes):
    # 5 s at 48 kHz with 512 frames owes 468.75 callbacks.
    report = {"accepted": True, "call_order": ORDER, "callbacks": 460, "output_ready_calls": 460,
              "timeinfo_callbacks": 460, "reset_requests": 0, "input_latency": 0,
              "output_latency": 1024, "clock_fault_observed": False, "loaded_frames": 235520,
              "completed_frames": 230400, "seconds": 5, "sample_rate": 48000.0,
              "buffer_frames": 512, "output_channels": 12}
    report.update(changes)
    return report


class ProbeTests(unittest.TestCase):
    def test_a_run_with_the_engine_on_the_device_clock_passes(self):
        self.assertEqual(probe_problems(good_report(), "attached", True), [])

    def test_a_probe_that_rejected_the_dll_fails(self):
        problems = probe_problems(good_report(accepted=False, call_order=ORDER[:3]))
        self.assertEqual(len(problems), 1)
        self.assertIn("did not accept", problems[0])

    def test_callbacks_that_stop_when_the_engine_leaves_fail(self):
        # Half the run's worth: the callbacks stopped when ssr-live was killed.
        problems = probe_problems(good_report(callbacks=230), "any")
        self.assertEqual(len(problems), 1)
        self.assertIn("the clock owed 469", problems[0])

    def test_a_stand_in_clock_that_spins_fails(self):
        problems = probe_problems(good_report(callbacks=2000), "any")
        self.assertEqual(len(problems), 1)
        self.assertIn("faster than the rate", problems[0])

    def test_the_pace_follows_the_rate_and_the_buffer(self):
        # 3 s at 96 kHz with 64 frames owes 4,500 callbacks.
        report = good_report(seconds=3, sample_rate=96000.0, buffer_frames=64, callbacks=4400)
        self.assertEqual(probe_problems(report, "any"), [])
        report["callbacks"] = 3000
        self.assertEqual(len(probe_problems(report, "any")), 1)

    def test_an_engine_that_never_took_frames_fails_when_one_was_expected(self):
        for loaded, completed in [(None, None), (0, 0), (1024, 0)]:
            report = good_report(loaded_frames=loaded, completed_frames=completed)
            problems = probe_problems(report, "attached")
            self.assertEqual(len(problems), 1, (loaded, completed))
            self.assertIn("no engine took the frames", problems[0])

    def test_more_completed_than_loaded_fails(self):
        problems = probe_problems(good_report(completed_frames=300000), "attached")
        self.assertEqual(problems, ["300000 frames completed but only 235520 were loaded"])

    def test_an_engine_that_kept_only_its_last_generation_fails(self):
        # windows-2025 at VB-CABLE's minimum period: the output restarted 137
        # times in 12 s and each restart reset the counters.
        report = good_report(callbacks=577, loaded_frames=6144, completed_frames=6144)
        self.assertEqual(probe_problems(report, "attached"),
                         ["the engine took 6144 of the 295424 frames the callbacks produced "
                          "(at least 80% of them)"])

    def test_frames_loaded_with_no_engine_fail(self):
        self.assertEqual(probe_problems(good_report(loaded_frames=None, completed_frames=None), "absent"), [])
        self.assertEqual(probe_problems(good_report(loaded_frames=0, completed_frames=0), "absent"), [])
        self.assertEqual(probe_problems(good_report(), "absent"),
                         ["235520 frames were loaded with no engine running"])

    def test_the_stand_in_clock_fails_only_when_asked(self):
        report = good_report(clock_fault_observed=True)
        self.assertEqual(probe_problems(report, "attached"), [])
        self.assertEqual(probe_problems(report, "attached", True),
                         ["the callbacks fell back to the DLL's stand-in clock"])

    def test_missing_or_malformed_figures_fail(self):
        for key, value in [("seconds", 0), ("sample_rate", None), ("buffer_frames", True),
                           ("callbacks", -1)]:
            problems = probe_problems(good_report(**{key: value}))
            self.assertEqual(problems, ["the report lacks seconds, sample_rate, buffer_frames or callbacks"],
                             key)
        self.assertEqual(probe_problems([]), ["the probe report must be an object"])

    def test_an_engine_that_restarted_its_output_fails(self):
        self.assertEqual(engine_report_problems({"output_restarts": 0}), [])
        self.assertEqual(engine_report_problems({"output_restarts": 137}),
                         ["the engine restarted its output 137 times; each restart starts a new "
                          "generation and drops the DAW's frames"])
        self.assertEqual(engine_report_problems({}), ["the engine report lacks output_restarts"])
        self.assertEqual(engine_report_problems([]), ["the engine report must be an object"])

    def test_the_command_line_reads_the_file_and_exits_with_the_verdict(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "probe.json"
            path.write_text(json.dumps(good_report()), encoding="utf-8")
            self.assertEqual(main([str(path), "--engine", "attached", "--no-clock-fault"]), 0)
            path.write_text(json.dumps(good_report(callbacks=10)), encoding="utf-8")
            self.assertEqual(main([str(path)]), 1)
            self.assertEqual(main([str(Path(folder) / "missing.json")]), 1)
            path.write_text(json.dumps(good_report()), encoding="utf-8")
            engine = Path(folder) / "live.json"
            engine.write_text(json.dumps({"output_restarts": 0}), encoding="utf-8")
            self.assertEqual(main([str(path), "--engine", "attached", "--engine-report", str(engine)]), 0)
            engine.write_text(json.dumps({"output_restarts": 3}), encoding="utf-8")
            self.assertEqual(main([str(path), "--engine", "attached", "--engine-report", str(engine)]), 1)
            self.assertEqual(main([str(path), "--engine-report", str(Path(folder) / "none.json")]), 1)


if __name__ == "__main__":
    unittest.main()
