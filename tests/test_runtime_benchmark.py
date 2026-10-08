"""Independent evidence-contract tests; no performance threshold assertions."""
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import signal
import shutil
import subprocess
import sys
import tempfile
import time
from types import SimpleNamespace
import unittest
from unittest import mock


SOURCE = Path(__file__).resolve().parents[1] / "tools/runtime_benchmark.py"
SPEC = importlib.util.spec_from_file_location("runtime_benchmark", SOURCE)
benchmark = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = benchmark
SPEC.loader.exec_module(benchmark)

OPTIONS = dict(iterations=5, warmup=1, buffer_frames=32, queue_capacity=64,
               telemetry_samples=4096, sample_rate=48000)
# This literal describes measured indices 1..5: 16,24,32,32,16 frames.
# There are ten down/up calls and one repeat at index 4, no unbound calls.
FIXTURE = """benchmark_schema=1 workload_id=beatkernel-runtime-cpu-v1 debug_assertions=false
software-only CPU workload; settings=Options { iterations: 5, warmup: 1, buffer_frames: 32, queue_capacity: 64, telemetry_samples: 4096, sample_rate: 48000 }; elapsed_ns=1000000
measured_blocks=5 software_operations=11 rendered_frames=120 pcm_checksum=-0.25
software_operations_per_second=11000.00
runtime_input_call_execution_ns=Some(TimingSummary { samples: 11, p50_ns: 100, p95_ns: 200, p99_ns: 300, max_ns: 400 }) capacity=4096 (measured process_input calls only)
mixer_render_execution_ns=Some(TimingSummary { samples: 5, p50_ns: 100, p95_ns: 200, p99_ns: 300, max_ns: 400 }) capacity=4096 (offline varying buffer sizes)
synthetic_generated_interval_jitter_ns=Some(IntervalJitterSummary { samples: 5, min_deviation_ns: -20000, max_deviation_ns: 10000, p50_abs_deviation_ns: 5000, p95_abs_deviation_ns: 20000, p99_abs_deviation_ns: 20000, max_abs_deviation_ns: 20000 }) nominal_ns=1000000 clock_domain=90 pairs=5 (generated timestamps; not measured callback/device timing)
software_runtime_counters=RuntimeCounters { inputs: 11, unbound: 0, rejected: 0, judge_results: 5, audio_commands: 5, queue_full: 0, queue_disconnected: 0, input_drops: 0, audio_underruns: 0 }
software_mixer_counters=AudioCounters { rendered_frames: 120, commands_consumed: 5, commands_applied: 5, late_commands: 0, pending_full: 0, voice_full: 0, unknown_samples: 0, unknown_stops: 0, invalid_gains: 0, invalid_rates: 0, invalid_times: 0 }
native_input_latency=unavailable physical_output_latency=unavailable native_callback_arrival_jitter=unavailable actual_native_underruns=unavailable
"""


def fixture_for(args):
    options = dict(zip((s[2:].replace("-", "_") for s in args[::2]),
                       map(int, args[1::2])))
    text = FIXTURE
    for name in ("buffer_frames", "sample_rate"):
        text = text.replace(f"{name}: {OPTIONS[name]}", f"{name}: {options[name]}")
    # For five blocks starting at index one, independently observed 15B/4.
    text = text.replace("rendered_frames=120", f"rendered_frames={options['buffer_frames'] * 15 // 4}")
    text = text.replace("rendered_frames: 120", f"rendered_frames: {options['buffer_frames'] * 15 // 4}")
    return text


