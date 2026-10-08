#!/usr/bin/env python3
"""Validated software-only evidence from an explicitly selected prebuilt binary."""
import argparse
import hashlib
import json
import math
import os
import pathlib
import platform
import re
import signal
import subprocess
import sys
import tempfile
import time

MAX_OUTPUT = 64 * 1024
WORKLOAD = "beatkernel-runtime-cpu-v1"
OPTION_LIMITS = {
    "iterations": (1, 10000), "warmup": (0, 1000),
    "buffer_frames": (8, 4096), "queue_capacity": (1, 65536),
    "telemetry_samples": (1, 65536), "sample_rate": (8000, 192000),
}
TIMING_FIELDS = ("samples", "p50_ns", "p95_ns", "p99_ns", "max_ns")
JITTER_FIELDS = ("samples", "min_deviation_ns", "max_deviation_ns",
                 "p50_abs_deviation_ns", "p95_abs_deviation_ns",
                 "p99_abs_deviation_ns", "max_abs_deviation_ns")
RUNTIME_FIELDS = ("inputs", "unbound", "rejected", "judge_results", "audio_commands",
                  "queue_full", "queue_disconnected", "input_drops", "audio_underruns")
MIXER_FIELDS = ("rendered_frames", "commands_consumed", "commands_applied", "late_commands",
                "pending_full", "voice_full", "unknown_samples", "unknown_stops",
                "invalid_gains", "invalid_rates", "invalid_times")
UNAVAILABLE = ("native_input_latency", "physical_output_latency",
               "native_callback_arrival_jitter", "actual_native_underruns")
FLOAT = r"[+-]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?"


class ReportError(ValueError):
    """A run cannot supply a complete trustworthy report."""


def _options(options):
    if set(options) != set(OPTION_LIMITS):
        raise ReportError("options must contain exactly the known Rust fields")
    for name, (low, high) in OPTION_LIMITS.items():
        value = options[name]
        if type(value) is not int or not low <= value <= high:
            raise ReportError(f"invalid {name}: expected {low}..{high}")
    return dict(options)


