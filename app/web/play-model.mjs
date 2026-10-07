// Shared numeric boundaries. No clock acquisition or gameplay simulation.
import { nanoseconds } from "./host_model.mjs";

const I64_MAX = 9223372036854775807n;
const I64_MIN = -9223372036854775808n;
const U64_MAX = 18446744073709551615n;
const DEFAULT_TIMING = Object.freeze({ earlyNs: 50000000n, lateNs: 50000000n, offsetNs: 0n });
// Original samples cover base62; section preparation may add 4096 tails.
export const ORIGINAL_PCM_SAMPLES = 62 * 62;
export const PLAY_PCM_SAMPLES = ORIGINAL_PCM_SAMPLES + 4096;

export function startFromSeconds(value) {
  if (typeof value !== "string" || value.length > 20 || value.trim() !== value) throw new Error("Enter nonnegative start seconds with up to nine decimal places.");
  return BigInt(nanoseconds(value));
}
export function validateStart(value = 0n) {
  if (typeof value !== "bigint" || value < 0n || value > I64_MAX) throw new Error("Live section start must be nonnegative signed 64-bit nanoseconds.");
  return value;
}
export function validateEnd(startNs, endNs = undefined) {
  if (typeof startNs !== "bigint") throw new Error("A live section requires its actual start in nanoseconds.");
  validateStart(startNs);
  if (endNs !== undefined && (typeof endNs !== "bigint" || endNs <= startNs || endNs > I64_MAX)) {
    throw new Error("Live section end must follow its start and fit signed 64-bit nanoseconds.");
  }
  return endNs;
}
export function sectionFromSeconds(startValue, endValue) {
  const startNs = startFromSeconds(startValue);
  const endNs = validateEnd(startNs, endValue === "" ? undefined : startFromSeconds(endValue));
  return Object.freeze({ startNs, endNs });
}

export function replayOutputFromMetadata(startNs, endNs, endFrame, rate) {
  if (typeof startNs !== "bigint") throw new Error("Replay output requires its actual original-song start.");
  validateStart(startNs);
  if (!Number.isInteger(rate) || rate < 1 || rate > 0xffffffff) throw new Error("Replay output requires a positive unsigned 32-bit sample rate.");
  if (endNs === undefined && endFrame === undefined) return Object.freeze({ endNs: undefined, endFrame: undefined });
  if (typeof endNs !== "bigint" || endNs <= startNs || endNs > I64_MAX
    || typeof endFrame !== "bigint" || endFrame <= 0n || endFrame > U64_MAX) {
    throw new Error("Finite replay requires paired original-song end and output-frame metadata.");
  }
  const sampleRate = BigInt(rate);
  const expected = ((endNs - startNs + 100000000n) * sampleRate + 999999999n) / 1000000000n;
  if (endFrame !== expected || (endFrame * 1000000000n + sampleRate - 1n) / sampleRate > I64_MAX) {
    throw new Error("Finite replay endpoint differs from its output grid or exceeds signed nanoseconds.");
  }
  return Object.freeze({ endNs, endFrame });
}

// Decimal user settings use integer arithmetic, independently of host clocks.
export function parseTimingMilliseconds(value) {
  if (typeof value !== "string" || value.length > 21) throw new Error("Timing must be decimal milliseconds, at most 21 characters.");
  const match = /^([+-]?)(\d+)(?:\.(\d{1,6}))?$/.exec(value);
  if (!match || match[0] !== value) throw new Error("Timing needs integer milliseconds and at most six fractional digits, without spaces or exponents.");
  const magnitude = BigInt(match[2]) * 1000000n + BigInt((match[3] ?? "").padEnd(6, "0"));
  const ns = match[1] === "-" ? -magnitude : magnitude;
  if (ns < I64_MIN || ns > I64_MAX) throw new Error("Timing exceeds signed 64-bit nanoseconds.");
  return ns;
}
export function validateTiming(value = undefined) {
  if (value === undefined) return DEFAULT_TIMING;
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("Invalid live judge timing settings.");
  const { earlyNs, lateNs, offsetNs } = value;
  if (typeof earlyNs !== "bigint" || typeof lateNs !== "bigint" || typeof offsetNs !== "bigint"
    || earlyNs < 0n || earlyNs > I64_MAX || lateNs < 0n || lateNs > I64_MAX
    || offsetNs < I64_MIN || offsetNs > I64_MAX) throw new Error("Judge windows must be nonnegative and all timing values must fit signed 64-bit nanoseconds.");
  return Object.freeze({ earlyNs, lateNs, offsetNs });
}
export function timingFromMilliseconds(early, late, offset) {
  return validateTiming({ earlyNs: parseTimingMilliseconds(early), lateNs: parseTimingMilliseconds(late),
    offsetNs: parseTimingMilliseconds(offset) });
}

