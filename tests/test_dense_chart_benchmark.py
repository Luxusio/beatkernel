"""Independent literal evidence fixtures for the opt-in dense chart adapter."""
import contextlib
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


SOURCE = Path(__file__).resolve().parents[1] / "tools/dense_chart_benchmark.py"
SPEC = importlib.util.spec_from_file_location("dense_chart_benchmark", SOURCE)
benchmark = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = benchmark
SPEC.loader.exec_module(benchmark)

OPTIONS = dict(notes=19, lanes=4, seek_cycles=8, origin_ns=100)
# Five rows: four holds at row zero, three holds in the partial final row.
# The last probe is exactly the final hold tail; the narrow window retains it.
FIXTURE = """benchmark_schema=1 workload_id=beatkernel-dense-chart-v1 debug_assertions=false
notes=19 lanes=4 seek_cycles=8 origin_ns=100
hold_count=7 record_count=38 result_count=26 checkpoint_count=3
seek_checks=8 projection_checks=8 max_visible=8 final_engine_hash=123 retained_probes=8
last_probe_target_ns=18000100 last_probe_cursor=38 last_probe_result_count=26 last_probe_engine_hash=456 last_probe_visible_count=3
setup_ns=1 recording_ns=2 seek_ns=3 projection_ns=4 verification_ns=5
software-only; phase timings are informational observations with no performance threshold, ranking or universal zero-cost claim
audio=unavailable native_clocks_latency=unavailable gpu_rendering=unavailable physical_devices=unavailable whole_player_behavior=unavailable actual_wall_clock_soak=unavailable
long origins test absolute timestamp placement only; lanes are not multiplayer members
"""

# These values are literal independently calculated contract facts, not output
# from the adapter's oracle. Hashes identify fixture cases, not real Rust state.
FIXED = (
    (dict(notes=20000, lanes=8, seek_cycles=16, origin_ns=0),
     dict(hold_count=5000, record_count=40000, result_count=25000,
          checkpoint_count=3, seek_checks=16, projection_checks=16,
          max_visible=24, final_engine_hash=111, retained_probes=16,
          last_probe_target_ns=48000000, last_probe_cursor=200,
          last_probe_result_count=128, last_probe_engine_hash=222,
          last_probe_visible_count=24)),
    (dict(notes=100000, lanes=64, seek_cycles=4, origin_ns=0),
     dict(hold_count=25024, record_count=200000, result_count=125024,
          checkpoint_count=3, seek_checks=4, projection_checks=4,
          max_visible=128, final_engine_hash=333, retained_probes=4,
          last_probe_target_ns=3000000, last_probe_cursor=128,
          last_probe_result_count=128, last_probe_engine_hash=444,
          last_probe_visible_count=128)),
)


def literal_summary(options, facts):
    """Format supplied literal facts; deliberately does no oracle arithmetic."""
    lines = FIXTURE.splitlines()
    groups = ((1, tuple(OPTIONS)),
              (2, ("hold_count", "record_count", "result_count", "checkpoint_count")),
              (3, ("seek_checks", "projection_checks", "max_visible", "final_engine_hash", "retained_probes")),
              (4, ("last_probe_target_ns", "last_probe_cursor", "last_probe_result_count", "last_probe_engine_hash", "last_probe_visible_count")))
    for index, keys in groups:
        values = options if index == 1 else facts
        lines[index] = " ".join(f"{key}={values[key]}" for key in keys)
    return "\n".join(lines) + "\n"


def fixture_for(args):
    for options, facts in FIXED:
        if args == argv_for(options):
            return literal_summary(options, facts)
    raise AssertionError(f"unexpected workload argv: {args!r}")


def argv_for(options):
    return [item for key in ("notes", "lanes", "seek_cycles", "origin_ns")
            for item in ("--" + key.replace("_", "-"), str(options[key]))]


