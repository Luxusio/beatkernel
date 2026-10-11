#!/usr/bin/env python3
"""Validated software-only dense chart evidence from a prebuilt executable."""
import argparse
import importlib.util
import math
import os
import pathlib
import platform
import re
import sys

# Resolve the sibling by file so direct execution and file-based imports work
# even when the caller's current directory is outside this repository.
if __package__:
    from . import runtime_benchmark as shared
else:
    _spec = importlib.util.spec_from_file_location(
        "runtime_benchmark", pathlib.Path(__file__).with_name("runtime_benchmark.py"))
    shared = importlib.util.module_from_spec(_spec)
    _spec.loader.exec_module(shared)

ReportError = shared.ReportError
run_process = shared.run_process
publish_report = shared.publish_report
WORKLOAD = "beatkernel-dense-chart-v1"
OPTION_LIMITS = {"notes": (1, 100000), "lanes": (1, 64),
                 "seek_cycles": (1, 1000), "origin_ns": (0, 604800000000000)}
MATRIX = ({"notes": 20000, "lanes": 8, "seek_cycles": 16, "origin_ns": 0},
          {"notes": 100000, "lanes": 64, "seek_cycles": 4, "origin_ns": 0})
COUNT_FIELDS = ("hold_count", "record_count", "result_count", "checkpoint_count")
CHECK_FIELDS = ("seek_checks", "projection_checks", "max_visible",
                "final_engine_hash", "retained_probes")
PROBE_FIELDS = ("last_probe_target_ns", "last_probe_cursor", "last_probe_result_count",
                "last_probe_engine_hash", "last_probe_visible_count")
TIMING_FIELDS = ("setup_ns", "recording_ns", "seek_ns", "projection_ns", "verification_ns")
UNAVAILABLE = ("audio", "native_clocks_latency", "gpu_rendering", "physical_devices",
               "whole_player_behavior", "actual_wall_clock_soak")
TIMING_NOTICE = ("software-only; phase timings are informational observations with no "
                 "performance threshold, ranking or universal zero-cost claim")
PLACEMENT_NOTICE = "long origins test absolute timestamp placement only; lanes are not multiplayer members"


def _options(options):
    if not isinstance(options, dict) or set(options) != set(OPTION_LIMITS):
        raise ReportError("options must contain exactly the known Rust fields")
    for name, (low, high) in OPTION_LIMITS.items():
        if type(options[name]) is not int or not low <= options[name] <= high:
            raise ReportError(f"invalid {name}: expected {low}..{high}")
    if options["notes"] * (options["seek_cycles"] + 1) > 5000000:
        raise ReportError("notes * (seek_cycles + 1) must not exceed 5000000")
    return dict(options)


def _line(text, fields, bits=64):
    match = shared._match(" ".join(re.escape(name) + r"=([0-9]+)" for name in fields), text)
    values = {}
    for name, value in zip(fields, match.groups()):
        if len(value) > (39 if bits == 128 else 20):
            raise ReportError("integer observation outside bounds")
        number = int(value)
        if number > 2**bits - 1:
            raise ReportError("integer observation outside bounds")
        values[name] = number
    return values