class ParserTests(unittest.TestCase):
    def test_literal_warmup_oracle_and_units(self):
        result = benchmark.parse_measurements(FIXTURE, OPTIONS)
        self.assertEqual(benchmark.expected_frames(OPTIONS), 120)
        self.assertEqual(benchmark.expected_operations(OPTIONS), 11)
        self.assertEqual(result["options"], OPTIONS)
        self.assertEqual(result["rendered_frames"], 120)
        self.assertEqual(result["software_operations"], 11)
        self.assertFalse(result["debug_assertions"])
        self.assertEqual(result["pcm_checksum"], -0.25)
        self.assertEqual(result["software_operations_per_second"], 11000.0)
        self.assertEqual(result["runtime_input_call_execution_ns"]["samples"], 11)
        self.assertEqual(result["synthetic_generated_interval_jitter_ns"]["min_deviation_ns"], -20000)

    def test_independent_offset_oracles(self):
        for warmup, frames, operations in ((0, 136, 13), (1, 120, 11),
                                           (2, 128, 11), (3, 136, 12),
                                           (4, 136, 13), (7, 136, 12)):
            with self.subTest(warmup=warmup):
                options = dict(OPTIONS, warmup=warmup)
                self.assertEqual(benchmark.expected_frames(options), frames)
                self.assertEqual(benchmark.expected_operations(options), operations)

    def test_zero_elapsed_without_throughput_is_unavailable(self):
        text = FIXTURE.replace("elapsed_ns=1000000", "elapsed_ns=0")
        text = text.replace("software_operations_per_second=11000.00\n", "")
        self.assertIsNone(benchmark.parse_measurements(text, OPTIONS)["software_operations_per_second"])

    def test_reject_contract_corruption(self):
        mutations = {
            "schema": ("benchmark_schema=1", "benchmark_schema=2"),
            "workload": ("beatkernel-runtime-cpu-v1", "beatkernel-runtime-cpu-v2"),
            "assertion": ("debug_assertions=false", "debug_assertions=maybe"),
            "duplicate banner": ("benchmark_schema=1", "benchmark_schema=1 benchmark_schema=1"),
            "unknown banner": ("benchmark_schema=1", "benchmark_schema=1 surprise=1"),
            "options": ("warmup: 1", "warmup: 0"),
            "duplicate option": ("warmup: 1", "warmup: 1, warmup: 1"),
            "unknown option": ("warmup: 1", "warmup: 1, magic: 7"),
            "missing option": ("warmup: 1, ", ""),
            "blocks": ("measured_blocks=5", "measured_blocks=4"),
            "duplicate summary": ("measured_blocks=5", "measured_blocks=5 measured_blocks=5"),
            "unknown summary": ("measured_blocks=5", "measured_blocks=5 surprise=0"),
            "operations": ("software_operations=11", "software_operations=12"),
            "constant frames": ("rendered_frames=120", "rendered_frames=160"),
            "checksum nan": ("pcm_checksum=-0.25", "pcm_checksum=NaN"),
            "checksum inf": ("pcm_checksum=-0.25", "pcm_checksum=inf"),
            "throughput nan": ("software_operations_per_second=11000.00", "software_operations_per_second=nan"),
            "negative elapsed": ("elapsed_ns=1000000", "elapsed_ns=-1"),
            "overflow elapsed": ("elapsed_ns=1000000", "elapsed_ns=" + "9" * 100),
            "wrong samples": ("samples: 11", "samples: 10"),
            "unordered timing": ("p95_ns: 200", "p95_ns: 99"),
            "negative timing": ("p50_ns: 100", "p50_ns: -1"),
            "duplicate timing": ("p50_ns: 100", "p50_ns: 100, p50_ns: 100"),
            "unknown timing": ("p50_ns: 100", "p50_ns: 100, average_ns: 0"),
            "missing timing": ("p50_ns: 100, ", ""),
            "capacity": ("capacity=4096", "capacity=4095"),
            "counter mismatch": ("inputs: 11", "inputs: 10"),
            "negative counter": ("queue_full: 0", "queue_full: -1"),
            "duplicate counter": ("queue_full: 0", "queue_full: 0, queue_full: 0"),
            "unknown counter": ("queue_full: 0", "queue_full: 0, surprise: 0"),
            "missing counter": ("queue_full: 0, ", ""),
            "synthetic samples": ("samples: 5, min_deviation", "samples: 4, min_deviation"),
            "synthetic order": ("p50_abs_deviation_ns: 5000", "p50_abs_deviation_ns: 30000"),
            "pairs": ("pairs=5", "pairs=4"),
        }
        for label, (old, new) in mutations.items():
            with self.subTest(label=label), self.assertRaises(benchmark.ReportError):
                benchmark.parse_measurements(FIXTURE.replace(old, new), OPTIONS)
        for text in (FIXTURE + FIXTURE.splitlines()[0] + "\n",
                     FIXTURE.replace("software_operations_per_second=11000.00\n", ""),
                     FIXTURE.replace(FIXTURE.splitlines()[0] + "\n", ""),
                     FIXTURE + " " * 65536):
            with self.subTest(text=text[:40]), self.assertRaises(benchmark.ReportError):
                benchmark.parse_measurements(text, OPTIONS)

    def test_retained_capacity_clamps_samples(self):
        options = dict(OPTIONS, telemetry_samples=3)
        text = FIXTURE.replace("4096", "3").replace("samples: 11", "samples: 3").replace("samples: 5", "samples: 3")
        self.assertEqual(benchmark.parse_measurements(text, options)["runtime_input_call_execution_ns"]["samples"], 3)


