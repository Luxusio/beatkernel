import { normalizedPath } from "./host_model.mjs";

const DATABASE = "beatkernel-records";
const STORES = ["metadata", "recordings"];
const MAX_RECORDS = 128;
const MAX_FILE = 64 * 1024 * 1024;
const MAX_TOTAL = 256 * 1024 * 1024;
const MAX_ARCHIVE = 5 * 1024 * 1024;
const MAX_SCORE = 18446744073709551615n;
const OWNER = Symbol("RecordsStore");

export class RecordStoreError extends Error {
  constructor(code, operation, message, cause) {
    super(message, cause === undefined ? undefined : { cause });
    this.name = "RecordStoreError";
    this.code = code;
    this.operation = operation;
  }
}

function storageError(operation, cause) {
  if (cause instanceof RecordStoreError) return cause;
  const code = cause?.name === "QuotaExceededError" ? "quota" : "storage";
  return new RecordStoreError(code, operation, `Recording ${operation} failed.`, cause);
}

function requireValue(condition, code, operation, message) {
  if (!condition) throw new RecordStoreError(code, operation, message);
}

function validId(value) {
  return Number.isSafeInteger(value) && value > 0;
}

function validBytes(value) {
  return value instanceof Uint8Array && value.buffer instanceof ArrayBuffer &&
    value.byteOffset === 0 && value.byteLength === value.buffer.byteLength &&
    value.byteLength > 0 && value.byteLength <= MAX_FILE;
}

function validArchiveBytes(value) {
  return value instanceof Uint8Array && value.buffer instanceof ArrayBuffer &&
    value.buffer.resizable !== true && value.byteOffset === 0 && value.byteLength === value.buffer.byteLength &&
    value.byteLength > 0 && value.byteLength <= MAX_ARCHIVE;
}

function validPlayer(value) {
  return Number.isInteger(value) && value >= 1 && value <= 0xffffffff;
}

function validName(value) {
  if (typeof value !== "string" || value.length === 0 || value.length > 512) return false;
  let length = 0;
  for (const _ of value) if (++length > 256) return false;
  return true;
}

function chartPath(value, code, operation) {
  requireValue(typeof value === "string" && value.length <= 4096,
    code, operation, "Recording chart path exceeds its limit.");
  try { return normalizedPath(value, 4096); }
  catch (cause) { throw new RecordStoreError(code, operation, "Recording chart path is invalid.", cause); }
}

function storedScore(value, operation) {
  requireValue(value === null || (typeof value === "bigint" && value >= 0n && value <= MAX_SCORE),
    "validation", operation, "Recording scores must be unsigned 64-bit integers or null.");
  return value === null ? null : value.toString();
}

function publicScore(value, operation) {
  if (value === null) return null;
  requireValue(typeof value === "string" && /^(0|[1-9][0-9]{0,19})$/.test(value),
    "corrupt", operation, "Stored recording score is not canonical.");
  const score = BigInt(value);
  requireValue(score <= MAX_SCORE, "corrupt", operation, "Stored recording score exceeds its limit.");
  return score;
}

function publicMetadata(value, operation) {
  requireValue(value !== null && typeof value === "object" && validId(value.id) &&
    validName(value.name) && typeof value.complete === "boolean" &&
    Number.isSafeInteger(value.createdAt) && value.createdAt >= 0 &&
    Number.isSafeInteger(value.byteLength) && value.byteLength > 0 && value.byteLength <= MAX_FILE,
  "corrupt", operation, "Stored recording metadata is invalid.");
  const path = chartPath(value.chartPath, "corrupt", operation);
  requireValue(path === value.chartPath, "corrupt", operation, "Stored recording path is not canonical.");
  const archived = value.archiveByteLength != null || value.archivePlayer != null;
  requireValue(!archived || (Number.isInteger(value.archiveByteLength) && value.archiveByteLength >= 1
    && value.archiveByteLength <= MAX_ARCHIVE && validPlayer(value.archivePlayer)),
  "corrupt", operation, "Stored archive association is invalid.");
  return {
    ...(archived ? { archiveByteLength: value.archiveByteLength, archivePlayer: value.archivePlayer } : {}),
    id: value.id, name: value.name, chartPath: path, complete: value.complete,
    hits: publicScore(value.hits, operation), misses: publicScore(value.misses, operation),
    combo: publicScore(value.combo, operation), createdAt: value.createdAt, byteLength: value.byteLength,
  };
}