def expected_frames(options_dict):
    o = _options(options_dict)
    b = o["buffer_frames"]
    pattern = (b, b // 2, 3 * b // 4, b)
    return sum(pattern[i % 4] for i in range(o["warmup"], o["warmup"] + o["iterations"]))


def expected_operations(options_dict):
    o = _options(options_dict)
    return sum(2 + (i % 4 == 0) + (i % 7 == 0)
               for i in range(o["warmup"], o["warmup"] + o["iterations"]))


def _match(pattern, text):
    match = re.fullmatch(pattern, text)
    if not match:
        raise ReportError("malformed or unsupported benchmark output")
    return match


def _integer(value, signed=False):
    _match(r"-?[0-9]+" if signed else r"[0-9]+", value)
    # Length check avoids Python's large decimal conversion boundary.
    if len(value) > 21:
        raise ReportError("integer observation outside bounds")
    number = int(value)
    if not (-(2**63) if signed else 0) <= number <= 2**64 - 1:
        raise ReportError("integer observation outside bounds")
    return number


def _fields(text, names, signed=()):
    result = {}
    for field in text.split(", "):
        match = _match(r"([a-z0-9_]+): (.+)", field)
        name, value = match.groups()
        if name not in names or name in result:
            raise ReportError("unknown or duplicate structure field")
        result[name] = _integer(value, name in signed)
    if set(result) != set(names):
        raise ReportError("missing structure field")
    return result


def _finite(value):
    _match(FLOAT, value)
    result = float(value)
    if not math.isfinite(result):
        raise ReportError("nonfinite observation")
    return result


def parse_measurements(stdout, expected_options_dict):
    options = _options(expected_options_dict)
    if not isinstance(stdout, str):
        raise ReportError("stdout must be UTF-8 text")
    try:
        size = len(stdout.encode("utf-8"))
    except UnicodeError as exc:
        raise ReportError("invalid UTF-8 output") from exc
    if size > MAX_OUTPUT:
        raise ReportError("benchmark output exceeds 64KiB")
    lines = stdout.splitlines()
    if len(lines) not in (9, 10):
        raise ReportError("missing, duplicate or unknown benchmark lines")
    banner = _match(r"benchmark_schema=1 workload_id=" + WORKLOAD +
                    r" debug_assertions=(true|false)", lines[0])
    result = {"benchmark_schema": 1, "workload_id": WORKLOAD,
              "debug_assertions": banner[1] == "true", "options": options}
    settings = _match(r"software-only CPU workload; settings=Options \{ (.+) \}; elapsed_ns=([0-9]+)", lines[1])
    if _fields(settings[1], OPTION_LIMITS) != options:
        raise ReportError("reported settings differ from requested options")
    result["elapsed_ns"] = _integer(settings[2])
    counts = _match(r"measured_blocks=([0-9]+) software_operations=([0-9]+) rendered_frames=([0-9]+) pcm_checksum=(" + FLOAT + r")", lines[2])
    for name, value in zip(("measured_blocks", "software_operations", "rendered_frames"), counts.groups()[:3]):
        result[name] = _integer(value)
    result["pcm_checksum"] = _finite(counts[4])
    if (result["measured_blocks"] != options["iterations"] or
            result["software_operations"] != expected_operations(options) or
            result["rendered_frames"] != expected_frames(options)):
        raise ReportError("measured block/operation/frame counts differ from workload")
    offset = 3
    result["software_operations_per_second"] = None
    if result["elapsed_ns"]:
        throughput = _match(r"software_operations_per_second=(" + FLOAT + r")", lines[offset])
        result["software_operations_per_second"] = _finite(throughput[1])
        if result["software_operations_per_second"] < 0:
            raise ReportError("negative throughput")
        offset += 1
    for name, suffix, count in (
            ("runtime_input_call_execution_ns", "measured process_input calls only", result["software_operations"]),
            ("mixer_render_execution_ns", "offline varying buffer sizes", options["iterations"])):
        match = _match(re.escape(name) + r"=Some\(TimingSummary \{ (.+) \}\) capacity=([0-9]+) \(" + re.escape(suffix) + r"\)", lines[offset])
        timing = _fields(match[1], TIMING_FIELDS)
        if (_integer(match[2]) != options["telemetry_samples"] or
                timing["samples"] != min(count, options["telemetry_samples"]) or
                not timing["p50_ns"] <= timing["p95_ns"] <= timing["p99_ns"] <= timing["max_ns"]):
            raise ReportError("invalid timing capacity/samples/percentile order")
        result[name] = timing
        offset += 1
    jitter = _match(r"synthetic_generated_interval_jitter_ns=Some\(IntervalJitterSummary \{ (.+) \}\) nominal_ns=([0-9]+) clock_domain=([0-9]+) pairs=([0-9]+) \(generated timestamps; not measured callback/device timing\)", lines[offset])
    summary = _fields(jitter[1], JITTER_FIELDS, ("min_deviation_ns", "max_deviation_ns"))
    if (summary["samples"] != min(options["iterations"], options["telemetry_samples"]) or
            summary["min_deviation_ns"] > summary["max_deviation_ns"] or
            not summary["p50_abs_deviation_ns"] <= summary["p95_abs_deviation_ns"] <= summary["p99_abs_deviation_ns"] <= summary["max_abs_deviation_ns"]):
        raise ReportError("invalid synthetic retained samples/percentiles")
    result["synthetic_generated_interval_jitter_ns"] = summary
    for name, value, expected in zip(("synthetic_nominal_ns", "synthetic_clock_domain", "synthetic_pairs"), jitter.groups()[1:], (1000000, 90, options["iterations"])):
        result[name] = _integer(value)
        if result[name] != expected:
            raise ReportError("unexpected synthetic provenance")
    offset += 1
    for name, kind, fields in (("software_runtime_counters", "RuntimeCounters", RUNTIME_FIELDS),
                               ("software_mixer_counters", "AudioCounters", MIXER_FIELDS)):
        match = _match(name + "=" + kind + r" \{ (.+) \}", lines[offset])
        result[name] = _fields(match[1], fields)
        offset += 1
    if (result["software_runtime_counters"]["inputs"] != result["software_operations"] or
            result["software_mixer_counters"]["rendered_frames"] != result["rendered_frames"]):
        raise ReportError("counter totals disagree with measured counts")
    if offset != len(lines) - 1 or lines[offset] != " ".join(name + "=unavailable" for name in UNAVAILABLE):
        raise ReportError("unknown or missing unavailable metric declaration")
    return result


def _read_output(handle):
    handle.seek(0)
    data = handle.read(MAX_OUTPUT + 1)
    if len(data) > MAX_OUTPUT:
        raise ReportError("child output exceeds 64KiB")
    try:
        return data.decode("utf-8", errors="strict")
    except UnicodeError as exc:
        raise ReportError("child output is not UTF-8") from exc


def _resources(usage=None):
    result = {"method": "unavailable", "cpu_user_seconds": None,
              "cpu_system_seconds": None, "peak_rss_bytes": None,
              "scope": "whole_child_lifetime"}
    if usage is not None:
        result["method"] = "wait4"
        for name, value in (("cpu_user_seconds", usage.ru_utime), ("cpu_system_seconds", usage.ru_stime)):
            if not math.isfinite(value) or value < 0:
                raise ReportError("invalid child resource observation")
            result[name] = value
        if sys.platform in ("linux", "darwin"):
            rss = usage.ru_maxrss
            if not math.isfinite(rss) or rss < 0:
                raise ReportError("invalid peak RSS observation")
            result["peak_rss_bytes"] = int(rss) * (1024 if sys.platform == "linux" else 1)
    return result


def run_process(binary, args, timeout_seconds):
    if not isinstance(timeout_seconds, (int, float)) or not math.isfinite(timeout_seconds) or timeout_seconds <= 0:
        raise ReportError("timeout must be finite and positive")
    posix_wait4 = os.name == "posix" and callable(getattr(os, "wait4", None))
    process = None
    usage = None
    started = time.monotonic_ns()
    with tempfile.TemporaryFile() as stdout, tempfile.TemporaryFile() as stderr:
        try:
            process = subprocess.Popen([os.fspath(binary), *args], stdin=subprocess.DEVNULL,
                                       stdout=stdout, stderr=stderr, shell=False,
                                       start_new_session=os.name == "posix")
            deadline = time.monotonic() + timeout_seconds
            while True:
                if posix_wait4:
                    pid, status, observation = os.wait4(process.pid, os.WNOHANG)
                    if pid:
                        process.returncode = os.waitstatus_to_exitcode(status)
                        usage = observation
                        break
                elif process.poll() is not None:
                    break
                if os.fstat(stdout.fileno()).st_size > MAX_OUTPUT or os.fstat(stderr.fileno()).st_size > MAX_OUTPUT:
                    raise ReportError("child output exceeds 64KiB")
                if time.monotonic() >= deadline:
                    raise ReportError("child process timed out")
                time.sleep(min(0.01, max(0, deadline - time.monotonic())))
            wall_ns = time.monotonic_ns() - started
            output, errors = _read_output(stdout), _read_output(stderr)
            if process.returncode != 0:
                raise ReportError(f"child exited with status {process.returncode}: {errors[:1000]}")
            result = {"stdout": output, "stderr": errors, "exit_code": process.returncode,
                      "wall_ns": wall_ns, "resources": _resources(usage)}
            return result
        except OSError as exc:
            raise ReportError(f"cannot execute benchmark: {exc}") from exc
        finally:
            if process is not None:
                try:
                    if os.name == "posix":
                        os.killpg(process.pid, signal.SIGKILL)
                    elif process.returncode is None:
                        process.kill()
                except ProcessLookupError:
                    pass
                finally:
                    if process.returncode is not None:
                        pass
                    elif posix_wait4:
                        while True:
                            try:
                                _, status, _ = os.wait4(process.pid, 0)
                                process.returncode = os.waitstatus_to_exitcode(status)
                                break
                            except InterruptedError:
                                continue
                    else:
                        process.wait()


def _artifact(binary):
    digest = hashlib.sha256()
    size = 0
    try:
        with open(binary, "rb") as handle:
            for chunk in iter(lambda: handle.read(1024 * 1024), b""):
                digest.update(chunk)
                size += len(chunk)
    except OSError as exc:
        raise ReportError(f"cannot read binary: {exc}") from exc
    return {"sha256": digest.hexdigest(), "bytes": size}


def _git_metadata():
    root = pathlib.Path(__file__).resolve().parents[1]
    try:
        revision = subprocess.run(["git", "rev-parse", "HEAD"], cwd=root, capture_output=True,
                                  timeout=2, check=True).stdout.decode("ascii").strip()
        if not re.fullmatch(r"[0-9a-f]{40,64}", revision):
            return None
        dirty = subprocess.run(["git", "--no-optional-locks", "status", "--porcelain", "--untracked-files=no"],
                               cwd=root, capture_output=True, timeout=2, check=True).stdout
        return {"revision": revision, "tracked_dirty": bool(dirty)}
    except (OSError, subprocess.SubprocessError, UnicodeError):
        return None


def collect_report(binary, iterations=2000, warmup=100, repeats=3, timeout_seconds=30, *, runner=None):
    binary = pathlib.Path(binary).resolve()
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise ReportError("--binary must name a prebuilt executable file")
    if type(repeats) is not int or not 1 <= repeats <= 10:
        raise ReportError("repeats must be 1..10")
    if not isinstance(timeout_seconds, (int, float)) or not math.isfinite(timeout_seconds) or not 1 <= timeout_seconds <= 240:
        raise ReportError("timeout must be finite and 1..240 seconds")
    options = _options({"iterations": iterations, "warmup": warmup, "buffer_frames": 32,
                        "queue_capacity": 64, "telemetry_samples": 4096, "sample_rate": 44100})
    artifact = _artifact(binary)
    runner = run_process if runner is None else runner
    cases = []
    assertion_mode = None
    for sample_rate in (44100, 48000):
        for buffer_frames in (32, 128, 512):
            case_options = dict(options, sample_rate=sample_rate, buffer_frames=buffer_frames)
            args = [item for name, value in case_options.items() for item in ("--" + name.replace("_", "-"), str(value))]
            repetitions = []
            deterministic = None
            for repetition in range(1, repeats + 1):
                observation = runner(binary, args, timeout_seconds)
                if observation.get("exit_code") != 0:
                    raise ReportError("child process failed")
                measurements = parse_measurements(observation["stdout"], case_options)
                mode = measurements["debug_assertions"]
                if assertion_mode is not None and assertion_mode != mode:
                    raise ReportError("binary assertion mode changed")
                assertion_mode = mode
                signature = [measurements[key] for key in ("measured_blocks", "software_operations", "rendered_frames", "pcm_checksum", "software_runtime_counters", "software_mixer_counters", "synthetic_generated_interval_jitter_ns")]
                if deterministic is not None and deterministic != signature:
                    raise ReportError("repeated deterministic observations differ")
                deterministic = signature
                repetitions.append({"repetition": repetition, **observation, "measurements": measurements})
            cases.append({"options": case_options, "args": args, "repetitions": repetitions})
    if _artifact(binary) != artifact:
        raise ReportError("binary artifact changed during matrix")
    return {"schema_version": 1, "workload_id": WORKLOAD, "debug_assertions": assertion_mode,
            "settings": {"iterations": iterations, "warmup": warmup, "repeats": repeats,
                         "timeout_seconds": timeout_seconds, "queue_capacity": 64, "telemetry_samples": 4096},
            "artifact": artifact,
            "host": {"os": platform.system(), "machine": platform.machine(),
                     "logical_cpus": os.cpu_count(), "python": platform.python_version()},
            "git": _git_metadata(), "unavailable": {name: None for name in (*UNAVAILABLE, "gpu_usage")},
            "resource_units": {"wall_ns": "nanoseconds", "cpu_user_seconds": "seconds",
                               "cpu_system_seconds": "seconds", "peak_rss_bytes": "bytes"},
            "resource_scope": "whole_child_lifetime including spawn, startup, preparation, warmup, measurement and reporting",
            "cases": cases}


def _preflight_output(output_path):
    path = pathlib.Path(output_path)
    if os.path.lexists(path):
        raise ReportError("output already exists (including symlinks)")
    if not path.parent.is_dir():
        raise ReportError("output parent directory does not exist")
    return path


def publish_report(report, output_path):
    text = json.dumps(report, indent=2, allow_nan=False) + "\n"
    if output_path is None:
        sys.stdout.write(text)
        return
    path = _preflight_output(output_path)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=path.parent,
                                         prefix=".runtime-benchmark-", delete=False) as handle:
            temporary = pathlib.Path(handle.name)
            handle.write(text)
            handle.flush()
            os.fsync(handle.fileno())
        os.link(temporary, path)
    except OSError as exc:
        raise ReportError(f"atomic no-overwrite publication failed: {exc}") from exc
    finally:
        if temporary is not None:
            temporary.unlink()


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, help="prebuilt runtime_bench executable; never built automatically")
    parser.add_argument("--output", help="new JSON path; existing entries are refused (default stdout)")
    parser.add_argument("--iterations", type=int, default=2000, help="measured blocks, 1..10000 (default 2000)")
    parser.add_argument("--warmup", type=int, default=100, help="warmup blocks, 0..1000 (default 100)")
    parser.add_argument("--repeats", type=int, default=3, help="repetitions per case, 1..10 (default 3)")
    parser.add_argument("--timeout", type=float, default=30, help="finite seconds per child, 1..240 (default 30)")
    args = parser.parse_args(argv)
    try:
        if args.output is not None:
            _preflight_output(args.output)
        report = collect_report(args.binary, args.iterations, args.warmup, args.repeats, args.timeout)
        publish_report(report, args.output)
    except (ReportError, OSError, ValueError) as exc:
        print(f"runtime benchmark: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
