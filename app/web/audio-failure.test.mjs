import assert from "node:assert/strict";
import test from "node:test";
import {
  AUDIO_FAILURE_DIAGNOSTIC_VERSION,
  AUDIO_FAILURE_ORIGIN_CONTROL,
  AUDIO_FAILURE_ORIGIN_ARM,
  AUDIO_FAILURE_ORIGIN_PROCESS,
  readAudioFailureDiagnostics,
} from "./audio-failure.mjs";

const U32_MAX = 0xffffffff;
const diagnosticFields = [
  "diagnosticVersion", "origin", "ownerPhase",
  "currentFramePresent", "currentFrame", "blockFramesPresent", "blockFrames",
  "expectedFramePresent", "expectedFrameLow", "expectedFrameHigh",
  "startFramePresent", "startFrameLow", "startFrameHigh",
  "successfulArmFramePresent", "successfulArmFrame",
];
function terminal(fields = {}) {
  return {
    kind: "terminal", generation: 17, status: 6,
    diagnosticVersion: 1, origin: 2, ownerPhase: 2,
    currentFramePresent: 1, currentFrame: Number.MAX_SAFE_INTEGER,
    blockFramesPresent: 1, blockFrames: U32_MAX,
    expectedFramePresent: 1, expectedFrameLow: U32_MAX, expectedFrameHigh: U32_MAX,
    startFramePresent: 1, startFrameLow: 0, startFrameHigh: 0x80000000,
    successfulArmFramePresent: 1, successfulArmFrame: 0,
    ...fields,
  };
}
function knownFields(message) {
  return Object.fromEntries(diagnosticFields.map(field => [field, message[field]]));
}
function rejects(message) {
  assert.throws(() => readAudioFailureDiagnostics(message), TypeError);
}

test("v1 preserves exact independent frame facts including high words and u32 maxima", () => {
  assert.equal(AUDIO_FAILURE_DIAGNOSTIC_VERSION, 1);
  assert.equal(AUDIO_FAILURE_ORIGIN_CONTROL, 0);
  assert.equal(AUDIO_FAILURE_ORIGIN_ARM, 1);
  assert.equal(AUDIO_FAILURE_ORIGIN_PROCESS, 2);
  const message = terminal();
  const snapshot = readAudioFailureDiagnostics(message);
  assert.deepEqual(snapshot, knownFields(message));
  assert.equal(snapshot.expectedFrameHigh, U32_MAX);
  assert.equal(snapshot.startFrameHigh, 0x80000000);
  assert.equal(snapshot.currentFrame, Number.MAX_SAFE_INTEGER);
  assert.equal(snapshot.successfulArmFrame, 0);
  assert.ok(Object.isFrozen(snapshot));
  // Facts retain their individual meanings; there is no cursor reconstruction,
  // inferred clock mapping or report/output authority in the returned object.
  assert.deepEqual(Object.keys(snapshot).sort(), [...diagnosticFields].sort());
});

test("all origins and owner phases admit zero-valued present facts", () => {
  for (const origin of [0, 1, 2]) {
    for (const ownerPhase of [0, 1, 2, 3]) {
      const message = terminal({ origin, ownerPhase, currentFrame: 0, blockFrames: 0,
        expectedFrameLow: 0, expectedFrameHigh: 0, startFrameLow: 0, startFrameHigh: 0 });
      assert.deepEqual(readAudioFailureDiagnostics(message), knownFields(message));
    }
  }
});

test("canonical absent values remain distinct from present zero", () => {
  const zero = terminal({ currentFrame: 0, blockFrames: 0, expectedFrameLow: 0,
    expectedFrameHigh: 0, startFrameLow: 0, startFrameHigh: 0 });
  const absent = terminal({ ...zero, currentFramePresent: 0, blockFramesPresent: 0,
    expectedFramePresent: 0, startFramePresent: 0, successfulArmFramePresent: 0 });
  const presentSnapshot = readAudioFailureDiagnostics(zero);
  const absentSnapshot = readAudioFailureDiagnostics(absent);
  for (const field of ["currentFramePresent", "blockFramesPresent", "expectedFramePresent",
    "startFramePresent", "successfulArmFramePresent"]) {
    assert.equal(presentSnapshot[field], 1);
    assert.equal(absentSnapshot[field], 0);
  }
  assert.deepEqual(absentSnapshot, knownFields(absent));
  assert.notDeepEqual(presentSnapshot, absentSnapshot);
});

test("legacy absence does not read diagnostic facts or coerce a version", () => {
  for (const message of [{ kind: "terminal", generation: 17, status: 6 },
    { kind: "terminal", diagnosticVersion: undefined }]) {
    Object.defineProperty(message, "currentFrame", {
      get() { throw new Error("legacy fields must remain unread"); },
    });
    assert.equal(readAudioFailureDiagnostics(message), null);
  }
});

test("unknown or malformed diagnostic versions fail explicitly", () => {
  for (const diagnosticVersion of [0, 2, -1, 1.5, "1", null, true, NaN,
    Infinity, Number.MAX_SAFE_INTEGER + 1, 1n]) {
    rejects(terminal({ diagnosticVersion }));
  }
});