class ProcessTests(unittest.TestCase):
    def test_own_stdout_stderr_and_original_exit(self):
        result = benchmark.run_process(sys.executable,
            ["-c", "import sys; print('out'); print('err', file=sys.stderr)"], 3)
        self.assertEqual(result["stdout"], "out\n")
        self.assertEqual(result["stderr"], "err\n")
        self.assertEqual(result["exit_code"], 0)
        self.assertGreater(result["wall_ns"], 0)
        self.assertEqual(result["resources"]["scope"], "whole_child_lifetime")
        with self.assertRaises(benchmark.ReportError) as caught:
            benchmark.run_process(sys.executable,
                ["-c", "import sys; print('original failure', file=sys.stderr); sys.exit(7)"], 3)
        self.assertIn("7", str(caught.exception))
        self.assertIn("original failure", str(caught.exception))

    def test_argv_is_literal(self):
        argument = "$(false); `false` && echo injected"
        result = benchmark.run_process(sys.executable, ["-c", "import sys; print(sys.argv[1])", argument], 3)
        self.assertEqual(result["stdout"], argument + "\n")

    def test_timeout_reaps_owned_child(self):
        with tempfile.TemporaryDirectory() as directory:
            pid_file = Path(directory) / "pid"
            program = "import os,pathlib,time; pathlib.Path(__import__('sys').argv[1]).write_text(str(os.getpid())); time.sleep(60)"
            with self.assertRaises(benchmark.ReportError):
                benchmark.run_process(sys.executable, ["-c", program, str(pid_file)], 0.5)
            self.assertTrue(pid_file.exists(), "fixture must actually start before timeout")
            if os.name == "posix":
                with self.assertRaises(ProcessLookupError):
                    os.kill(int(pid_file.read_text()), 0)

    @unittest.skipUnless(os.name == "posix", "owned process groups require POSIX")
    def test_post_reap_cleanup_stops_same_group_descendant(self):
        leaf_program = (
            "import json,os,pathlib,sys,time; "
            "marker=pathlib.Path(sys.argv[1]); temporary=marker.with_suffix('.tmp'); "
            "temporary.write_text(json.dumps([os.getpid(),os.getpgrp()])); temporary.replace(marker); "
            "time.sleep(60)"
        )
        parent_program = """
import os, pathlib, subprocess, sys, time
marker = pathlib.Path(sys.argv[1])
leaf = subprocess.Popen([sys.executable, '-c', sys.argv[2], str(marker)])
deadline = time.monotonic() + 2
while not marker.exists() and time.monotonic() < deadline:
    time.sleep(0.01)
if not marker.exists():
    leaf.kill()
    leaf.wait()
    raise RuntimeError('descendant did not start')
if sys.argv[3] == 'nonzero':
    sys.exit(7)
if sys.argv[3] == 'success':
    print('done')
    sys.exit(0)
os.write(1, b'\\xff')
"""
        def still_running(pid):
            try:
                os.kill(pid, 0)
            except ProcessLookupError:
                return False
            # A Linux orphan can remain in /proc until init reaps it. A zombie
            # has stopped executing and is not a surviving sleeping descendant.
            if sys.platform.startswith("linux"):
                try:
                    state = (Path("/proc") / str(pid) / "stat").read_text().rsplit(")", 1)[1].split()[0]
                except FileNotFoundError:
                    return False
                if state == "Z":
                    return False
            return True

        for failure in ("nonzero", "malformed_utf8", "success"):
            with self.subTest(failure=failure), tempfile.TemporaryDirectory() as directory:
                marker = Path(directory) / "owned-leaf.json"
                owned_group = None
                leaf_pid = None
                try:
                    arguments = ["-c", parent_program, str(marker), leaf_program, failure]
                    if failure == "success":
                        result = benchmark.run_process(sys.executable, arguments, 5)
                        self.assertEqual(result["exit_code"], 0)
                        self.assertEqual(result["stdout"], "done\n")
                    else:
                        with self.assertRaises(benchmark.ReportError):
                            benchmark.run_process(sys.executable, arguments, 5)
                    self.assertTrue(marker.exists(), "descendant must actually start")
                    leaf_pid, owned_group = json.loads(marker.read_text())
                    self.assertNotEqual(owned_group, os.getpgrp(), "fixture must own a separate group")
                    deadline = time.monotonic() + 2
                    while still_running(leaf_pid) and time.monotonic() < deadline:
                        time.sleep(0.01)
                    self.assertFalse(still_running(leaf_pid),
                                     "cleanup after leader reaping must terminate its descendant")
                finally:
                    # Preserve test isolation even against the unfixed runner.
                    # Never target any process group except this fixture's group.
                    if marker.exists() and owned_group is None:
                        leaf_pid, owned_group = json.loads(marker.read_text())
                    if owned_group is not None and owned_group != os.getpgrp() and still_running(leaf_pid):
                        try:
                            os.killpg(owned_group, signal.SIGKILL)
                        except ProcessLookupError:
                            pass

    def test_output_limits_and_utf8(self):
        for program in ("import os; os.write(1, b'\\xff')",
                        "print('x' * 70000)",
                        "import sys; sys.stderr.write('x' * 70000)"):
            with self.subTest(program=program), self.assertRaises(benchmark.ReportError):
                benchmark.run_process(sys.executable, ["-c", program], 3)

    def test_resource_fallback_is_explicit(self):
        with mock.patch.object(benchmark.os, "wait4", None, create=True):
            result = benchmark.run_process(sys.executable, ["-c", "pass"], 3)
        resources = result["resources"]
        self.assertEqual(resources["method"], "unavailable")
        for key in ("cpu_user_seconds", "cpu_system_seconds", "peak_rss_bytes"):
            self.assertIsNone(resources[key])

    @unittest.skipUnless(os.name == "posix" and callable(getattr(os, "wait4", None)), "wait4 unavailable")
    def test_per_child_resource_adapter_units(self):
        real_wait4 = os.wait4
        observed = []
        def attributed_wait4(pid, options):
            child, status, usage = real_wait4(pid, options)
            if child:
                observed.append(child)
            # Adapter fixture replaces only this child's resource observations;
            # status and reaping still belong to the actual owned subprocess.
            return child, status, SimpleNamespace(ru_utime=1.25, ru_stime=0.5, ru_maxrss=17)
        for platform, expected_rss in (("linux", 17 * 1024), ("darwin", 17), ("unknown", None)):
            observed.clear()
            with self.subTest(platform=platform), mock.patch.object(benchmark.os, "wait4", attributed_wait4), mock.patch.object(benchmark.sys, "platform", platform):
                result = benchmark.run_process(sys.executable, ["-c", "pass"], 3)
            self.assertEqual(len(observed), 1, "one owner must reap exactly this child")
            resources = result["resources"]
            self.assertEqual(resources["method"], "wait4")
            self.assertEqual(resources["cpu_user_seconds"], 1.25)
            self.assertEqual(resources["cpu_system_seconds"], 0.5)
            self.assertEqual(resources["peak_rss_bytes"], expected_rss)


class CollectorAndPublicationTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.binary = Path(self.directory.name) / "prebuilt"
        self.binary.write_bytes(b"explicit prebuilt fixture identity")
        self.binary.chmod(0o755)
        self.calls = []

    def runner(self, binary, args, timeout):
        self.calls.append((binary, list(args), timeout))
        return dict(stdout=fixture_for(args), stderr="fixture diagnostic\n", exit_code=0,
                    wall_ns=2000000 + len(self.calls), resources=dict(method="unavailable",
                    scope="whole_child_lifetime", cpu_user_seconds=None,
                    cpu_system_seconds=None, peak_rss_bytes=None))

    def collect(self, runner=None):
        return benchmark.collect_report(self.binary, iterations=5, warmup=1,
            repeats=2, timeout_seconds=3, runner=runner or self.runner)

    def test_complete_fixed_matrix_raw_evidence_and_identity(self):
        report = self.collect()
        self.assertEqual(report["schema_version"], 1)
        self.assertEqual(report["workload_id"], "beatkernel-runtime-cpu-v1")
        self.assertFalse(report["debug_assertions"])
        self.assertEqual(report["artifact"]["bytes"], self.binary.stat().st_size)
        import hashlib
        self.assertEqual(report["artifact"]["sha256"], hashlib.sha256(self.binary.read_bytes()).hexdigest())
        self.assertEqual(len(self.calls), 12)
        self.assertEqual([(c["options"]["sample_rate"], c["options"]["buffer_frames"])
                          for c in report["cases"]],
                         [(44100,32),(44100,128),(44100,512),(48000,32),(48000,128),(48000,512)])
        for case in report["cases"]:
            self.assertEqual(case["options"]["queue_capacity"], 64)
            self.assertEqual(case["options"]["telemetry_samples"], 4096)
            self.assertEqual([r["repetition"] for r in case["repetitions"]], [1,2])
            for run in case["repetitions"]:
                self.assertEqual(run["stderr"], "fixture diagnostic\n")
                self.assertEqual(run["stdout"], fixture_for(case["args"]))
                self.assertEqual(run["measurements"]["options"], case["options"])
        self.assertIn("host", report)
        self.assertIn("git", report)
        self.assertIn("unavailable", report)

    def test_deterministic_mismatch_refuses_whole_report(self):
        for replacement in (("pcm_checksum=-0.25", "pcm_checksum=-0.5"),
                            ("judge_results: 5", "judge_results: 4"),
                            ("debug_assertions=false", "debug_assertions=true")):
            self.calls.clear()
            def runner(binary, args, timeout):
                result = self.runner(binary, args, timeout)
                if len(self.calls) == 2:
                    result["stdout"] = result["stdout"].replace(*replacement)
                return result
            with self.subTest(replacement=replacement), self.assertRaises(benchmark.ReportError):
                self.collect(runner)

    def test_artifact_mutation_refuses(self):
        def runner(binary, args, timeout):
            result = self.runner(binary, args, timeout)
            if len(self.calls) == 12:
                self.binary.write_bytes(b"changed")
            return result
        with self.assertRaises(benchmark.ReportError):
            self.collect(runner)

    def test_timing_variation_does_not_change_deterministic_agreement(self):
        def runner(binary, args, timeout):
            result = self.runner(binary, args, timeout)
            if len(self.calls) % 2 == 0:
                result["stdout"] = result["stdout"].replace("elapsed_ns=1000000", "elapsed_ns=2000000").replace("software_operations_per_second=11000.00", "software_operations_per_second=5500.00").replace("p50_ns: 100", "p50_ns: 101")
            return result
        report = self.collect(runner)
        self.assertEqual(len(report["cases"]), 6)
        first, second = report["cases"][0]["repetitions"]
        self.assertNotEqual(first["measurements"]["elapsed_ns"], second["measurements"]["elapsed_ns"])
        self.assertEqual(first["measurements"]["pcm_checksum"], second["measurements"]["pcm_checksum"])

    def test_process_failure_stops_without_publishing(self):
        target = Path(self.directory.name) / "report.json"
        def runner(binary, args, timeout):
            raise benchmark.ReportError("failed original child")
        with mock.patch.object(benchmark, "run_process", runner), contextlib.redirect_stderr(io.StringIO()):
            code = benchmark.main(["--binary", str(self.binary), "--output", str(target), "--iterations", "5", "--warmup", "1"])
        self.assertNotEqual(code, 0)
        self.assertFalse(target.exists())

    def test_existing_and_dangling_symlink_preflight_prevents_launch(self):
        target = Path(self.directory.name) / "report.json"
        for symlink in (False, True):
            if symlink:
                target.unlink()
                target.symlink_to(Path(self.directory.name) / "missing")
            else:
                target.write_bytes(b"preserve")
            with mock.patch.object(benchmark, "collect_report") as collector, contextlib.redirect_stderr(io.StringIO()):
                code = benchmark.main(["--binary", str(self.binary), "--output", str(target)])
            self.assertNotEqual(code, 0)
            collector.assert_not_called()
            if symlink:
                self.assertTrue(target.is_symlink())
            else:
                self.assertEqual(target.read_bytes(), b"preserve")

    def test_atomic_publication_and_collision_cleanup(self):
        report = self.collect()
        target = Path(self.directory.name) / "report.json"
        benchmark.publish_report(report, target)
        self.assertEqual(json.loads(target.read_text()), report)
        before = sorted(p.name for p in target.parent.iterdir())
        with self.assertRaises(benchmark.ReportError):
            benchmark.publish_report(dict(report, schema_version=99), target)
        self.assertEqual(json.loads(target.read_text()), report)
        self.assertEqual(sorted(p.name for p in target.parent.iterdir()), before)

    def test_failed_atomic_link_leaves_no_temporary_or_replacement(self):
        target = Path(self.directory.name) / "new.json"
        existing = Path(self.directory.name) / "existing.json"
        existing.write_bytes(b"keep")
        before = sorted(p.name for p in target.parent.iterdir())
        with mock.patch.object(benchmark.os, "link", side_effect=OSError("unsupported")):
            with self.assertRaises(benchmark.ReportError):
                benchmark.publish_report(self.collect(), target)
        self.assertEqual(existing.read_bytes(), b"keep")
        self.assertFalse(target.exists())
        self.assertEqual(sorted(p.name for p in target.parent.iterdir()), before)

    def test_atomic_collision_preserves_new_existing_entry(self):
        target = Path(self.directory.name) / "raced.json"
        def collision(source, destination, *args, **kwargs):
            Path(destination).write_bytes(b"concurrent owner")
            raise FileExistsError("concurrent output")
        with mock.patch.object(benchmark.os, "link", collision):
            with self.assertRaises(benchmark.ReportError):
                benchmark.publish_report(self.collect(), target)
        self.assertEqual(target.read_bytes(), b"concurrent owner")
        self.assertEqual(sorted(p.name for p in target.parent.iterdir()), ["prebuilt", "raced.json"])


