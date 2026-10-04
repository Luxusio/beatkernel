import { KEY_BINDINGS, snapshotBindings, timingFromMilliseconds, audioOutputFromFields, audioLimitsFromFields, sectionFromSeconds } from "./play-model.mjs";

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

// Shared bounded DTO validation; only the Worker calls the file/JSON helpers.
export function snapshotBrowserSettings(value) {
  object(value, ["kind", "version", "timing", "output", "capacities", "section", "bindings"]);
  const { kind, version, timing: timingValue, output: outputValue, capacities: capacityValue,
    section: sectionValue, bindings: bindingValue } = value;
  if (kind !== "beatkernel-browser-settings" || version !== 1) throw new Error("Choose a version 1 BeatKernel browser settings file.");
  const timing = strings(timingValue, { earlyMs: 21, lateMs: 21, offsetMs: 21 });
  const output = strings(outputValue, { latency: 16, latencyMs: 21, rate: 10 });
  const capacities = strings(capacityValue, { queueCapacity: 5, maxVoices: 5, pendingCapacity: 5, maxFrames: 5, maxCommandsPerRender: 5 });
  const section = strings(sectionValue, { startSeconds: 20, endSeconds: 20 });
  timingFromMilliseconds(timing.earlyMs, timing.lateMs, timing.offsetMs);
  audioOutputFromFields(output.latency, output.latencyMs, output.rate);
  // The stored Custom draft remains meaningful when a category is selected.
  audioOutputFromFields("custom", output.latencyMs, output.rate);
  audioLimitsFromFields(capacities);
  sectionFromSeconds(section.startSeconds, section.endSeconds);
  if (!Array.isArray(bindingValue) || bindingValue.length !== LANES.length) throw new Error("Settings require all eighteen keyboard lanes.");
  const rows = [];
  for (let index = 0; index < LANES.length; index++) {
    const row = bindingValue[index];
    if (!Array.isArray(row) || row.length !== 2) throw new Error("Settings bindings require lane and key-code pairs.");
    const lane = row[0], code = row[1];
    if (!LANES.includes(lane) || typeof code !== "string" || code.length > 32) throw new Error("Invalid settings keyboard binding.");
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
