// Authored for deferred execution with Node's built-in test runner.
import assert from "node:assert/strict";
import { File as NodeFile } from "node:buffer";
import test from "node:test";
import {
  LIMITS, snapshotFiles, normalizedPath, preflight, seconds, nanoseconds,
  previewNanos,
} from "./host_model.mjs";

globalThis.File ??= NodeFile;

function file(name, size = 1) {
  const value = new File(["x"], name);
  Object.defineProperty(value, "size", { value: size });
  return value;
}

test("decimal conversion retains one nanosecond, twenty hours, a week and values above Number precision", () => {
  for (const [ns, decimal] of [
    ["0", "0"],
    ["1", "0.000000001"],
    ["12345678901", "12.345678901"],
    ["72000000000000", "72000"],
    ["604800000000000", "604800"],
    ["9007199254740993", "9007199.254740993"],
    ["9223372034854775807", "9223372034.854775807"],
  ]) {
    assert.equal(seconds(ns), decimal);
    assert.equal(nanoseconds(decimal), ns);
    assert.equal(previewNanos(ns), BigInt(ns));
  }
  assert.equal(seconds("9223372036854775807"), "9223372036.854775807");
  assert.equal(nanoseconds("0000000001.010000000"), "1010000000");
});

test("preview boundaries reserve the exact two-second lookahead without rounding", () => {
  assert.equal(previewNanos("9223372034854775807"), 9223372034854775807n);
  for (const value of [
    "9223372034854775808", "9223372036854775807", "18446744073709551616",
    "-1", "+1", "1.0", "1e9", "", " 1", "1 ",
  ]) assert.throws(() => previewNanos(value), value);
  for (const value of [
    "9223372034.854775808", "9223372036.854775807", "9999999999.999999999",
    "-0.1", "+1", ".1", "1.", "1.0000000001", "1e3", " 1", "1 ", "",
  ]) assert.throws(() => nanoseconds(value), value);
});

test("signed replay display keeps one leading sign while preview admission remains nonnegative", () => {
  for (const [ns, decimal] of [
    ["-1", "-0.000000001"],
    ["-100000000", "-0.1"],
    ["-999999999", "-0.999999999"],
    ["-1000000000", "-1"],
    ["-1000000001", "-1.000000001"],
    ["-12345678901", "-12.345678901"],
    ["-604800000000000", "-604800"],
    ["-9223372036854775808", "-9223372036.854775808"],
  ]) {
    assert.equal(seconds(ns), decimal);
    assert.throws(() => previewNanos(ns));
    assert.throws(() => nanoseconds(decimal));
  }
  assert.equal(seconds("-0"), "0");
});

test("relative paths preserve case and Unicode while rejecting unsafe names", () => {
  assert.equal(normalizedPath("Song\\.\\音楽//Kick.WAV"), "Song/音楽/Kick.WAV");
  assert.equal(normalizedPath("./song/chart.bms"), "song/chart.bms");
  assert.equal(normalizedPath("가", 3), "가");
  assert.throws(() => normalizedPath("가", 2));
  for (const value of ["", ".", "./", "/song.bms", "\\song.bms", "C:\\song.bms", "C:song.bms", "song/../kick.wav", "a\0b"]) {
    assert.throws(() => normalizedPath(value), value);
  }
  assert.throws(() => normalizedPath(null));
});

test("folder paths are captured before structured cloning drops File extension properties", () => {
  const original = new File(["#BPM 120"], "chart.bms");
  Object.defineProperty(original, "webkitRelativePath", { value: "Songs/曲/chart.bms" });
  const entries = snapshotFiles({ 0: original, length: 1 });
  assert.equal(entries[0].path, "Songs/曲/chart.bms");
  assert.equal(entries[0].file, original);

  const cloned = structuredClone(entries);
  assert.equal(cloned[0].path, "Songs/曲/chart.bms");
  assert.equal(cloned[0].file.webkitRelativePath, undefined);
  // Node versions can deserialize File as Blob. Supply the standard File
  // fields explicitly while retaining the real cloned envelope's path.
  const received = new File([cloned[0].file], original.name);
  const admitted = preflight([{ file: received, path: cloned[0].path }]);
  assert.equal(admitted[0].path, "Songs/曲/chart.bms");
  assert.equal(snapshotFiles([new File(["x"], "flat.bms")])[0].path, "flat.bms");
});

test("preflight sorts normalized paths and admits exact configured budgets", () => {
  const limits = { files: 2, file: 4, total: 8, path: 32 };
  const first = file("b.wav", 4);
  const second = file("a.bms", 4);
  const entries = preflight([
    { file: first, path: "Song\\b.wav" },
    { file: second, path: "Song/./a.bms" },
  ], limits);
  assert.deepEqual(entries.map(entry => entry.path), ["Song/a.bms", "Song/b.wav"]);
  assert.equal(entries[0].file, second);
  assert.equal(entries[1].file, first);
  assert.equal(LIMITS.file, 67108864);
  assert.equal(LIMITS.total, 268435456);
});

test("invalid metadata and namespace collisions reject before any byte acquisition", () => {
  let reads = 0;
  const watched = (name, size) => {
    const value = file(name, size);
    value.arrayBuffer = () => { reads++; throw new Error("metadata must be checked first"); };
    return value;
  };
  const entry = (path, size = 1) => ({ file: watched("file", size), path });
  const limits = { files: 2, file: 4, total: 6, path: 32 };
  for (const entries of [
    [],
    [entry("a"), entry("b"), entry("c")],
    [entry("a", 5)],
    [entry("a", 4), entry("b", 3)],
    [entry("a", -1)],
    [entry("a", 1.5)],
    [entry("a", Number.MAX_SAFE_INTEGER + 1)],
    [entry("a"), entry("./a")],
    [entry("a"), entry("a/b")],
    [entry("a/b"), entry("a")],
    [entry("a"), entry("../bad")],
    [entry("a"), { file: { size: 1, arrayBuffer() { reads++; } }, path: "b" }],
    [entry("x".repeat(33))],
  ]) assert.throws(() => preflight(entries, limits));
  assert.equal(reads, 0);
});
