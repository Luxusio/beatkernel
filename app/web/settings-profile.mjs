import { KEY_BINDINGS, snapshotBindings, timingFromMilliseconds, timingPolicyFromFields, audioOutputFromFields, audioLimitsFromFields, sectionFromSeconds } from "./play-model.mjs";

const MAX_BYTES = 16384;
const LANES = KEY_BINDINGS.map(([lane]) => lane).sort((a, b) => a - b);

function object(value, names) {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("Settings need a complete version 1 object.");
  const keys = Reflect.ownKeys(value);
  if (keys.length !== names.length || keys.some(key => !names.includes(key))) throw new Error("Settings contain missing or unknown fields.");
}
function strings(value, limits) {
  object(value, Object.keys(limits));
  const result = {};
  for (const [name, maximum] of Object.entries(limits)) {
    const field = value[name];
    if (typeof field !== "string" || field.length > maximum) throw new Error(`Settings ${name} must be a bounded string.`);
    result[name] = field;
  }
  return Object.freeze(result);
}

// Legacy retained Settings drafts own scalar strings and optional timing policy.
export function snapshotBrowserSettingsScalars(value) {
  object(value, ["timing", "output", "capacities", "section"]);
  const { timing: timingValue, output: outputValue, capacities: capacityValue, section: sectionValue } = value;
  const hasPolicy = timingValue && ["presetId", "rankPrecedence", "gauge"].some(name => Object.hasOwn(timingValue, name));
  const timing = strings(timingValue, { earlyMs: 21, lateMs: 21, offsetMs: 21,
    ...(hasPolicy ? { presetId: 96, rankPrecedence: 16, gauge: 16 } : {}) });
  if (hasPolicy) timingPolicyFromFields(timing.presetId, timing.rankPrecedence, timing.gauge);
  const output = strings(outputValue, { latency: 16, latencyMs: 21, rate: 10 });
  const capacities = strings(capacityValue, { queueCapacity: 5, maxVoices: 5, pendingCapacity: 5, maxFrames: 5, maxCommandsPerRender: 5 });
  const section = strings(sectionValue, { startSeconds: 20, endSeconds: 20 });
  timingFromMilliseconds(timing.earlyMs, timing.lateMs, timing.offsetMs);
  audioOutputFromFields(output.latency, output.latencyMs, output.rate);
  // The stored Custom draft remains meaningful when a category is selected.
  audioOutputFromFields("custom", output.latencyMs, output.rate);
  audioLimitsFromFields(capacities);
  sectionFromSeconds(section.startSeconds, section.endSeconds);
  return Object.freeze({ timing, output, capacities, section });
}

export function settingsMenuFields(value) {
  const isProfile = value && typeof value === "object"
    && ["kind", "version", "bindings"].some(name => Object.hasOwn(value, name));
  const { timing, output, capacities, section } = isProfile
    ? snapshotBrowserSettings(value) : snapshotBrowserSettingsScalars(value);
  return Object.freeze([
    timing.earlyMs, timing.lateMs, timing.offsetMs,
    output.latency, output.latencyMs, output.rate,
    capacities.queueCapacity, capacities.maxVoices, capacities.pendingCapacity,
    capacities.maxFrames, capacities.maxCommandsPerRender,
    section.startSeconds, section.endSeconds,
    timing.presetId ?? "", timing.rankPrecedence ?? "rank-first", timing.gauge ?? "beatkernel",
  ]);
}

export function settingsMenuSnapshot(fields) {
  if (!Array.isArray(fields) || ![13, 16].includes(fields.length)) {
    throw new Error("Browser settings draft requires thirteen or sixteen fields.");
  }
  return snapshotBrowserSettingsScalars({
    timing: { earlyMs: fields[0], lateMs: fields[1], offsetMs: fields[2],
      ...(fields.length === 16 ? { presetId: fields[13], rankPrecedence: fields[14], gauge: fields[15] } : {}) },
    output: { latency: fields[3], latencyMs: fields[4], rate: fields[5] },
    capacities: { queueCapacity: fields[6], maxVoices: fields[7], pendingCapacity: fields[8],
      maxFrames: fields[9], maxCommandsPerRender: fields[10] },
    section: { startSeconds: fields[11], endSeconds: fields[12] },
  });
}