function saveSnapshot(value) {
  const operation = "save";
  requireValue(value !== null && typeof value === "object", "validation", operation, "Recording is required.");
  const { bytes, name, complete, hits, misses, combo, completedArchive, archivePlayer } = value;
  const archived = completedArchive != null;
  requireValue(archived ? validArchiveBytes(completedArchive) && validPlayer(archivePlayer) : archivePlayer == null,
    "validation", operation, "Completed archive requires standalone bytes of at most 5 MiB and an original u32 player ID.");
  requireValue(validBytes(bytes), "validation", operation, "Recording bytes must be a nonempty standalone Uint8Array of at most 64 MiB.");
  requireValue(validName(name) && typeof complete === "boolean", "validation", operation, "Recording name or completion label is invalid.");
  const metadata = {
    ...(archived ? { archiveByteLength: completedArchive.byteLength, archivePlayer } : {}),
    name, chartPath: chartPath(value.chartPath, "validation", operation), complete,
    hits: storedScore(hits, operation), misses: storedScore(misses, operation),
    combo: storedScore(combo, operation), createdAt: Date.now(), byteLength: bytes.byteLength,
  };
  requireValue(Number.isSafeInteger(metadata.createdAt) && metadata.createdAt >= 0,
    "validation", operation, "Recording creation time is invalid.");
  // Validate all metadata before allocating the bounded private snapshot.
  const snapshot = new Uint8Array(bytes.byteLength);
  snapshot.set(bytes);
  if (!archived) return { metadata, bytes: snapshot };
  const archiveSnapshot = new Uint8Array(completedArchive.byteLength);
  archiveSnapshot.set(completedArchive);
  return { metadata, bytes: snapshot, completedArchive: archiveSnapshot, archivePlayer };
}

function validateSchema(db, transaction) {
  for (const name of STORES) {
    requireValue(db.objectStoreNames.contains(name), "corrupt", "open", "Recording database schema is incomplete.");
    const store = transaction.objectStore(name);
    requireValue(store.keyPath === "id" && store.autoIncrement === (name === "metadata"),
      "corrupt", "open", "Recording database schema is incompatible.");
  }
}

// A bounded cursor is also the capacity check inside the save transaction. No
// recording payload is acquired while listing or making that decision.
function scanMetadata(store, operation, watch, done) {
  const rows = [];
  let bytes = 0;
  watch(store.openCursor(), cursor => {
    if (!cursor) { done(rows, bytes); return; }
    requireValue(rows.length < MAX_RECORDS, "corrupt", operation, "Stored recording count exceeds its limit.");
    const row = publicMetadata(cursor.value, operation);
    requireValue(cursor.primaryKey === row.id && (!rows.length || rows[rows.length - 1].id < row.id),
      "corrupt", operation, "Stored recording key is inconsistent.");
    bytes += row.byteLength + (row.archiveByteLength ?? 0);
    requireValue(bytes <= MAX_TOTAL, "corrupt", operation, "Stored recording bytes exceed their limit.");
    rows.push(row);
    cursor.continue();
  });
}

export class RecordsStore {
  #db;
  #timeoutMs;
  #closed = false;
  #pending = new Set();

  constructor(token, db, timeoutMs) {
    if (token !== OWNER) throw new TypeError("Use RecordsStore.open().");
    this.#db = db;
    this.#timeoutMs = timeoutMs;
    db.onversionchange = () => this.#fence(new RecordStoreError("versionchange", "close", "Recording database version changed; reopen it explicitly."));
    db.onclose = () => this.#fence(new RecordStoreError("closed", "close", "Recording database connection closed unexpectedly."));
  }