class GitMetadataTests(unittest.TestCase):
    @unittest.skipUnless(shutil.which("git"), "Git unavailable")
    def test_clean_metadata_does_not_refresh_index(self):
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            tools = repository / "tools"
            tools.mkdir()
            tracked = repository / "tracked.txt"
            tracked.write_text("unchanged fixture content\n")
            environment = dict(os.environ)
            # Keep the fixture independent of any surrounding repository or
            # personal configuration; all Git mutations belong to this tempdir.
            for key in tuple(environment):
                if key.startswith("GIT_"):
                    del environment[key]
            environment.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull)
            def git(*args):
                return subprocess.run([shutil.which("git"), *args], cwd=repository,
                    env=environment, check=True, capture_output=True, text=True,
                    timeout=5).stdout.strip()
            git("init", "--quiet")
            git("add", "tracked.txt")
            git("-c", "user.name=Fixture", "-c", "user.email=fixture@invalid",
                "commit", "--quiet", "--no-gpg-sign", "-m", "fixture")
            revision = git("rev-parse", "HEAD")
            index = repository / ".git" / "index"
            original_bytes = index.read_bytes()
            original_mtime = index.stat().st_mtime_ns
            original_stat = tracked.stat()
            os.utime(tracked, ns=(original_stat.st_atime_ns,
                                 original_stat.st_mtime_ns + 1_000_000_000))
            with mock.patch.object(benchmark, "__file__", str(tools / "runtime_benchmark.py")), mock.patch.dict(os.environ, environment, clear=True):
                metadata = benchmark._git_metadata()
            self.assertEqual(metadata["revision"], revision)
            self.assertIs(metadata["tracked_dirty"], False)
            self.assertEqual(index.read_bytes(), original_bytes)
            self.assertEqual(index.stat().st_mtime_ns, original_mtime)
            self.assertFalse((repository / ".git" / "index.lock").exists())