export function audioOutputFromFields(latency, ms, rate) {
  let latencyHint = latency;
  if (latency === "custom") {
    if (typeof ms !== "string" || ms.length > 21) throw new Error("Output latency must be unsigned decimal milliseconds, at most 21 characters.");
    const match = /^\d+(?:\.\d{1,6})?$/.exec(ms);
    if (!match || match[0] !== ms) throw new Error("Output latency needs unsigned decimal milliseconds with up to six fractional digits.");
    const ns = parseTimingMilliseconds(ms);
    if (ns > 60000000000n) throw new Error("Output latency must be between 0 and 60000 milliseconds.");
    latencyHint = Number(ns) / 1000000000;
  } else if (!["interactive", "balanced", "playback"].includes(latency)) {
    throw new Error("Choose a known output latency category or Custom.");
  }
  if (typeof rate !== "string" || rate.length > 10) throw new Error("Requested output rate must be a positive decimal integer, or blank for automatic selection.");
  if (rate === "") return Object.freeze({ latencyHint });
  const match = /^\d+$/.exec(rate);
  const sampleRate = Number(rate);
  if (!match || match[0] !== rate || !Number.isInteger(sampleRate) || sampleRate < 1 || sampleRate > 0xffffffff) {
    throw new Error("Requested output rate must be an unsigned integer from 1 to 4294967295 Hz.");
  }
  return Object.freeze({ latencyHint, sampleRate });
}

export function audioLimitsFromFields(fields) {
  const maxima = { queueCapacity: 65536, maxVoices: 4096, pendingCapacity: 4096, maxFrames: 4096, maxCommandsPerRender: 65536 };
  const names = Object.keys(maxima);
  if (!fields || typeof fields !== "object" || Array.isArray(fields)) throw new Error("Audio capacities require all five named settings.");
  const keys = Reflect.ownKeys(fields);
  if (keys.length !== names.length || keys.some(key => !names.includes(key))) throw new Error("Audio capacities require exactly the five known settings.");
  const limits = {};
  for (const name of names) {
    const value = fields[name];
    if (typeof value !== "string" || value.length < 1 || value.length > 5) throw new Error(`${name} must be an unsigned decimal integer of at most five digits.`);
    const match = /^\d+$/.exec(value);
    const capacity = Number(value);
    if (!match || match[0] !== value || !Number.isInteger(capacity) || capacity < 1 || capacity > maxima[name]) {
      throw new Error(`${name} must be an unsigned integer from 1 to ${maxima[name]}.`);
    }
    limits[name] = capacity;
  }
  return Object.freeze(limits);
}