def _expected(options):
    """Count source rows, inclusive record prefixes and geometric overlaps."""
    notes, lanes = options["notes"], options["lanes"]
    rows = (notes + lanes - 1) // lanes
    counts = [min(lanes, notes - row * lanes) for row in range(rows)]
    holds = sum(count for row, count in enumerate(counts) if row % 4 == 0)
    expected = {"hold_count": holds, "record_count": 2 * notes,
                "result_count": notes + holds, "checkpoint_count": 3,
                "seek_checks": options["seek_cycles"],
                "projection_checks": options["seek_cycles"],
                "retained_probes": options["seek_cycles"], "max_visible": 0}
    for cycle in range(options["seek_cycles"]):
        slot = cycle % 5
        row = rows - 1 if slot == 4 else ((cycle // 5) % ((rows + 3) // 4)) * 4
        target = options["origin_ns"] + row * 4000000 + (3 if slot == 4 else slot) * 1000000
        radius = 500000 if slot == 2 else 4000000
        cursor = results = visible = 0
        for source_row, count in enumerate(counts):
            head = options["origin_ns"] + source_row * 4000000
            tail = head + 2000000
            hold = source_row % 4 == 0
            if head <= target:
                cursor += count
                results += count
            if tail <= target:
                cursor += count
                if hold:
                    results += count
            if head <= target + radius and (tail if hold else head) >= target - radius:
                visible += count
        expected["max_visible"] = max(expected["max_visible"], visible)
        expected.update(last_probe_target_ns=target, last_probe_cursor=cursor,
                        last_probe_result_count=results, last_probe_visible_count=visible)
    return expected


def parse_measurements(stdout, options):
    options = _options(options)
    if not isinstance(stdout, str):
        raise ReportError("stdout must be UTF-8 text")
    try:
        if len(stdout.encode("utf-8")) > shared.MAX_OUTPUT:
            raise ReportError("benchmark output exceeds 64KiB")
    except UnicodeError as exc:
        raise ReportError("invalid UTF-8 output") from exc
    lines = stdout.splitlines()
    if len(lines) != 9:
        raise ReportError("missing, duplicate or unknown benchmark lines")
    banner = shared._match(r"benchmark_schema=1 workload_id=" + WORKLOAD +
                           r" debug_assertions=(true|false)", lines[0])
    if _line(lines[1], OPTION_LIMITS) != options:
        raise ReportError("reported settings differ from requested options")
    result = {"benchmark_schema": 1, "workload_id": WORKLOAD,
              "debug_assertions": banner[1] == "true", "options": options}
    for line, fields in zip(lines[2:5], (COUNT_FIELDS, CHECK_FIELDS, PROBE_FIELDS)):
        result.update(_line(line, fields))
    result.update(_line(lines[5], TIMING_FIELDS, bits=128))
    if (lines[6] != TIMING_NOTICE or
            lines[7] != " ".join(name + "=unavailable" for name in UNAVAILABLE) or
            lines[8] != PLACEMENT_NOTICE):
        raise ReportError("unknown or missing software evidence declaration")
    for name, expected in _expected(options).items():
        if result[name] != expected:
            raise ReportError(f"{name} differs from workload oracle")
    return result


def collect_report(binary, repeats=3, timeout_seconds=30, *, runner=None):
    binary = pathlib.Path(binary).resolve()
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise ReportError("--binary must name a prebuilt executable file")
    if type(repeats) is not int or not 1 <= repeats <= 10:
        raise ReportError("repeats must be 1..10")
    if (type(timeout_seconds) not in (int, float) or not math.isfinite(timeout_seconds)
            or not 1 <= timeout_seconds <= 240):
        raise ReportError("timeout must be finite and 1..240 seconds")
    artifact = shared._artifact(binary)
    runner = run_process if runner is None else runner
    cases = []
    assertion_mode = None
    for fixed_options in MATRIX:
        options = _options(fixed_options)
        args = [item for name, value in options.items()
                for item in ("--" + name.replace("_", "-"), str(value))]
        repetitions = []
        deterministic = None
        for repetition in range(1, repeats + 1):
            observation = runner(binary, args, timeout_seconds)
            if observation.get("exit_code") != 0:
                raise ReportError("child process failed")
            measurements = parse_measurements(observation.get("stdout"), options)
            mode = measurements["debug_assertions"]
            if assertion_mode is not None and assertion_mode != mode:
                raise ReportError("binary assertion mode changed")
            assertion_mode = mode
            signature = {name: value for name, value in measurements.items()
                         if name not in TIMING_FIELDS}
            if deterministic is not None and deterministic != signature:
                raise ReportError("repeated deterministic observations differ")
            deterministic = signature
            repetitions.append({"repetition": repetition, **observation,
                                "argv": [os.fspath(binary), *args], "measurements": measurements})
        cases.append({"options": options, "args": args, "repetitions": repetitions})
    if shared._artifact(binary) != artifact:
        raise ReportError("binary artifact changed during matrix")
    return {"schema_version": 1, "workload_id": WORKLOAD, "debug_assertions": assertion_mode,
            "settings": {"repeats": repeats, "timeout_seconds": timeout_seconds},
            "artifact": artifact,
            "host": {"os": platform.system(), "machine": platform.machine(),
                     "logical_cpus": os.cpu_count(), "python": platform.python_version()},
            "git": shared._git_metadata(),
            "unavailable": {name: None for name in (*UNAVAILABLE, *shared.UNAVAILABLE, "gpu_usage")},
            "phase_timing_units": {name: "nanoseconds" for name in TIMING_FIELDS},
            "resource_units": {"wall_ns": "nanoseconds", "cpu_user_seconds": "seconds",
                               "cpu_system_seconds": "seconds", "peak_rss_bytes": "bytes"},
            "resource_scope": "whole_child_lifetime including spawn, startup, setup, recording, seek, projection, verification and reporting",
            "cases": cases}


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, help="prebuilt dense_chart_stress executable; never built automatically")
    parser.add_argument("--output", help="new JSON path; existing entries refused (default stdout)")
    parser.add_argument("--repeats", type=int, default=3, help="repetitions per case, 1..10 (default 3)")
    parser.add_argument("--timeout", type=float, default=30, help="finite seconds per child, 1..240 (default 30)")
    args = parser.parse_args(argv)
    try:
        if args.output is not None:
            shared._preflight_output(args.output)
        report = collect_report(args.binary, args.repeats, args.timeout)
        publish_report(report, args.output)
    except (ReportError, OSError, ValueError) as exc:
        print(f"dense chart benchmark: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