export function settingsMenuProfileFields(value) {
  const { timing, output, capacities, section, bindings } = snapshotBrowserSettings(value);
  return Object.freeze([
    ...settingsMenuFields({ timing, output, capacities, section }),
    ...bindings.map(([, code]) => code),
  ]);
}

export function settingsMenuProfileSnapshot(fields) {
  if (!Array.isArray(fields) || fields.length !== 34) {
    throw new Error("Browser settings profile draft requires thirty-four fields.");
  }
  const scalars = settingsMenuSnapshot(fields.slice(0, 16));
  return snapshotBrowserSettings({
    kind: "beatkernel-browser-settings", version: 1, ...scalars,
    bindings: LANES.map((lane, index) => [lane, fields[16 + index]]),
  });
}

// Shared bounded DTO validation; only the Worker calls the file/JSON helpers.
export function snapshotBrowserSettings(value) {
  object(value, ["kind", "version", "timing", "output", "capacities", "section", "bindings"]);
  const { kind, version, timing: timingValue, output: outputValue, capacities: capacityValue,
    section: sectionValue, bindings: bindingValue } = value;
  if (kind !== "beatkernel-browser-settings" || version !== 1) throw new Error("Choose a version 1 BeatKernel browser settings file.");
  const { timing, output, capacities, section } = snapshotBrowserSettingsScalars({
    timing: timingValue, output: outputValue, capacities: capacityValue, section: sectionValue });
  if (!Array.isArray(bindingValue) || bindingValue.length !== LANES.length) throw new Error("Settings require all eighteen keyboard lanes.");
  const rows = [];
  for (let index = 0; index < LANES.length; index++) {
    const row = bindingValue[index];
    if (!Array.isArray(row) || row.length !== 2) throw new Error(`Settings binding index ${index} requires a lane and key-code pair.`);
    const lane = row[0], code = row[1];
    if (!LANES.includes(lane)) throw new Error(`Invalid settings keyboard lane at binding index ${index}.`);
    const context = `Settings binding index ${index}, lane ${lane.toString(16)}, menu field ${16 + LANES.indexOf(lane)}`;
    if (typeof code !== "string" || code.length > 32) throw new Error(`${context} must contain a bounded physical key code.`);
    rows.push(Object.freeze([lane, code]));
  }
  snapshotBindings(rows);
  rows.sort((a, b) => a[0] - b[0]);
  return Object.freeze({ kind, version, timing, output, capacities, section, bindings: Object.freeze(rows) });
}

export function encodeBrowserSettings(value) {
  const bytes = new TextEncoder().encode(JSON.stringify(snapshotBrowserSettings(value)));
  if (bytes.byteLength < 1 || bytes.byteLength > MAX_BYTES) throw new Error("Settings exceed 16 KiB.");
  return bytes;
}

export async function decodeBrowserSettings(file) {
  if (typeof File !== "function" || !(file instanceof File)) throw new Error("Choose an actual settings file.");
  const size = file.size;
  if (!Number.isSafeInteger(size) || size < 1 || size > MAX_BYTES) throw new Error("Choose one nonempty settings file no larger than 16 KiB.");
  const buffer = await file.arrayBuffer();
  if (!(buffer instanceof ArrayBuffer) || buffer.resizable || buffer.byteLength !== size || buffer.byteLength > MAX_BYTES) {
    throw new Error("Settings file size changed or exceeds 16 KiB.");
  }
  const text = new TextDecoder("utf-8", { fatal: true }).decode(buffer);
  return snapshotBrowserSettings(JSON.parse(text));
}