export function millisecondsToNanos(value) {
  if (typeof value !== "number" || !Number.isFinite(value) || value < 0) throw new Error("Invalid Window performance timestamp.");
  const whole = Math.floor(value);
  if (!Number.isSafeInteger(whole)) throw new Error("Window performance timestamp exceeds integer precision.");
  const ns = BigInt(whole) * 1000000n + BigInt(Math.floor((value - whole) * 1000000));
  if (ns > I64_MAX) throw new Error("Window performance timestamp exceeds signed nanoseconds.");
  return ns;
}
export function frameNanos(frame, rate) {
  if (typeof frame !== "bigint" || frame < 0n || frame > U64_MAX
    || !Number.isInteger(rate) || rate < 1 || rate > 0xffffffff) throw new Error("Invalid audio frame grid.");
  const ns = frame * 1000000000n / BigInt(rate);
  if (ns > I64_MAX) throw new Error("Audio frame exceeds signed nanoseconds.");
  return ns;
}
export function audioScheduleFromFrame(contextFrame, startFrame, rate) {
  if (typeof contextFrame !== "bigint" || contextFrame < 0n || contextFrame > U64_MAX
    || typeof startFrame !== "bigint" || startFrame < 0n || startFrame > U64_MAX
    || !Number.isInteger(rate) || rate < 1 || rate > 0xffffffff) throw new Error("Invalid audio schedule frame grid.");
  const frame = contextFrame + BigInt(Math.ceil(rate / 50));
  if (frame > U64_MAX) throw new Error("Audio schedule frame overflow.");
  return frameNanos(frame > startFrame ? frame - startFrame : 0n, rate);
}
export function secondsToNanos(value) {
  if (typeof value !== "number" || !Number.isFinite(value) || value < 0) throw new Error("Invalid context timestamp.");
  const whole = Math.floor(value);
  if (!Number.isSafeInteger(whole)) throw new Error("Context timestamp exceeds integer precision.");
  const ns = BigInt(whole) * 1000000000n + BigInt(Math.floor((value - whole) * 1000000000));
  if (ns > I64_MAX) throw new Error("Context timestamp exceeds signed nanoseconds.");
  return ns;
}
function bracketedClock(clock) {
  if (!clock || ![clock.beforeMs, clock.contextTime, clock.afterMs].every(value => typeof value === "number" && Number.isFinite(value) && value >= 0)
    || clock.afterMs < clock.beforeMs) throw new Error("Invalid bracketed audio clock.");
  const contextNs = secondsToNanos(clock.contextTime);
  frameNanos(0n, clock.sampleRate);
  const before = millisecondsToNanos(clock.beforeMs);
  const after = millisecondsToNanos(clock.afterMs);
  return { contextNs, before, after, midpoint: before + (after - before) / 2n };
}
export function startProjection(clock, frame) {
  const { midpoint, contextNs } = bracketedClock(clock);
  const origin = midpoint + frameNanos(frame, clock.sampleRate) - contextNs;
  if (origin < 0n || origin > I64_MAX) throw new Error("Audio start projection exceeds Window time.");
  return origin;
}
// Translate a committed Window-clock target once to the actual output grid.
// The returned origin is shared by the audio arm and game activation.
export function committedStartProjection(clock, targetHostNs, nowMs, peerUncertaintyNs = 0n,
  minLeadNs = 100000000n, maxUncertaintyNs = 100000000n) {
  if (typeof targetHostNs !== "bigint" || targetHostNs < 0n || targetHostNs > I64_MAX
    || typeof peerUncertaintyNs !== "bigint" || peerUncertaintyNs < 0n || peerUncertaintyNs > U64_MAX
    || typeof minLeadNs !== "bigint" || minLeadNs < 1n || minLeadNs > I64_MAX
    || typeof maxUncertaintyNs !== "bigint" || maxUncertaintyNs < 1n || maxUncertaintyNs > I64_MAX) {
    throw new Error("Invalid committed multiplayer start bounds.");
  }
  const { contextNs, before, after, midpoint } = bracketedClock(clock);
  const now = millisecondsToNanos(nowMs);
  if (after > now || now - after > 100000000n) throw new Error("Multiplayer audio clock bracket is stale or in the future.");
  const uncertaintyNs = peerUncertaintyNs + (after - before + 1n) / 2n;
  if (uncertaintyNs > maxUncertaintyNs) throw new Error("Multiplayer start uncertainty exceeds policy.");
  if (targetHostNs - now - uncertaintyNs < minLeadNs) throw new Error("Committed multiplayer start has insufficient preparation lead.");
  const audioTarget = contextNs + targetHostNs - midpoint;
  if (audioTarget < 0n) throw new Error("Committed multiplayer start precedes the audio context.");
  const startFrame = (audioTarget * BigInt(clock.sampleRate) + 999999999n) / 1000000000n;
  const origin = startProjection(clock, startFrame);
  const roundingNs = origin - targetHostNs;
  if (roundingNs < 0n || roundingNs > (1000000000n + BigInt(clock.sampleRate) - 1n) / BigInt(clock.sampleRate)) {
    throw new Error("Multiplayer start projection escaped its output frame.");
  }
  return { startFrame, origin, uncertaintyNs, roundingNs };
}
// Actual reported presentation only; never extrapolate it to UI "now".
export function presentationPoint(timestamp, start, rate, nowMs, maxAgeMs = 1000) {
  return presentationPair(timestamp, start, rate, nowMs, maxAgeMs)?.outputNs ?? null;
}
// Keep the original host coordinate paired with the reported device position.
// Neither coordinate is acquired or advanced by this numeric boundary.
export function presentationPair(timestamp, start, rate, nowMs, maxAgeMs = 1000) {
  if (!timestamp || ![timestamp.contextTime, timestamp.performanceTime, nowMs, maxAgeMs]
    .every(value => typeof value === "number" && Number.isFinite(value) && value >= 0)) {
    throw new Error("Invalid audio presentation observation.");
  }
  // Validate the armed grid even when no output evidence is available yet.
  frameNanos(start, rate);
  if (timestamp.contextTime === 0 || timestamp.performanceTime === 0
    || timestamp.performanceTime > nowMs || nowMs - timestamp.performanceTime > maxAgeMs) return null;
  const contextNs = secondsToNanos(timestamp.contextTime);
  const startNs = (start * 1000000000n + BigInt(rate) - 1n) / BigInt(rate);
  const relative = contextNs - startNs;
  return relative < 0n ? null : { outputNs: relative, hostNs: millisecondsToNanos(timestamp.performanceTime) };
}
export function reportWord(words, index) {
  if (!(words instanceof Uint32Array) || words.length !== 56 || !Number.isInteger(index) || index < 0 || index >= 28) throw new Error("Invalid audio report words.");
  return BigInt(words[index * 2]) | BigInt(words[index * 2 + 1]) << 32n;
}
export function renderedCursor(report, start) {
  if (!report || typeof report.available !== "boolean") throw new Error("Invalid audio report.");
  const words = report.words;
  for (const index of [0, 5, 6, 11, 23, 25, 27]) {
    if (reportWord(words, index) > 1n) throw new Error("Invalid audio report flag.");
  }
  if (report.available !== (reportWord(words, 0) === 1n) || reportWord(words, 27) !== 0n) throw new Error("Audio report is terminal or inconsistent.");
  if (reportWord(words, 25) !== 1n || reportWord(words, 26) !== start) throw new Error("Audio report has a different armed start.");
  if (!report.available) return null;
  if (reportWord(words, 4) > reportWord(words, 2)) throw new Error("Invalid audio playback extent.");
  for (let index = 16; index <= 22; index++) {
    if (reportWord(words, index) !== 0n) throw new Error("Mixer rejected a gameplay audio command.");
  }
  const cursor = reportWord(words, 3) + reportWord(words, 4);
  if (cursor > U64_MAX) throw new Error("Audio playback cursor overflow.");
  return cursor;
}