  static async open(options = {}) {
    requireValue(options !== null && typeof options === "object", "validation", "open", "Recording store options are invalid.");
    const { timeoutMs = 10000, signal } = options;
    requireValue(Number.isSafeInteger(timeoutMs) && timeoutMs >= 1 && timeoutMs <= 60000,
      "validation", "open", "Recording store timeout must be between 1 and 60000 milliseconds.");
    requireValue(signal === undefined || (signal !== null && typeof signal.aborted === "boolean" &&
      typeof signal.addEventListener === "function" && typeof signal.removeEventListener === "function"),
    "validation", "open", "Recording store cancellation signal is invalid.");
    if (signal?.aborted) throw new RecordStoreError("aborted", "open", "Recording database opening was cancelled.");
    let factory;
    try { factory = options.factory === undefined ? globalThis.indexedDB : options.factory; }
    catch (cause) { throw new RecordStoreError("unavailable", "open", "IndexedDB is unavailable.", cause); }
    requireValue(factory && typeof factory.open === "function", "unavailable", "open", "IndexedDB is unavailable.");
    return new Promise((resolve, reject) => {
      let request;
      let openingConnection;
      let settled = false;
      let timer;
      const cleanupConnection = db => { try { db?.close(); } catch {} };
      const abortUpgrade = () => { try { request?.transaction?.abort(); } catch {} };
      const settle = (error, owner) => {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        signal?.removeEventListener("abort", abort);
        if (error) { abortUpgrade(); cleanupConnection(openingConnection); reject(error); }
        else resolve(owner);
      };
      const abort = () => settle(new RecordStoreError("aborted", "open", "Recording database opening was cancelled."));
      timer = setTimeout(() => settle(new RecordStoreError("timeout", "open", "Recording database opening timed out.")), timeoutMs);
      signal?.addEventListener("abort", abort, { once: true });
      if (signal?.aborted) { abort(); return; }
      try {
        request = factory.open(DATABASE, 1);
        request.onblocked = () => settle(new RecordStoreError("blocked", "open", "Recording database opening is blocked by another connection."));
        request.onerror = () => settle(storageError("open", request.error));
        request.onupgradeneeded = () => {
          if (settled) { abortUpgrade(); cleanupConnection(request.result); return; }
          try {
            const db = request.result;
            openingConnection = db;
            for (const name of STORES) {
              if (!db.objectStoreNames.contains(name)) db.createObjectStore(name, { keyPath: "id", autoIncrement: name === "metadata" });
            }
            validateSchema(db, request.transaction);
          } catch (cause) {
            settle(storageError("open", cause));
            cleanupConnection(request.result);
          }
        };
        request.onsuccess = () => {
          const db = request.result;
          openingConnection = db;
          if (settled) { cleanupConnection(db); return; }
          try {
            // Schema attributes are available synchronously; this readonly
            // inspection queues no requests and performs no mutation.
            requireValue(STORES.every(name => db.objectStoreNames.contains(name)),
              "corrupt", "open", "Recording database schema is incomplete.");
            validateSchema(db, db.transaction(STORES, "readonly"));
            settle(null, new RecordsStore(OWNER, db, timeoutMs));
          } catch (cause) { cleanupConnection(db); settle(storageError("open", cause)); }
        };
      } catch (cause) { settle(storageError("open", cause)); }
    });
  }

