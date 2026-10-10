#!/usr/bin/env python3
"""Validate producer-observed loading trials; never synthesize missing evidence.

Input schema 1: one software path/workload/environment, canonical fixture/config
hashes, and paired before/after groups for each (scenario, temperature). Each
trial uses relative monotonic milliseconds, starting at its observed request.
Quantiles use nearest rank: sorted[ceil(p * n) - 1], including p50 (no averaging).
Artifact bytes and event origins are observed by the runner; this consumer checks
that the supplied loaded hashes/correlation match their declared provenance.
"""
import argparse
import json
import math
import pathlib
import re
import sys

MAX_INPUT_BYTES = 8 * 1024 * 1024
EVENTS = (
    "request", "first_pending_frame", "catalog_selectable", "prepared_ready",
    "first_presented_usable", "audio_ready", "held_work_released",
)


class ReportError(ValueError):
    """Records do not support a comparable software loading report."""


def _fields(value, names, label):
    if type(value) is not dict or set(value) != set(names):
        raise ReportError(f"{label}: expected exactly {', '.join(names)}")
    return value


def _text(value, label):
    if type(value) is not str or not value.strip():
        raise ReportError(f"{label}: expected a nonempty string")
    return value


def _hash(value, label):
    if type(value) is not str or re.fullmatch(r"[0-9a-f]{64}", value) is None:
        raise ReportError(f"{label}: expected lowercase SHA-256")
    return value


def _number(value, label):
    if type(value) not in (int, float):
        raise ReportError(f"{label}: expected a finite nonnegative number")
    try:
        finite = math.isfinite(value)
    except OverflowError:
        finite = False
    if not finite or value < 0:
        raise ReportError(f"{label}: expected a finite nonnegative number")
    return value


def _manifest(value, label):
    if type(value) is not dict or not value:
        raise ReportError(f"{label}: observed artifact provenance is required")
    for url, digest in value.items():
        _text(url, f"{label} URL")
        _hash(digest, f"{label} hash")
    return dict(value)


def _measurements(value, label, integers=False):
    if type(value) is not dict:
        raise ReportError(f"{label}: expected an object (null values mean unavailable)")
    for key, number in value.items():
        _text(key, f"{label} name")
        if number is None:
            continue
        _number(number, f"{label}.{key}")
        if integers and (type(number) is not int or number > 2**64 - 1):
            raise ReportError(f"{label}.{key}: expected a nonnegative u64 counter")
    return dict(value)


def _samples(value, label):
    if type(value) is not list:
        raise ReportError(f"{label}: expected an array of actual latency observations")
    return [_number(number, label) for number in value]


def summarize(values):
    """Nearest-rank statistics over observed values only; empty is unavailable."""
    ordered = sorted(values)
    if not ordered:
        return {"samples": 0, "p50": None, "p95": None, "max": None}
    return {"samples": len(ordered),
            "p50": ordered[math.ceil(0.50 * len(ordered)) - 1],
            "p95": ordered[math.ceil(0.95 * len(ordered)) - 1],
            "max": ordered[-1]}