class ParserTests(unittest.TestCase):
    def test_literal_partial_hold_row_and_tail(self):
        result = benchmark.parse_measurements(FIXTURE, OPTIONS)
        self.assertEqual(result["options"], OPTIONS)
        self.assertFalse(result["debug_assertions"])
        for key, value in dict(hold_count=7, record_count=38, result_count=26,
                              checkpoint_count=3, max_visible=8,
                              last_probe_target_ns=18000100, last_probe_cursor=38,
                              last_probe_result_count=26, last_probe_visible_count=3).items():
            self.assertEqual(result[key], value, key)

    def test_independent_fixed_matrix_facts(self):
        for options, facts in FIXED:
            with self.subTest(options=options):
                result = benchmark.parse_measurements(literal_summary(options, facts), options)
                for key, value in facts.items():
                    self.assertEqual(result[key], value, key)

    def test_literal_prefix_tail_inclusive_window_and_backseek(self):
        # Eleven rows with holds at 0, 4 and 8. Cycle five visits the
        # final singleton row, then cycle six seeks backwards to row four.
        # At row four's head, the inclusive window includes rows 3, 4, 5.
        for seeks, target, cursor, results, visible, maximum in (
                (5, 43000000, 42, 27, 1, 4),
                (6, 16000000, 18, 12, 6, 6),
                (7, 17000000, 18, 12, 4, 6),
                (8, 18000000, 20, 14, 2, 6),
                (11, 32000000, 34, 22, 6, 6)):
            options = dict(notes=21, lanes=2, seek_cycles=seeks, origin_ns=0)
            facts = dict(hold_count=6, record_count=42, result_count=27,
                         checkpoint_count=3, seek_checks=seeks, projection_checks=seeks,
                         retained_probes=seeks, max_visible=maximum,
                         final_engine_hash=123, last_probe_engine_hash=456,
                         last_probe_target_ns=target, last_probe_cursor=cursor,
                         last_probe_result_count=results, last_probe_visible_count=visible)
            with self.subTest(seeks=seeks):
                result = benchmark.parse_measurements(literal_summary(options, facts), options)
                self.assertEqual(result["last_probe_cursor"], cursor)
                self.assertEqual(result["last_probe_visible_count"], visible)

    def test_u64_hash_and_u128_phase_boundaries(self):
        text = FIXTURE.replace("final_engine_hash=123", f"final_engine_hash={2**64 - 1}")
        text = text.replace("last_probe_engine_hash=456", "last_probe_engine_hash=0")
        for key, value in (("setup_ns", 1), ("recording_ns", 2), ("seek_ns", 3),
                           ("projection_ns", 4), ("verification_ns", 5)):
            text = text.replace(f"{key}={value}", f"{key}={2**128 - 1}")
        result = benchmark.parse_measurements(text, OPTIONS)
        self.assertEqual(result["final_engine_hash"], 2**64 - 1)
        self.assertEqual(result["last_probe_engine_hash"], 0)
        for key in ("setup_ns", "recording_ns", "seek_ns", "projection_ns", "verification_ns"):
            self.assertEqual(result[key], 2**128 - 1)

    def test_valid_minimum_notes_maximum_lanes_cycles_and_origin(self):
        options = dict(notes=1, lanes=64, seek_cycles=1000, origin_ns=604800000000000)
        facts = dict(hold_count=1, record_count=2, result_count=2, checkpoint_count=3,
                     seek_checks=1000, projection_checks=1000, retained_probes=1000,
                     max_visible=1, final_engine_hash=123, last_probe_engine_hash=456,
                     last_probe_target_ns=604800003000000, last_probe_cursor=2,
                     last_probe_result_count=2, last_probe_visible_count=1)
        result = benchmark.parse_measurements(literal_summary(options, facts), options)
        self.assertEqual(result["options"], options)
        self.assertEqual(result["last_probe_target_ns"], 604800003000000)

    def test_structure_and_semantic_corruption_is_rejected(self):
        mutations = (
            ("benchmark_schema=1", "benchmark_schema=2"),
            ("beatkernel-dense-chart-v1", "beatkernel-dense-chart-v2"),
            ("debug_assertions=false", "debug_assertions=maybe"),
            ("origin_ns=100", "origin_ns=101"),
            ("hold_count=7", "hold_count=8"),
            ("record_count=38", "record_count=37"),
            ("result_count=26", "result_count=25"),
            ("checkpoint_count=3", "checkpoint_count=2"),
            ("seek_checks=8", "seek_checks=7"),
            ("projection_checks=8", "projection_checks=9"),
            ("retained_probes=8", "retained_probes=7"),
            ("max_visible=8", "max_visible=7"),
            ("last_probe_target_ns=18000100", "last_probe_target_ns=18000000"),
            ("last_probe_cursor=38", "last_probe_cursor=37"),
            ("last_probe_result_count=26", "last_probe_result_count=25"),
            ("last_probe_visible_count=3", "last_probe_visible_count=4"),
            ("audio=unavailable", "audio=0"),
            ("ranking or universal zero-cost claim", "ranking"),
            ("lanes are not multiplayer members", "lanes are multiplayer members"),
        )
        for old, new in mutations:
            with self.subTest(old=old), self.assertRaises(benchmark.ReportError):
                benchmark.parse_measurements(FIXTURE.replace(old, new), OPTIONS)
        lines = FIXTURE.splitlines()
        corruptions = ["\n".join(lines[:i] + lines[i + 1:]) + "\n" for i in range(9)]
        corruptions += [FIXTURE + line + "\n" for line in lines]
        corruptions += [FIXTURE + "unknown=1\n", FIXTURE + " " * 65536]
        for index, line in enumerate(lines[:6]):
            first_field = line.split()[0]
            changed = list(lines)
            changed[index] += " " + first_field
            corruptions.append("\n".join(changed) + "\n")
            changed[index] = line + " unknown=1"
            corruptions.append("\n".join(changed) + "\n")
            changed[index] = line.replace(first_field, "broken-field", 1)
            corruptions.append("\n".join(changed) + "\n")
        for index, text in enumerate(corruptions):
            with self.subTest(structural_case=index), self.assertRaises(benchmark.ReportError):
                benchmark.parse_measurements(text, OPTIONS)

    def test_every_integer_rejects_negative_malformed_and_overflow(self):
        # Timing observations are u128; all other summary integers are u64.
        for line in FIXTURE.splitlines()[:6]:
            for field in line.split():
                key, value = field.split("=", 1)
                if not value.isdecimal() or key == "benchmark_schema":
                    continue
                limit = 2**128 if key in ("setup_ns", "recording_ns", "seek_ns", "projection_ns", "verification_ns") else 2**64
                for bad in ("-1", "1.5", "nan", str(limit)):
                    with self.subTest(key=key, bad=bad), self.assertRaises(benchmark.ReportError):
                        benchmark.parse_measurements(FIXTURE.replace(field, f"{key}={bad}", 1), OPTIONS)

    def test_oversized_decimal_fields_fail_with_report_error(self):
        # Remain below the stdout cap while exceeding Python's default decimal
        # conversion limit; the adapter must reject these before int conversion.
        for field in ("hold_count=7", "setup_ns=1"):
            key = field.split("=", 1)[0]
            text = FIXTURE.replace(field, f"{key}=" + "9" * 5000, 1)
            with self.subTest(key=key), self.assertRaises(benchmark.ReportError):
                benchmark.parse_measurements(text, OPTIONS)

    def test_options_types_bounds_and_work_budget(self):
        for key, bad in (("notes", 0), ("notes", 100001), ("lanes", 0),
                         ("lanes", 65), ("seek_cycles", 0), ("seek_cycles", 1001),
                         ("origin_ns", -1), ("origin_ns", 604800000000001)):
            with self.subTest(key=key, bad=bad), self.assertRaises(benchmark.ReportError):
                benchmark.parse_measurements(FIXTURE, dict(OPTIONS, **{key: bad}))
        for key in OPTIONS:
            for bad in (True, "4", 4.0, None):
                with self.subTest(key=key, bad=bad), self.assertRaises(benchmark.ReportError):
                    benchmark.parse_measurements(FIXTURE, dict(OPTIONS, **{key: bad}))
        for options in (dict(OPTIONS, unknown=1), {k: v for k, v in OPTIONS.items() if k != "lanes"},
                        dict(notes=100000, lanes=64, seek_cycles=50, origin_ns=0)):
            with self.subTest(options=options), self.assertRaises(benchmark.ReportError):
                benchmark.parse_measurements(FIXTURE, options)


class CollectorTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.binary = Path(self.directory.name) / "prebuilt"
        self.binary.write_bytes(b"independent executable fixture identity")
        self.binary.chmod(0o755)
        self.calls = []

    def runner(self, binary, args, timeout):
        self.calls.append((binary, list(args), timeout))
        return dict(stdout=fixture_for(args), stderr="fixture diagnostic\n", exit_code=0,
                    wall_ns=2000000 + len(self.calls),
                    resources=dict(method="unavailable", scope="whole_child_lifetime",
                                   cpu_user_seconds=None, cpu_system_seconds=None,
                                   peak_rss_bytes=None))

    def test_default_six_observations_exact_argv_and_raw_evidence(self):
        report = benchmark.collect_report(self.binary, runner=self.runner)
        self.assertEqual(report["schema_version"], 1)
        self.assertEqual(report["workload_id"], "beatkernel-dense-chart-v1")
        self.assertEqual(benchmark.WORKLOAD, report["workload_id"])
        self.assertFalse(report["debug_assertions"])
        self.assertEqual(len(self.calls), 6)
        self.assertEqual([call[1] for call in self.calls],
                         [argv_for(FIXED[0][0])] * 3 + [argv_for(FIXED[1][0])] * 3)
        self.assertTrue(all(call[0] == self.binary.resolve() and call[2] == 30 for call in self.calls))
        self.assertEqual(report["artifact"], dict(bytes=self.binary.stat().st_size,
                         sha256=hashlib.sha256(self.binary.read_bytes()).hexdigest()))
        self.assertEqual([case["options"] for case in report["cases"]], [entry[0] for entry in FIXED])
        for case, (options, facts) in zip(report["cases"], FIXED):
            self.assertEqual(case["args"], argv_for(options))
            self.assertEqual([run["repetition"] for run in case["repetitions"]], [1, 2, 3])
            for run in case["repetitions"]:
                self.assertEqual(run["argv"], [str(self.binary.resolve()), *argv_for(options)])
                self.assertEqual(run["stdout"], literal_summary(options, facts))
                self.assertEqual(run["stderr"], "fixture diagnostic\n")
                self.assertEqual(run["exit_code"], 0)
                self.assertGreater(run["wall_ns"], 0)
                self.assertEqual(run["resources"]["scope"], "whole_child_lifetime")
                self.assertEqual(run["measurements"]["options"], options)
        self.assertIn("host", report)
        self.assertIn("git", report)
        self.assertIn("whole_child_lifetime", report["resource_scope"])
        self.assertEqual(report["resource_units"], dict(wall_ns="nanoseconds",
                         cpu_user_seconds="seconds", cpu_system_seconds="seconds", peak_rss_bytes="bytes"))
        for key in ("audio", "native_clocks_latency", "gpu_rendering", "physical_devices",
                    "whole_player_behavior", "actual_wall_clock_soak"):
            self.assertIsNone(report["unavailable"][key])

    def test_per_case_hash_consistency_and_global_assertion_mode(self):
        for old, new, call_number in (("final_engine_hash=111", "final_engine_hash=112", 2),
                                     ("last_probe_engine_hash=222", "last_probe_engine_hash=223", 2),
                                     ("debug_assertions=false", "debug_assertions=true", 4)):
            self.calls.clear()
            def runner(binary, args, timeout):
                result = self.runner(binary, args, timeout)
                if len(self.calls) == call_number:
                    result["stdout"] = result["stdout"].replace(old, new)
                return result
            with self.subTest(old=old), self.assertRaises(benchmark.ReportError):
                benchmark.collect_report(self.binary, runner=runner)

    def test_timings_may_vary_without_corrupting_repeat_identity(self):
        def runner(binary, args, timeout):
            result = self.runner(binary, args, timeout)
            result["stdout"] = result["stdout"].replace("setup_ns=1", f"setup_ns={len(self.calls)}")
            return result
        report = benchmark.collect_report(self.binary, runner=runner)
        self.assertEqual(len(report["cases"]), 2)
        self.assertNotEqual(report["cases"][0]["repetitions"][0]["stdout"],
                            report["cases"][0]["repetitions"][1]["stdout"])

    def test_changed_artifact_and_partial_failure_abort_without_publication(self):
        for mode in ("mutation", "nonzero", "malformed", "exception"):
            self.calls.clear()
            self.binary.write_bytes(b"independent executable fixture identity")
            target = Path(self.directory.name) / "report.json"
            def runner(binary, args, timeout):
                result = self.runner(binary, args, timeout)
                if len(self.calls) == 4:
                    if mode == "mutation":
                        self.binary.write_bytes(b"changed artifact")
                    elif mode == "nonzero":
                        result["exit_code"] = 7
                    elif mode == "malformed":
                        result["stdout"] = "partial stdout\n"
                    else:
                        raise benchmark.ReportError("original child failure")
                return result
            with self.subTest(mode=mode), mock.patch.object(benchmark, "run_process", runner), contextlib.redirect_stderr(io.StringIO()):
                self.assertNotEqual(benchmark.main(["--binary", str(self.binary), "--output", str(target)]), 0)
            self.assertFalse(target.exists())
            self.assertLessEqual(len(self.calls), 6)

    def test_collector_rejects_invalid_settings_before_launch(self):
        for settings in (dict(repeats=0), dict(repeats=11), dict(repeats=True),
                         dict(repeats=1.0), dict(timeout_seconds=0), dict(timeout_seconds=241),
                         dict(timeout_seconds=float("nan")), dict(timeout_seconds=float("inf")),
                         dict(timeout_seconds=True), dict(timeout_seconds="30")):
            with self.subTest(settings=settings), self.assertRaises(benchmark.ReportError):
                benchmark.collect_report(self.binary, runner=self.runner, **settings)
        self.assertEqual(self.calls, [])

    def test_existing_output_and_dangling_symlink_prevent_launch(self):
        target = Path(self.directory.name) / "report.json"
        for symlink in (False, True):
            if symlink:
                target.unlink()
                target.symlink_to(Path(self.directory.name) / "absent")
            else:
                target.write_bytes(b"preserve original")
            with self.subTest(symlink=symlink), mock.patch.object(benchmark, "collect_report") as collector, contextlib.redirect_stderr(io.StringIO()):
                self.assertNotEqual(benchmark.main(["--binary", str(self.binary), "--output", str(target)]), 0)
            collector.assert_not_called()
            self.assertEqual(target.is_symlink(), symlink)
            if not symlink:
                self.assertEqual(target.read_bytes(), b"preserve original")

    def test_atomic_collision_preserves_concurrent_owner_and_cleans_temp(self):
        target = Path(self.directory.name) / "report.json"
        report = benchmark.collect_report(self.binary, repeats=1, runner=self.runner)
        def collision(source, destination, *args, **kwargs):
            Path(destination).write_bytes(b"concurrent owner")
            raise FileExistsError("output race")
        with mock.patch.object(os, "link", collision), self.assertRaises(benchmark.ReportError):
            benchmark.publish_report(report, target)
        self.assertEqual(target.read_bytes(), b"concurrent owner")
        self.assertEqual(sorted(path.name for path in target.parent.iterdir()), ["prebuilt", "report.json"])

    def test_stdout_is_complete_json(self):
        output = io.StringIO()
        with mock.patch.object(benchmark, "run_process", self.runner), contextlib.redirect_stdout(output):
            code = benchmark.main(["--binary", str(self.binary), "--repeats", "1", "--timeout", "2"])
        self.assertEqual(code, 0)
        self.assertEqual(len(json.loads(output.getvalue())["cases"]), 2)
        self.assertEqual([call[2] for call in self.calls], [2, 2])


