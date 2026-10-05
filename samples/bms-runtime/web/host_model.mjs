// Shared import metadata and exact preview-time boundaries; no file reads.
export const LIMITS = Object.freeze({ files: 32768, file: 64 * 1024 * 1024, total: 256 * 1024 * 1024, path: 4096 });

export function snapshotFiles(files) {
  if (!files?.length || files.length > LIMITS.files) throw new Error(`Select between 1 and ${LIMITS.files} files.`);
  // File serialization does not promise browser-specific relative-path fields.
  return Array.from(files, file => ({ file, path: file.webkitRelativePath || file.name }));
}

export function normalizedPath(name, maxPathBytes = LIMITS.path) {
  if (typeof name !== "string" || new TextEncoder().encode(name).length > maxPathBytes) throw new Error("File path exceeds the import limit.");
  const slash = name.replaceAll("\\", "/");
  if (!slash || slash.startsWith("/") || slash.includes("\0")) throw new Error("File path must be relative.");
  const parts = slash.split("/");
  if (parts.includes("..")) throw new Error("Parent paths are not allowed.");
  const key = parts.filter(part => part && part !== ".").join("/");
  if (!key || /^[\x00-\x7f]:/.test(key)) throw new Error("File path must name a relative file.");
  return key;
}

export function preflight(files, limits = LIMITS) {
  if (!Array.isArray(files) || !files.length || files.length > limits.files) throw new Error(`Select between 1 and ${limits.files} files.`);
  let total = 0;
  const entries = files.map(({ file, path }) => {
    if (!(file instanceof File) || !Number.isSafeInteger(file.size) || file.size < 0 || file.size > limits.file) throw new Error(`Each selected file must be at most ${limits.file} bytes.`);
    total += file.size;
    if (!Number.isSafeInteger(total) || total > limits.total) throw new Error(`Selected files exceed the ${limits.total}-byte import limit.`);
    return { file, path: normalizedPath(path, limits.path) };
  });
  entries.sort((a, b) => a.path < b.path ? -1 : a.path > b.path ? 1 : 0);
  const keys = new Set(entries.map(entry => entry.path));
  if (keys.size !== entries.length) throw new Error("Selected files have duplicate normalized paths.");
  for (const { path } of entries) {
    let slash = path.indexOf("/");
    while (slash >= 0) {
      if (keys.has(path.slice(0, slash))) throw new Error("A selected path is both a file and a directory.");
      slash = path.indexOf("/", slash + 1);
    }
  }
  return entries;
}

export function seconds(ns) {
  const value = BigInt(ns);
  const magnitude = value < 0n ? -value : value;
  const fraction = (magnitude % 1000000000n).toString().padStart(9, "0").replace(/0+$/, "");
  return `${value < 0n ? "-" : ""}${magnitude / 1000000000n}${fraction ? `.${fraction}` : ""}`;
}

export function previewNanos(input) {
  if (typeof input !== "string" || !/^\d{1,19}$/.test(input)) throw new Error("Invalid preview nanoseconds.");
  const ns = BigInt(input);
  if (ns > 9223372034854775807n) throw new Error("Preview time exceeds the signed 64-bit range with lookahead.");
  return ns;
}

export function nanoseconds(input) {
  if (!/^\d{1,10}(?:\.\d{1,9})?$/.test(input)) throw new Error("Enter nonnegative seconds with up to nine decimal places.");
  const [whole, fraction = ""] = input.split(".");
  const ns = BigInt(whole) * 1000000000n + BigInt(fraction.padEnd(9, "0"));
  return previewNanos(ns.toString()).toString();
}

export function validateHistoricalGradeSnapshot(value) {
  if (!value || !Number.isSafeInteger(value.pages) || value.pages < 1 || value.pages > 1024
    || !Number.isSafeInteger(value.page) || value.page < 0 || value.page >= value.pages) {
    throw new Error("Historical grade page metadata is invalid.");
  }
  return { page: value.page, pages: value.pages };
}
export class HistoricalGradePager {
  #selection = null;
  #pending = null;
  #lastRpc = 0;
  bind(id, page, pages) {
    if (!Number.isSafeInteger(id) || id < 1) throw new Error("Historical grade selection identity is invalid.");
    const metadata = validateHistoricalGradeSnapshot({ page, pages });
    this.#selection = { id, ...metadata };
    this.#pending = null;
    this.#lastRpc = 0;
  }
  snapshot() { return this.#selection ? { ...this.#selection, pending: this.#pending !== null } : null; }
  request(page, rpcId) {
    if (!this.#selection || this.#pending) return null;
    validateHistoricalGradeSnapshot({ page, pages: this.#selection.pages });
    if (page === this.#selection.page) return null;
    if (!Number.isSafeInteger(rpcId) || rpcId < 1 || rpcId <= this.#lastRpc) throw new Error("Historical grade RPC identity must increase.");
    this.#pending = { page, rpcId };
    this.#lastRpc = rpcId;
    return { kind: "historical-record-page", id: this.#selection.id, rpcId, page };
  }
  accept(reply) {
    if (!this.#selection || !this.#pending || reply?.id !== this.#selection.id || reply?.rpcId !== this.#pending.rpcId) return false;
    if (reply.kind !== "historical-record-page-result") throw new Error("Historical grade receipt kind is invalid.");
    if (reply.error !== null) {
      if (typeof reply.error !== "string" || reply.error.length < 1 || reply.error.length > 4096 || reply.gradePage !== null || reply.gradePages !== 0) {
        throw new Error("Historical grade refusal is invalid.");
      }
    } else {
      const metadata = validateHistoricalGradeSnapshot({ page: reply.gradePage, pages: reply.gradePages });
      if (metadata.page !== this.#pending.page || metadata.pages !== this.#selection.pages) throw new Error("Historical grade receipt does not match its request.");
      this.#selection = { id: this.#selection.id, ...metadata };
    }
    this.#pending = null;
    return true;
  }
  cancel() { this.#pending = null; }
  clear() { this.#selection = null; this.#pending = null; this.#lastRpc = 0; }
}