def _trial(value, group, seen_trials, cold_owners):
    _fields(value, ("trial_id", "process_id", "library_id", "cache_owner_id",
                    "reused_owner_id", "correlation_id", "events", "loaded_artifacts",
                    "phases_ms", "counters", "input_latency_ms", "redraw_latency_ms"), "trial")
    for key in ("trial_id", "process_id", "library_id", "cache_owner_id", "correlation_id"):
        _text(value[key], key)
    if value["trial_id"] in seen_trials:
        raise ReportError("duplicate trial identity")
    seen_trials.add(value["trial_id"])
    if group["temperature"] == "cold":
        if value["reused_owner_id"] is not None:
            raise ReportError("cold trial cannot reuse an owner")
        for key in cold_owners:
            if value[key] in cold_owners[key]:
                raise ReportError(f"cold trials require fresh {key}")
            cold_owners[key].add(value[key])
    elif value["reused_owner_id"] != value["cache_owner_id"]:
        raise ReportError("warm trial must identify its exact reused cache owner")
    loaded = _manifest(value["loaded_artifacts"], "loaded_artifacts")
    for url, digest in loaded.items():
        if group["artifact_manifest"].get(url) != digest:
            raise ReportError("loaded artifact differs from frozen manifest")
    if type(value["events"]) is not list:
        raise ReportError("events must be observed event records")
    observed = {}
    previous = -1
    for event in value["events"]:
        _fields(event, ("name", "at_ms", "correlation_id", "source"), "event")
        if event["name"] not in EVENTS or event["name"] in observed:
            raise ReportError("unknown or duplicate event")
        if event["correlation_id"] != value["correlation_id"]:
            raise ReportError("event belongs to a different request owner")
        _text(event["source"], "event source")
        at = _number(event["at_ms"], "event time")
        if at < previous:
            raise ReportError("event observations are out of monotonic order")
        previous = at
        observed[event["name"]] = at
    if observed.get("request") != 0 or not value["events"] or value["events"][0]["name"] != "request":
        raise ReportError("first event must be the observed request normalized to zero")
    usable = observed.get("first_presented_usable")
    ready = observed.get("catalog_selectable", observed.get("prepared_ready"))
    if usable is None or ready is None or usable < ready:
        raise ReportError("usable presentation requires preceding correlated readiness")
    if "audio_ready" in observed:
        if "prepared_ready" not in observed or observed["audio_ready"] < observed["prepared_ready"]:
            raise ReportError("audio-ready cannot precede actual preparation readiness")
    return {**value, "events_ms": observed,
            "phases_ms": _measurements(value["phases_ms"], "phases_ms"),
            "counters": _measurements(value["counters"], "counters", integers=True),
            "input_latency_ms": _samples(value["input_latency_ms"], "input_latency_ms"),
            "redraw_latency_ms": _samples(value["redraw_latency_ms"], "redraw_latency_ms")}


def _group(value, canonical, seen_trials, cold_owners):
    _fields(value, ("variant", "scenario", "temperature", "fixture_sha256", "config_sha256",
                    "source_revision", "artifact_manifest", "trials"), "group")
    if value["variant"] not in ("before", "after") or value["temperature"] not in ("cold", "warm"):
        raise ReportError("invalid variant or temperature")
    _text(value["scenario"], "scenario")
    for key in ("fixture_sha256", "config_sha256"):
        if _hash(value[key], key) != canonical[key]:
            raise ReportError(f"incomparable {key}")
    if type(value["source_revision"]) is not str or re.fullmatch(r"[0-9a-f]{7,40}", value["source_revision"]) is None:
        raise ReportError("source_revision must be 7..40 lowercase hexadecimal characters")
    manifest = _manifest(value["artifact_manifest"], "artifact_manifest")
    if type(value["trials"]) is not list or len(value["trials"]) < 5:
        raise ReportError("at least five comparable trials are required per group")
    group = {**value, "artifact_manifest": manifest}
    group["trials"] = [_trial(trial, group, seen_trials, cold_owners) for trial in value["trials"]]
    result = {"trial_count": len(group["trials"]), "source_revision": value["source_revision"],
              "artifact_manifest": manifest, "trials": group["trials"]}
    for category in ("events_ms", "phases_ms", "counters"):
        names = set(EVENTS) if category == "events_ms" else set().union(*(t[category] for t in group["trials"]))
        result[category] = {name: summarize([t[category][name] for t in group["trials"] if t[category].get(name) is not None]) for name in sorted(names)}
    for category in ("input_latency_ms", "redraw_latency_ms"):
        result[category] = summarize([sample for trial in group["trials"] for sample in trial[category]])
    result["usable_before_held_release"] = [
        t["events_ms"]["first_presented_usable"] < t["events_ms"]["held_work_released"]
        if "held_work_released" in t["events_ms"] else None for t in group["trials"]]
    return result