// Explicit single-keyboard controls, independent of OS-specific key scan codes.
export const KEY_BINDINGS = Object.freeze([
  [0x16, "ShiftLeft", 1], [0x11, "KeyZ", 2], [0x12, "KeyS", 3], [0x13, "KeyX", 4],
  [0x14, "KeyD", 5], [0x15, "KeyC", 6], [0x18, "KeyF", 7], [0x19, "KeyV", 8], [0x17, "Space", 9],
  [0x26, "ShiftRight", 10], [0x21, "KeyN", 11], [0x22, "KeyJ", 12], [0x23, "KeyM", 13],
  [0x24, "KeyK", 14], [0x25, "Comma", 15], [0x28, "KeyL", 16], [0x29, "Period", 17], [0x27, "Slash", 18],
].map(row => Object.freeze(row)));

// W3C KeyboardEvent.code names; IDs are this browser app's source namespace.
// Keep all assigned IDs, including 19+, fixed; append future choices with new IDs.
export const KEY_CHOICES = Object.freeze([
  ...KEY_BINDINGS.map(([, code, id]) => [code, id]),
  ["KeyA", 19], ["KeyB", 20], ["KeyE", 21], ["KeyG", 22], ["KeyH", 23], ["KeyI", 24],
  ["KeyO", 25], ["KeyP", 26], ["KeyQ", 27], ["KeyR", 28], ["KeyT", 29], ["KeyU", 30], ["KeyW", 31], ["KeyY", 32],
  ["Digit0", 33], ["Digit1", 34], ["Digit2", 35], ["Digit3", 36], ["Digit4", 37],
  ["Digit5", 38], ["Digit6", 39], ["Digit7", 40], ["Digit8", 41], ["Digit9", 42],
  ["Backquote", 43], ["Minus", 44], ["Equal", 45], ["BracketLeft", 46], ["BracketRight", 47],
  ["Backslash", 48], ["Semicolon", 49], ["Quote", 50], ["IntlBackslash", 51], ["IntlRo", 52], ["IntlYen", 53],
  ["ArrowLeft", 54], ["ArrowDown", 55], ["ArrowUp", 56], ["ArrowRight", 57],
  ["Numpad0", 58], ["Numpad1", 59], ["Numpad2", 60], ["Numpad3", 61], ["Numpad4", 62],
  ["Numpad5", 63], ["Numpad6", 64], ["Numpad7", 65], ["Numpad8", 66], ["Numpad9", 67],
  ["NumpadDecimal", 68], ["NumpadComma", 69], ["NumpadAdd", 70], ["NumpadSubtract", 71],
  ["NumpadMultiply", 72], ["NumpadDivide", 73], ["NumpadEnter", 74], ["NumpadEqual", 75],
  ["ControlLeft", 76], ["ControlRight", 77], ["Enter", 78], ["Backspace", 79], ["Tab", 80],
].map(row => Object.freeze(row)));
const keyIds = new Map(KEY_CHOICES);
const knownLanes = new Set(KEY_BINDINGS.map(row => row[0]));

