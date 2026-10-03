// Files are immutable handles here. Selection and snapshots never acquire bytes.
export const OPPONENT_LIMITS = Object.freeze({ count: 8, bytes: 64 * 1024 * 1024, labelBytes: 256, sourceKeyBytes: 1024 });
const encoder = new TextEncoder();
const I64_MIN = -9223372036854775808n;
const I64_MAX = 9223372036854775807n;
const U64_MAX = 18446744073709551615n;

function text(value, limit, field) {
  if (typeof value !== "string" || value.length === 0 || value.length > limit
    || /[\u0000-\u001f\u007f-\u009f]/u.test(value)) throw new TypeError(`Invalid opponent ${field}.`);
  for (const scalar of value) {
    const code = scalar.codePointAt(0);
    if (code >= 0xd800 && code <= 0xdfff) throw new TypeError(`Invalid opponent ${field} Unicode.`);
  }
  if (encoder.encode(value).byteLength > limit) throw new RangeError(`Opponent ${field} is too long.`);
  return value;
}

function descriptor(value) {
  if (!value || !(value.file instanceof File) || typeof value.own !== "boolean") throw new TypeError("An opponent needs an actual File and explicit Own/Other choice.");
  const size = value.file.size;
  if (!Number.isSafeInteger(size) || size < 1 || size > OPPONENT_LIMITS.bytes) throw new RangeError("Opponent recording must contain 1 byte to 64 MiB.");
  return Object.freeze({ file: value.file,
    sourceKey: text(value.sourceKey, OPPONENT_LIMITS.sourceKeyBytes, "source key"),
    own: value.own, label: text(value.label, OPPONENT_LIMITS.labelBytes, "label") });
}

export function validateSelections(value) {
  if (!Array.isArray(value) || value.length > OPPONENT_LIMITS.count) throw new RangeError("Choose at most eight saved opponents.");
  const keys = new Set();
  let bytes = 0;
  const entries = [];
  for (const entry of value) {
    const accepted = descriptor(entry);
    if (keys.has(accepted.sourceKey)) throw new Error("That recording is already selected as an opponent.");
    keys.add(accepted.sourceKey);
    bytes += accepted.file.size;
    if (bytes > OPPONENT_LIMITS.bytes) throw new RangeError("Selected opponent recordings exceed 64 MiB.");
    entries.push(accepted);
  }
  return Object.freeze(entries);
}

export class SavedOpponentSelection {
  #entries = Object.freeze([]);
  get size() { return this.#entries.length; }
  get byteLength() { return this.#entries.reduce((sum, entry) => sum + entry.file.size, 0); }
  add(value) {
    const next = validateSelections([...this.#entries, value]);
    this.#entries = next;
    return next[next.length - 1];
  }
  remove(sourceKey) {
    const index = this.#entries.findIndex(entry => entry.sourceKey === sourceKey);
    if (index < 0) return false;
    this.#entries = Object.freeze(this.#entries.filter((_, position) => position !== index));
    return true;
  }
  clear() { this.#entries = Object.freeze([]); }
  snapshot() { return Object.freeze(this.#entries.slice()); }
}

export function opponentLabel(filename) {
  if (typeof filename !== "string") throw new TypeError("A recording filename is required.");
  let label = "";
  let bytes = 0;
  for (const scalar of filename) {
    const code = scalar.codePointAt(0);
    const clean = code < 0x20 || (code >= 0x7f && code <= 0x9f) || (code >= 0xd800 && code <= 0xdfff) ? " " : scalar;
    const count = encoder.encode(clean).byteLength;
    if (bytes + count > OPPONENT_LIMITS.labelBytes) break;
    label += clean;
    bytes += count;
  }
  return label.trim() || "Saved replay";
}

export function validateOpponentSnapshot(value, expectedCount) {
  if (!Number.isInteger(expectedCount) || expectedCount < 1 || expectedCount > OPPONENT_LIMITS.count
    || !Array.isArray(value) || value.length !== expectedCount) throw new RangeError("Saved opponent result count changed.");
  const time = item => typeof item === "bigint" && item >= I64_MIN && item <= I64_MAX;
  const counter = item => typeof item === "bigint" && item >= 0n && item <= U64_MAX;
  return Object.freeze(Array.from(value, row => {
    if (!row || (row.kind !== "own" && row.kind !== "other") || !time(row.songNs)
      || (row.recordedUntilNs !== null && !time(row.recordedUntilNs))
      || !counter(row.hits) || !counter(row.misses) || !counter(row.combo) || !counter(row.maxCombo)
      || row.combo > row.maxCombo || row.maxCombo > row.hits) throw new TypeError("Invalid saved opponent result.");
    return Object.freeze({ kind: row.kind, label: text(row.label, OPPONENT_LIMITS.labelBytes, "label"),
      songNs: row.songNs, recordedUntilNs: row.recordedUntilNs,
      hits: row.hits, misses: row.misses, combo: row.combo, maxCombo: row.maxCombo });
  }));
}