class CliTests(unittest.TestCase):
    def test_help_without_binary_and_invalid_options(self):
        output = io.StringIO()
        with contextlib.redirect_stdout(output), self.assertRaises(SystemExit) as caught:
            benchmark.main(["--help"])
        self.assertEqual(caught.exception.code, 0)
        for flag in ("--binary", "--output", "--repeats", "--timeout"):
            self.assertIn(flag, output.getvalue())
        for args in ([], ["--binary", "/absent"], ["--binary", sys.executable, "--repeats", "0"],
                     ["--binary", sys.executable, "--repeats", "11"],
                     ["--binary", sys.executable, "--timeout", "nan"],
                     ["--binary", sys.executable, "--timeout", "241"],
                     ["--binary", sys.executable, "--notes", "2"]):
            error = io.StringIO()
            with self.subTest(args=args), mock.patch.object(benchmark, "run_process") as runner, contextlib.redirect_stderr(error):
                try:
                    code = benchmark.main(args)
                except SystemExit as exception:
                    code = exception.code
            self.assertNotEqual(code, 0)
            self.assertTrue(error.getvalue().strip())
            runner.assert_not_called()

    @unittest.skipUnless(os.name == "posix", "executable script fixture requires POSIX")
    def test_real_fake_child_and_cli_outside_repository_cwd(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / "fake dense child"
            marker = root / "argv.jsonl"
            output = root / "report.json"
            summaries = {str(options["notes"]): literal_summary(options, facts) for options, facts in FIXED}
            program = (f"#!{sys.executable}\n"
                       "import json,pathlib,sys\n"
                       f"marker=pathlib.Path({str(marker)!r})\n"
                       "with marker.open('a') as handle: handle.write(json.dumps(sys.argv[1:])+'\\n')\n"
                       f"summaries={summaries!r}\n"
                       "print(summaries[sys.argv[2]],end='')\n"
                       "print('actual fixture stderr',file=sys.stderr)\n")
            binary.write_text(program)
            binary.chmod(0o755)
            command = [sys.executable, str(SOURCE), "--binary", str(binary), "--output", str(output), "--repeats", "1"]
            result = subprocess.run(command, cwd=root, capture_output=True, text=True, timeout=20)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout, "")
            report = json.loads(output.read_text())
            self.assertEqual([json.loads(line) for line in marker.read_text().splitlines()],
                             [argv_for(options) for options, _ in FIXED])
            self.assertEqual(len(report["cases"]), 2)
            for case in report["cases"]:
                run = case["repetitions"][0]
                self.assertEqual(run["stderr"], "actual fixture stderr\n")
                self.assertEqual(run["resources"]["scope"], "whole_child_lifetime")
                self.assertGreaterEqual(run["wall_ns"], 0)
            before = output.read_bytes()
            launched = marker.read_bytes()
            result = subprocess.run(command, cwd=root, capture_output=True, text=True, timeout=20)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(output.read_bytes(), before)
            self.assertEqual(marker.read_bytes(), launched)

    @unittest.skipUnless(os.name == "posix", "executable script fixture requires POSIX")
    def test_actual_child_nonzero_refuses_publication(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / "failing child"
            output = root / "report.json"
            binary.write_text(f"#!{sys.executable}\nimport sys\nprint('original dense failure', file=sys.stderr)\nsys.exit(7)\n")
            binary.chmod(0o755)
            result = subprocess.run([sys.executable, str(SOURCE), "--binary", str(binary),
                                     "--output", str(output)], cwd=root,
                                    capture_output=True, text=True, timeout=20)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("original dense failure", result.stderr)
            self.assertFalse(output.exists())
            self.assertEqual(result.stdout, "")


if __name__ == "__main__":
    unittest.main()
