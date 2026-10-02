// Shared numeric boundaries. No clock acquisition or gameplay simulation.
const I64_MAX = 9223372036854775807n;
const U64_MAX = 18446744073709551615n;

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
export function secondsToNanos(value) {
  if (typeof value !== "number" || !Number.isFinite(value) || value < 0) throw new Error("Invalid context timestamp.");
  const whole = Math.floor(value);
  if (!Number.isSafeInteger(whole)) throw new Error("Context timestamp exceeds integer precision.");
  const ns = BigInt(whole) * 1000000000n + BigInt(Math.floor((value - whole) * 1000000000));
  if (ns > I64_MAX) throw new Error("Context timestamp exceeds signed nanoseconds.");
  return ns;
}
export function startProjection(clock, frame) {
  if (!clock || ![clock.beforeMs, clock.contextTime, clock.afterMs].every(value => typeof value === "number" && Number.isFinite(value) && value >= 0)
    || clock.afterMs < clock.beforeMs) throw new Error("Invalid bracketed audio clock.");
  const contextNs = secondsToNanos(clock.contextTime);
  const targetNs = frameNanos(frame, clock.sampleRate);
  const before = millisecondsToNanos(clock.beforeMs);
  const after = millisecondsToNanos(clock.afterMs);
  const origin = before + (after - before) / 2n + targetNs - contextNs;
  if (origin < 0n || origin > I64_MAX) throw new Error("Audio start projection exceeds Window time.");
  return origin;
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
export function bindingsFor(lanes) {
  if (!(lanes instanceof Uint8Array) && !Array.isArray(lanes)) throw new Error("Invalid prepared lanes.");
  if (lanes.length > 18 || new Set(lanes).size !== lanes.length) throw new Error("Invalid prepared lane count.");
  return Array.from(lanes, lane => {
    const binding = KEY_BINDINGS.find(row => row[0] === lane);
    if (!binding) throw new Error("Prepared lane has no keyboard mapping.");
    return binding;
  });
}