export function snapshotBindings(rows) {
  if (!Array.isArray(rows) || rows.length > 18) throw new Error("Keyboard bindings must contain at most 18 lane rows.");
  const lanes = new Set();
  const codes = new Set();
  const selected = [];
  for (const row of rows) {
    if (!Array.isArray(row) || row.length !== 2 || !knownLanes.has(row[0])
      || typeof row[1] !== "string" || row[1].length > 32 || lanes.has(row[0])) throw new Error("Keyboard binding rows need unique supported lanes and physical key codes.");
    const [lane, code] = row;
    lanes.add(lane);
    if (code === "") continue;
    const id = keyIds.get(code);
    if (id === undefined) throw new Error("Choose a supported physical key; Escape is reserved for Stop.");
    if (codes.has(code)) throw new Error(`Keyboard key ${code} is assigned to more than one lane.`);
    codes.add(code);
    selected.push(Object.freeze([lane, code, id]));
  }
  return Object.freeze(selected);
}

export function bindingsFor(lanes, selection = KEY_BINDINGS) {
  if (!(lanes instanceof Uint8Array) && !Array.isArray(lanes)) throw new Error("Invalid prepared lanes.");
  if (lanes.length > 18 || new Set(lanes).size !== lanes.length) throw new Error("Invalid prepared lane count.");
  let selected = selection;
  if (selection !== KEY_BINDINGS) {
    if (!Array.isArray(selection) || selection.length > 18) throw new Error("Invalid keyboard binding snapshot.");
    selected = snapshotBindings(Array.from(selection, row => {
      if (!Array.isArray(row) || row.length !== 3 || typeof row[1] !== "string" || row[1].length > 32
        || !keyIds.has(row[1]) || keyIds.get(row[1]) !== row[2]) throw new Error("Keyboard binding snapshot has an unknown physical key identity.");
      return [row[0], row[1]];
    }));
  }
  return Array.from(lanes, lane => {
    const binding = selected.find(row => row[0] === lane);
    if (!binding) throw new Error("Prepared lane has no keyboard mapping.");
    return binding;
  });
}