  get closed() { return this.#closed; }

  #ensureOpen(operation) {
    if (this.#closed) throw new RecordStoreError("closed", operation, "Recording database owner is closed.");
  }

  #fence(error) {
    if (this.#closed) return;
    this.#closed = true;
    this.#db.onversionchange = null;
    this.#db.onclose = null;
    for (const cancel of this.#pending) cancel(error);
    try { this.#db.close(); } catch {}
  }

  close() {
    this.#fence(new RecordStoreError("closed", "close", "Recording database owner was closed."));
  }

  #transaction(operation, stores, mode, start) {
    return new Promise((resolve, reject) => {
      let transaction;
      try {
        this.#ensureOpen(operation);
        transaction = this.#db.transaction(stores, mode);
      } catch (cause) { reject(storageError(operation, cause)); return; }
      let settled = false;
      let failure;
      let result;
      let ready = false;
      let timer;
      const settle = error => {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        this.#pending.delete(cancel);
        if (error) reject(error);
        else resolve(result);
      };
      const fail = cause => {
        if (settled) return;
        failure ??= storageError(operation, cause);
        try { transaction.abort(); } catch {}
      };
      const cancel = error => {
        failure ??= new RecordStoreError(error.code, operation, error.message, error);
        try { transaction.abort(); } catch {}
        settle(failure);
      };
      const watch = (request, success) => {
        request.onerror = () => fail(request.error);
        request.onsuccess = () => {
          if (settled || failure) return;
          try { success(request.result); } catch (cause) { fail(cause); }
        };
      };
      transaction.oncomplete = () => {
        // Request success is not a commit. Only this event can resolve.
        settle(failure ?? (ready ? undefined : new RecordStoreError("storage", operation, "Recording transaction completed without a result.")));
      };
      transaction.onabort = () => settle(failure ?? storageError(operation, transaction.error));
      transaction.onerror = event => fail(event.target?.error ?? transaction.error);
      this.#pending.add(cancel);
      timer = setTimeout(() => this.#fence(new RecordStoreError("timeout", operation,
        "Recording operation timed out; its commit status is unknown. Reopen and inspect the library.")), this.#timeoutMs);
      try { start(transaction, watch, value => { result = value; ready = true; }); }
      catch (cause) { fail(cause); }
    });
  }

  async list() {
    return this.#transaction("list", ["metadata"], "readonly", (transaction, watch, done) => {
      scanMetadata(transaction.objectStore("metadata"), "list", watch, rows => done(rows.reverse()));
    });
  }

  async save(value) {
    this.#ensureOpen("save");
    let snapshot;
    try { snapshot = saveSnapshot(value); } catch (cause) { throw storageError("save", cause); }
    return this.#transaction("save", STORES, "readwrite", (transaction, watch, done) => {
      const metadata = transaction.objectStore("metadata");
      scanMetadata(metadata, "save", watch, (rows, bytes) => {
        requireValue(rows.length < MAX_RECORDS && bytes + snapshot.metadata.byteLength + (snapshot.metadata.archiveByteLength ?? 0) <= MAX_TOTAL,
          "validation", "save", "Recording library capacity exceeded; delete a record explicitly before saving.");
        watch(metadata.add(snapshot.metadata), id => {
          requireValue(validId(id), "corrupt", "save", "Generated recording id is invalid.");
          const record = publicMetadata({ ...snapshot.metadata, id }, "save");
          const payload = { id, bytes: snapshot.bytes,
            ...(snapshot.completedArchive ? { completedArchive: snapshot.completedArchive, archivePlayer: snapshot.archivePlayer } : {}) };
          watch(transaction.objectStore("recordings").add(payload), () => done(record));
        });
      });
    });
  }

  async load(id) {
    this.#ensureOpen("load");
    requireValue(validId(id), "validation", "load", "Recording id must be a positive safe integer.");
    return this.#transaction("load", STORES, "readonly", (transaction, watch, done) => {
      watch(transaction.objectStore("metadata").get(id), stored => {
        requireValue(stored !== undefined, "not-found", "load", "Recording does not exist.");
        const metadata = publicMetadata(stored, "load");
        requireValue(metadata.id === id, "corrupt", "load", "Stored recording id is inconsistent.");
        watch(transaction.objectStore("recordings").get(id), payload => {
          requireValue(payload !== null && typeof payload === "object" && payload.id === id &&
            validBytes(payload.bytes) && payload.bytes.byteLength === metadata.byteLength,
          "corrupt", "load", "Stored recording payload is missing or invalid.");
          if (metadata.archiveByteLength !== undefined) {
            requireValue(validArchiveBytes(payload.completedArchive) && payload.completedArchive.byteLength === metadata.archiveByteLength
              && payload.archivePlayer === metadata.archivePlayer,
            "corrupt", "load", "Stored archive payload association is invalid.");
            done({ metadata, bytes: payload.bytes, completedArchive: payload.completedArchive, archivePlayer: payload.archivePlayer });
          } else {
            requireValue(payload.completedArchive == null && payload.archivePlayer == null,
              "corrupt", "load", "Stored archive payload has no matching metadata.");
            done({ metadata, bytes: payload.bytes });
          }
        });
      });
    });
  }

  async remove(id) {
    this.#ensureOpen("remove");
    requireValue(validId(id), "validation", "remove", "Recording id must be a positive safe integer.");
    return this.#transaction("remove", STORES, "readwrite", (transaction, watch, done) => {
      const metadata = transaction.objectStore("metadata");
      watch(metadata.get(id), stored => {
        const existed = stored !== undefined;
        // Explicit deletion can also remove a damaged or orphaned entry.
        watch(metadata.delete(id), () => {
          watch(transaction.objectStore("recordings").delete(id), () => done(existed));
        });
      });
    });
  }
}