def build_report(records):
    _fields(records, ("schema_version", "workload_id", "path", "environment", "fixture_sha256",
                      "config_sha256", "groups"), "input")
    if type(records["schema_version"]) is not int or records["schema_version"] != 1:
        raise ReportError("unsupported schema_version")
    _text(records["workload_id"], "workload_id")
    if records["path"] not in ("native", "browser"):
        raise ReportError("path must be native or browser")
    if type(records["environment"]) is not dict or not records["environment"]:
        raise ReportError("actual environment is required")
    for key in ("fixture_sha256", "config_sha256"):
        _hash(records[key], key)
    if type(records["groups"]) is not list or not records["groups"]:
        raise ReportError("paired trial groups are required")
    paired = {}
    seen_trials = set()
    cold_owners = {"process_id": set(), "library_id": set(), "cache_owner_id": set()}
    for value in records["groups"]:
        group = _group(value, records, seen_trials, cold_owners)
        pair = paired.setdefault((value["scenario"], value["temperature"]), {})
        if value["variant"] in pair:
            raise ReportError("duplicate scenario/temperature/variant group")
        pair[value["variant"]] = group
    comparisons = []
    for (scenario, temperature), pair in sorted(paired.items()):
        if set(pair) != {"before", "after"}:
            raise ReportError("each scenario/temperature requires before and after groups")
        metrics = {}
        for category in ("events_ms", "phases_ms", "counters", "input_latency_ms", "redraw_latency_ms"):
            before, after = pair["before"][category], pair["after"][category]
            names = set(before) | set(after) if category in ("events_ms", "phases_ms", "counters") else {"latency"}
            for name in sorted(names):
                mapped = category in ("events_ms", "phases_ms", "counters")
                b = before.get(name, summarize([])) if mapped else before
                a = after.get(name, summarize([])) if mapped else after
                complete = b["samples"] == pair["before"]["trial_count"] and a["samples"] == pair["after"]["trial_count"]
                if not mapped:
                    complete = all(t[category] for t in pair["before"]["trials"] + pair["after"]["trials"])
                comparable = bool(complete)
                metrics[f"{category}/{name}"] = {
                    "comparable": comparable,
                    "reason": None if comparable else "unavailable or incomplete observations; no before/after claim",
                    "before": b, "after": a,
                    **{f"{quantile}_delta": a[quantile] - b[quantile] if comparable else None for quantile in ("p50", "p95", "max")}}
        comparisons.append({"scenario": scenario, "temperature": temperature, **pair, "metrics": metrics})
    return {"schema_version": 1, "workload_id": records["workload_id"], "path": records["path"],
            "environment": records["environment"], "fixture_sha256": records["fixture_sha256"],
            "config_sha256": records["config_sha256"], "quantile_method": "nearest-rank",
            "delta_definition": "after minus before; milliseconds for _ms categories, counts for counters",
            "evidence_scope": "producer-observed software events and declared loaded artifact hashes; no physical latency proof",
            "comparisons": comparisons}


def _object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ReportError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", type=pathlib.Path)
    parser.add_argument("--output", type=pathlib.Path)
    args = parser.parse_args(argv)
    try:
        with args.input.open("rb") as stream:
            raw = stream.read(MAX_INPUT_BYTES + 1)
        if len(raw) > MAX_INPUT_BYTES:
            raise ReportError("loading records exceed 8 MiB")
        records = json.loads(raw.decode("utf-8"), object_pairs_hook=_object,
                             parse_constant=lambda value: (_ for _ in ()).throw(ReportError(f"nonfinite JSON value: {value}")))
        rendered = json.dumps(build_report(records), ensure_ascii=False, indent=2, allow_nan=False) + "\n"
        if args.output:
            args.output.write_text(rendered, encoding="utf-8")
        else:
            sys.stdout.write(rendered)
        return 0
    except (ReportError, OSError, UnicodeError, json.JSONDecodeError, TypeError, OverflowError) as error:
        print(f"loading_report: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