class CliTests(unittest.TestCase):
    def test_help_without_binary(self):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            with self.assertRaises(SystemExit) as caught:
                benchmark.main(["--help"])
        self.assertEqual(caught.exception.code, 0)
        for option in ("--binary", "--output", "--iterations", "--warmup", "--repeats", "--timeout"):
            self.assertIn(option, output.getvalue())

    def test_invalid_cli_is_actionable_before_child_execution(self):
        for args in ([], ["--binary", "/does/not/exist"],
                     ["--binary", sys.executable, "--iterations", "0"],
                     ["--binary", sys.executable, "--iterations", "10001"],
                     ["--binary", sys.executable, "--warmup", "1001"],
                     ["--binary", sys.executable, "--repeats", "11"],
                     ["--binary", sys.executable, "--timeout", "nan"],
                     ["--binary", sys.executable, "--timeout", "241"],
                     ["--binary", sys.executable, "--buffer-frames", "32"]):
            error = io.StringIO()
            with self.subTest(args=args), mock.patch.object(benchmark, "run_process") as child_runner, contextlib.redirect_stderr(error):
                try:
                    code = benchmark.main(args)
                except SystemExit as exception:
                    code = exception.code
            self.assertNotEqual(code, 0)
            self.assertTrue(error.getvalue().strip())
            child_runner.assert_not_called()


if __name__ == "__main__":
    unittest.main()