test("nonobject messages fail explicitly instead of becoming legacy absence", () => {
  for (const message of [null, undefined, 0, "terminal", false, 1n]) rejects(message);
});

test("versioned diagnostics require every field with its exact numeric type", () => {
  for (const field of diagnosticFields.filter(field => field !== "diagnosticVersion")) {
    const missing = terminal(); delete missing[field]; rejects(missing);
    for (const value of [undefined, null, "0", false, 0n, NaN, Infinity, -Infinity, {}, []]) {
      rejects(terminal({ [field]: value }));
    }
  }
});

test("presence flags accept only numeric zero and one", () => {
  for (const field of ["currentFramePresent", "blockFramesPresent", "expectedFramePresent",
    "startFramePresent", "successfulArmFramePresent"]) {
    for (const value of [-1, 2, 0.5, Number.MAX_SAFE_INTEGER]) {
      rejects(terminal({ [field]: value }));
    }
  }
});

test("origin and owner phase reject unknown and fractional enum values", () => {
  for (const origin of [-1, 3, 0.5, Number.MAX_SAFE_INTEGER]) rejects(terminal({ origin }));
  for (const ownerPhase of [-1, 4, 0.5, Number.MAX_SAFE_INTEGER]) rejects(terminal({ ownerPhase }));
});

test("frame integers never accept negative, fractional or unsafe values", () => {
  for (const field of ["currentFrame", "successfulArmFrame"]) {
    for (const value of [-1, 0.5, Number.MAX_SAFE_INTEGER + 1, 2 ** 64]) {
      rejects(terminal({ [field]: value }));
    }
  }
});

test("block count and native halves retain unsigned32 bounds", () => {
  for (const field of ["blockFrames", "expectedFrameLow", "expectedFrameHigh",
    "startFrameLow", "startFrameHigh"]) {
    for (const value of [-1, 0.5, U32_MAX + 1, Number.MAX_SAFE_INTEGER]) {
      rejects(terminal({ [field]: value }));
    }
    assert.equal(readAudioFailureDiagnostics(terminal({ [field]: 0 }))[field], 0);
    assert.equal(readAudioFailureDiagnostics(terminal({ [field]: U32_MAX }))[field], U32_MAX);
  }
});

test("absence rejects nonzero payloads including either native word", () => {
  for (const [presence, values] of [
    ["currentFramePresent", ["currentFrame"]],
    ["blockFramesPresent", ["blockFrames"]],
    ["expectedFramePresent", ["expectedFrameLow", "expectedFrameHigh"]],
    ["startFramePresent", ["startFrameLow", "startFrameHigh"]],
    ["successfulArmFramePresent", ["successfulArmFrame"]],
  ]) {
    const absent = terminal({ [presence]: 0, ...Object.fromEntries(values.map(field => [field, 0])) });
    assert.equal(readAudioFailureDiagnostics(absent)[presence], 0);
    for (const field of values) rejects({ ...absent, [field]: 1 });
  }
});

test("each known getter is read once and unrelated getters are never touched", () => {
  const expected = knownFields(terminal()), reads = new Map(), message = {};
  for (const [field, value] of Object.entries(expected)) {
    Object.defineProperty(message, field, { enumerable: true, get() {
      reads.set(field, (reads.get(field) ?? 0) + 1);
      assert.equal(reads.get(field), 1, `${field} must be read once`);
      return value;
    } });
  }
  for (const field of ["kind", "generation", "status", "report", "trustedOutput", "clockMapping", "arbitrary"]) {
    Object.defineProperty(message, field, { enumerable: true, get() {
      throw new Error(`unrelated field ${field} was read`);
    } });
  }
  assert.deepEqual(readAudioFailureDiagnostics(message), expected);
  assert.deepEqual([...reads.keys()].sort(), [...diagnosticFields].sort());
  for (const count of reads.values()) assert.equal(count, 1);
});

test("throwing known getters produce an explicit protocol TypeError", () => {
  for (const field of diagnosticFields) {
    const message = terminal(); let reads = 0;
    Object.defineProperty(message, field, { get() {
      reads++;
      throw new Error(`hostile ${field}`);
    } });
    rejects(message);
    assert.equal(reads, 1);
  }
});

test("the frozen known-only snapshot is independent of a mutable terminal", () => {
  const message = terminal({ arbitrary: { mutable: true }, report: { available: true },
    clockMapping: 7, trustedOutput: 1 });
  const expected = knownFields(message), snapshot = readAudioFailureDiagnostics(message);
  assert.notEqual(snapshot, message);
  for (const field of diagnosticFields) message[field] = 0;
  message.arbitrary.mutable = false;
  message.expectedFrameHigh = 1;
  assert.deepEqual(snapshot, expected);
  assert.equal(Object.hasOwn(snapshot, "arbitrary"), false);
  assert.equal(Object.hasOwn(snapshot, "report"), false);
  assert.equal(Object.hasOwn(snapshot, "clockMapping"), false);
  assert.equal(Object.hasOwn(snapshot, "trustedOutput"), false);
  assert.throws(() => { snapshot.expectedFrameHigh = 0; }, TypeError);
  assert.throws(() => { snapshot.clockMapping = 0; }, TypeError);
});
