/**
 * Actual-browser integration probe; it never supplies gameplay/output evidence.
 * Run after generating both browser WASM packages:
 * PUPPETEER_MODULE=/path/to/puppeteer-core CHROMIUM=/path/to/chromium \
 *   node app/web/render-integration.browser.mjs
 * Optional: RENDER_INTEGRATION_OUT, RENDER_INTEGRATION_PORT, TLS_CERT, TLS_KEY,
 * MULTIPLAYER_SERVER (already-built binary), RENDER_ROOM_PORT/RENDER_ROOM_URL.
 * For the owned self-signed local room backend, set
 * RENDER_WEBTRANSPORT_DEVELOPER_MODE=1. This is local certificate bootstrap,
 * not production TLS acceptance. Chromium separately requires a system root:
 * https://chromium.googlesource.com/chromium/src/net/+/refs/heads/main/quic/quic_context.h
 * RENDER_INTEGRATION_CASE=local-touch repeats only that diagnostic case;
 * unrun full-acceptance requirements remain false and the process exits 1.
 * A nonzero exit preserves every unmet requirement in evidence.json. This is
 * executable development evidence, not a Harness review/QA receipt.
 */
import assert from "node:assert/strict";
import { createServer } from "node:https";
import { readFile, writeFile, mkdir, stat } from "node:fs/promises";
import { spawn, spawnSync } from "node:child_process";
import { X509Certificate, createHash } from "node:crypto";
import { createRequire } from "node:module";
import { resolve, dirname, extname, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const out = resolve(process.env.RENDER_INTEGRATION_OUT ?? resolve(root, "target/wf/browser-render-worker/integration-development-1"));
const port = Number(process.env.RENDER_INTEGRATION_PORT ?? 8101);
const selectedCase = process.env.RENDER_INTEGRATION_CASE ?? null;
const required = ["ready-preview", "surface-zero-resize", "genuine-completed", "history", "replay", "live-stalled-input-audio", "local-touch", "cumulative-after-ack", "terminal-capture-cleanup", "stale-owner", "room", "combined-results-room", "malformed-atomic", "cold-sequence-atomic", "mode-admission-atomic"];
const evidence = { kind: "development-browser-integration", selectedCase, required, checks: [], scenarios: [], ceiling: [], source: {}, workers: [] };
let browser, server, backend, tls, browserOptions;
const browsers = new Set();
const pages = new Set();
const delay = ms => new Promise(resolveDelay => setTimeout(resolveDelay, ms));
const check = (id, passed, detail) => evidence.checks.push({ id, passed, detail });

function wav() {
  const frames = 9600, b = Buffer.alloc(44 + frames * 2);
  b.write("RIFF"); b.writeUInt32LE(b.length - 8, 4); b.write("WAVEfmt ", 8);
  b.writeUInt32LE(16, 16); b.writeUInt16LE(1, 20); b.writeUInt16LE(1, 22);
  b.writeUInt32LE(48000, 24); b.writeUInt32LE(96000, 28); b.writeUInt16LE(2, 32); b.writeUInt16LE(16, 34);
  b.write("data", 36); b.writeUInt32LE(frames * 2, 40);
  for (let i = 0; i < frames; i++) b.writeInt16LE(Math.round(Math.sin(i * Math.PI * 880 / 48000) * 1200), 44 + i * 2);
  return b;
}

async function serve() {
  await mkdir(out, { recursive: true });
  await writeFile(resolve(out, "x.wav"), wav());
  await writeFile(resolve(out, "short.bms"), "#TITLE Integration short\n#BPM 120\n#TOTAL 320\n#WAV01 x.wav\n#00011:00010000\n");
  await writeFile(resolve(out, "long.bms"), "#TITLE Integration original input\n#BPM 120\n#TOTAL 320\n#WAV01 x.wav\n#00011:00010101\n#00211:01010101\n#00311:01\n");
  const cert = process.env.TLS_CERT ?? resolve(out, "cert.pem");
  const key = process.env.TLS_KEY ?? resolve(out, "key.pem");
  if (!process.env.TLS_CERT && !process.env.TLS_KEY) {
    const result = spawnSync("openssl", ["req", "-x509", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:prime256v1", "-nodes", "-keyout", key, "-out", cert, "-days", "1", "-subj", "/CN=localhost", "-addext", "subjectAltName=IP:127.0.0.1,DNS:localhost"], { encoding: "utf8" });
    if (result.status !== 0) throw new Error(`Cannot create owned TLS certificate: ${result.stderr}`);
  }
  const certificate = new X509Certificate(await readFile(cert));
  tls = { cert, key, spki: createHash("sha256").update(certificate.publicKey.export({ type: "spki", format: "der" })).digest("base64") };
  const mime = { ".js": "text/javascript", ".mjs": "text/javascript", ".wasm": "application/wasm", ".html": "text/html", ".css": "text/css" };
  server = createServer({ cert: await readFile(cert), key: await readFile(key) }, async (request, response) => {
    try {
      let path = resolve(root, `.${decodeURIComponent(new URL(request.url, "https://localhost").pathname)}`);
      const fromRoot = relative(root, path);
      if (fromRoot === ".." || fromRoot.startsWith(`..${sep}`)) throw new Error("Outside repository");
      if ((await stat(path)).isDirectory()) path = resolve(path, "index.html");
      response.writeHead(200, { "Content-Type": mime[extname(path)] ?? "application/octet-stream", "Cross-Origin-Opener-Policy": "same-origin", "Cross-Origin-Embedder-Policy": "require-corp", "Cache-Control": "no-store" });
      response.end(await readFile(path));
    } catch { response.writeHead(404); response.end(); }
  });
  await new Promise((resolveListen, reject) => { server.once("error", reject); server.listen(port, "127.0.0.1", resolveListen); });
}

// Observation only: preserve real Worker constructors, transfer lists, events,
// acquisitions and capture bytes. No protocol responses or timestamps are made.
function windowObservation() {
  const data = globalThis.__renderIntegration = { messages: [], statuses: [], input: [], captures: [], owners: [], worklets: [], geometry: [] };
  const summary = value => {
    const output = {};
    for (const key of ["kind", "id", "playId", "rpcId", "tickId", "renderId", "generation", "content", "geometryVersion", "mode", "message", "error", "reason", "page", "pages", "width", "height", "completed", "replayComplete", "hits", "misses", "combo", "pendingInputs", "ackSequence", "released", "watermark", "nowNs", "contextFrame", "result", "event", "completedResults", "roomResults"]) {
      if (value?.[key] !== undefined) {
        const x = value[key];
        output[key] = typeof x === "bigint" ? String(x) : x && typeof x === "object" ? JSON.parse(JSON.stringify(x, (k, v) => typeof v === "bigint" ? String(v) : ArrayBuffer.isView(v) ? { byteLength: v.byteLength } : v)) : x;
      }
    }
    if (value?.events) output.events = JSON.parse(JSON.stringify(value.events, (k, v) => typeof v === "bigint" ? String(v) : v));
    return output;
  };
  const ActualWorker = Worker;
  globalThis.Worker = class extends ActualWorker {
    constructor(url, options) {
      super(url, options);
      this.probeURL = String(url);
      data.owners.push(this);
      data.messages.push({ direction: "created", url: this.probeURL, at: performance.now() });
      this.addEventListener("message", event => {
        data.messages.push({ direction: "received", url: this.probeURL, at: performance.now(), ...summary(event.data) });
        const payload = event.data;
        if (payload?.kind === "render-geometry") data.geometry.push({ ...payload });
        if (payload?.replay instanceof Uint8Array) data.captures.push({ kind: payload.kind, playId: payload.playId, complete: payload.replayComplete, bytes: Array.from(payload.replay), archive: payload.completedArchive instanceof Uint8Array ? Array.from(payload.completedArchive) : null });
        for (const record of payload?.replays ?? []) if (record.replay instanceof Uint8Array) data.captures.push({ kind: payload.kind, playId: payload.playId, player: record.player, complete: record.replayComplete, bytes: Array.from(record.replay), archive: payload.completedArchive instanceof Uint8Array ? Array.from(payload.completedArchive) : null });
      });
    }
    postMessage(value, transfer) {
      data.messages.push({ direction: "sent", url: this.probeURL, at: performance.now(), ...summary(value) });
      return super.postMessage(value, transfer);
    }
    terminate() { data.messages.push({ direction: "terminated", url: this.probeURL, at: performance.now() }); return super.terminate(); }
  };
  const ActualWorklet = AudioWorkletNode;
  globalThis.AudioWorkletNode = class extends ActualWorklet {
    constructor(context, name, options) {
      super(context, name, options);
      data.worklets.push({ name, sampleRate: context.sampleRate, state: context.state });
      this.port.addEventListener("message", event => data.messages.push({ direction: "worklet", at: performance.now(), ...summary(event.data) }));
    }
  };
  const ActualContext = AudioContext;
  globalThis.AudioContext = class extends ActualContext {
    close() {
      return super.close().then(result => { data.messages.push({ direction: "audio-context-closed", at: performance.now(), state: this.state }); return result; });
    }
  };
  for (const kind of ["keydown", "keyup", "pointerdown", "pointerup", "pointermove"]) document.addEventListener(kind, event => data.input.push({ kind, trusted: event.isTrusted, timestamp: event.timeStamp, callbackAt: performance.now(), code: event.code, pointerType: event.pointerType, pointerId: event.pointerId, x: event.clientX, y: event.clientY }), true);
  addEventListener("DOMContentLoaded", () => new MutationObserver(() => data.statuses.push({ at: performance.now(), text: document.querySelector("#status")?.textContent })).observe(document.querySelector("#status"), { childList: true, subtree: true, characterData: true }));
}

function workerObservation() {
  if (globalThis.__renderPortProbe) return;
  const probe = globalThis.__renderPortProbe = { records: [], packets: [], hold: false, held: [], port: null };
  // Test-only reader for the small actual tap-note fixture. The wire field
  // order is render_wire.rs write_frame; unsupported comparison/room packets
  // are recorded as unparsed, never treated as successful state evidence.
  function frame(bytes) {
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    let at = 40;
    const read = (method, size) => { if (at + size > bytes.length) throw Error("truncated observed frame"); const value = view[method](at, true); at += size; return value; };
    const u8 = () => read("getUint8", 1), u32 = () => read("getUint32", 4), u64 = () => read("getBigUint64", 8), i64 = () => read("getBigInt64", 8);
    const skip = n => { if (at + n > bytes.length) throw Error("truncated observed frame"); at += n; };
    const optional64 = () => u8() ? String(i64()) : null;
    const page = u32(), lookahead = String(i64()), roomDisabled = u8(), members = [];
    const count = u32(); if (count > 4) throw Error("unsupported observed member count");
    for (let i = 0; i < count; i++) {
      const player = u32(), song = String(i64()), pressed = u32(), comparisonHeight = u32(); skip(3);
      let score = null, missOffset = null;
      if (u8()) { const hits = String(u64()); missOffset = at; const misses = String(u64()), combo = String(u64()), maxCombo = String(u64()); skip(64); optional64(); optional64(); optional64(); score = { hits, misses, combo, maxCombo }; }
      if (u8()) skip(17);
      if (u8()) {
        if (u32() !== 0) throw Error("ghost comparison frame outside small-fixture reader");
        if (u8()) { u8(); if (u8()) skip(40); }
      }
      const recentCount = u32(); if (recentCount > 128) throw Error("unsupported observed recent count");
      const recent = [];
      for (let j = 0; j < recentCount; j++) recent.push({ object: String(u64()), stage: u32(), customStage: u32(), outcome: u32(), grade: u32(), delta: String(i64()), at: String(i64()) });
      const lastMiss = optional64(), pages = [], pageCount = u32(); if (pageCount > 245) throw Error("unsupported observed progress count");
      for (let j = 0; j < pageCount; j++) {
        const index = u32(), valid = u32(), completed = u32(), packedOffset = at, states = [];
        if (valid > 4096) throw Error("unsupported observed note extent");
        for (let word = 0; word < 128; word++) { const bits = u64(); for (let bit = 0; bit < 32 && word * 32 + bit < valid; bit++) states.push(Number(bits >> BigInt(bit * 2) & 3n)); }
        pages.push({ index, valid, completed, packedOffset, states });
      }
      members.push({ player, song, pressed, comparisonHeight, score, missOffset, recent, lastMiss, pages });
    }
    if (u8()) throw Error("room frame outside small-fixture reader");
    if (at !== bytes.length) throw Error("observed frame trailing bytes");
    return { page, lookahead, roomDisabled, members };
  }
  function room(bytes) {
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    let at = 40;
    const read = (method, n) => { if (at + n > bytes.length) throw Error("truncated observed room"); const value = view[method](at, true); at += n; return value; };
    const u8 = () => read("getUint8", 1), u32 = () => read("getUint32", 4);
    const skip = n => { if (at + n > bytes.length) throw Error("truncated observed room"); at += n; };
    const text = bound => { const n = u32(); if (n > bound || at + n > bytes.length) throw Error("observed room text extent"); const value = new TextDecoder("utf-8", { fatal: true }).decode(bytes.subarray(at, at + n)); at += n; return value; };
    if (u8()) skip(8); skip(8); if (u8()) skip(1); if (u8()) skip(8);
    const hosts = u32(); if (hosts > 64) throw Error("observed room host extent");
    for (let i = 0; i < hosts; i++) { skip(9); const players = u32(); if (players > 64) throw Error("observed room player extent"); skip(players * 4); }
    const initialPage = u32(), count = u32(), pages = [];
    if (count > 1008) throw Error("observed room page extent");
    for (let i = 0; i < count; i++) {
      const status = u8(), page = u32(), pageCount = u32(), failed = u8(), heading = text(256), error = u8() ? text(4096) : null;
      const rows = u32(); if (rows > 4) throw Error("observed room row extent");
      for (let j = 0; j < rows; j++) { skip(12); if (u8()) skip(40); skip(1); text(128); text(128); text(128); }
      pages.push({ status, page, pageCount, failed, heading, error });
    }
    if (at !== bytes.length) throw Error("observed room trailing bytes");
    return { initialPage, pages };
  }
  const original = MessagePort.prototype.postMessage;
  const start = MessagePort.prototype.start;
  const observed = new WeakSet();
  const summarize = message => {
    const value = {};
    for (const key of ["kind", "mode", "operationId", "generation", "content", "sequence", "packetKind", "geometryVersion", "page", "width", "height", "status", "message"]) if (message?.[key] !== undefined) value[key] = typeof message[key] === "bigint" ? String(message[key]) : message[key];
    if (message?.packet instanceof Uint8Array) {
      const bytes = message.packet, h = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
      value.packet = { byteLength: bytes.length, kind: h.getUint16(6, true), generation: String(h.getBigUint64(8, true)), content: String(h.getBigUint64(16, true)), sequence: String(h.getBigUint64(24, true)) };
      if (value.packet.kind === 2) { try { value.frame = frame(bytes); } catch (error) { value.unparsedFrame = String(error); } }
      if (value.packet.kind === 6) { try { value.room = room(bytes); } catch (error) { value.unparsedRoom = String(error); } }
    }
    return value;
  };
  MessagePort.prototype.postMessage = function(message, transfer) {
    probe.records.push({ direction: "sent", at: performance.now(), ...summarize(message) });
    if (message?.kind === "state-ack") {
      probe.port = this;
      probe.identity = message;
      if (probe.hold) { probe.held.push([this, message, transfer]); return; }
    }
    return original.call(this, message, transfer);
  };
  MessagePort.prototype.start = function() {
    if (!observed.has(this)) {
      observed.add(this);
      this.addEventListener("message", event => {
        const message = event.data;
        if (message?.packet instanceof Uint8Array) probe.packets.push({ bytes: Array.from(message.packet), ...summarize(message) });
        probe.records.push({ direction: "received", at: performance.now(), ...summarize(message) });
      });
    }
    return start.call(this);
  };
  probe.release = () => { probe.hold = false; for (const [port, message, transfer] of probe.held.splice(0)) original.call(port, message, transfer); };
  probe.malformed = () => {
    if (!probe.identity || !probe.port) throw Error("No actual renderer identity observed");
    const { generation, content, sequence } = probe.identity;
    const original = probe.packets.findLast(x => x.packet.kind === 2 && x.packet.generation === String(generation) && x.frame?.members[0]?.pages.length);
    if (!original) throw Error("No original progress body for malformed final-page injection");
    const packet = Uint8Array.from(original.bytes), h = new DataView(packet.buffer), progress = original.frame.members[0].pages.at(-1);
    h.setBigUint64(24,sequence+100n,true);
    h.setBigUint64(original.frame.members[0].missOffset, 0n, true);
    const slot = progress.valid - 1, offset = progress.packedOffset + Math.floor(slot / 32) * 8, shift = BigInt(slot % 32 * 2);
    h.setBigUint64(offset, h.getBigUint64(offset,true) | (3n << shift), true);
    const operationId = 18446744073709551615n;
    probe.injection = { generation: String(generation), content: String(content), sequence: String(sequence + 100n), operationId: String(operationId), before: probe.records.length, originalSequence: original.packet.sequence, malformedFinalPackedSlot: slot, changedEarlierMisses: true };
    probe.port.dispatchEvent(new MessageEvent("message", { data: { kind: "packet", packet, generation, content, operationId } }));
    return probe.injection;
  };
  probe.atomic = async () => {
    const generation = String(probe.identity.generation);
    const original = probe.packets.filter(x => x.packet.generation === generation && [1, 2].includes(x.packet.kind));
    const candidateIndex = original.findLastIndex(x => x.frame?.members[0]?.pages.length);
    if (candidateIndex < 1 || candidateIndex + 1 >= original.length) throw Error("No original progress frame plus following frame for atomic proof");
    const { BrowserView } = await import(new URL("./pkg/beatkernel_bms_runtime.js", self.location.href).href);
    const canvas = new OffscreenCanvas(960, 720), binding = await BrowserView.create(canvas);
    const image = async () => {
      for (let attempt = 0; attempt < 4; attempt++) { binding.draw_visual(); if (!binding.needs_redraw()) break; await new Promise(resolveDraw => setTimeout(resolveDraw, 16)); }
      if (binding.needs_redraw()) throw Error("Atomic mirror did not submit GPU geometry");
      return Array.from(new Uint8Array(await (await canvas.convertToBlob({ type: "image/png" })).arrayBuffer()));
    };
    const imageHash = async bytes => Array.from(new Uint8Array(await crypto.subtle.digest("SHA-256", Uint8Array.from(bytes)))).map(byte => byte.toString(16).padStart(2, "0")).join("");
    try {
      for (const packet of original.slice(0, candidateIndex + 1)) binding.import_visual_packet(Uint8Array.from(packet.bytes), 16777216, 1024);
      const before = await image(), candidate = original[candidateIndex], corrupted = Uint8Array.from(candidate.bytes), h = new DataView(corrupted.buffer);
      const registration = original.find(packet => packet.packet.kind === 1);
      if (!registration || registration.packet.sequence !== "0") throw Error("No genuine sequence-zero cold registration for native mirror");
      const currentGeneration = BigInt(registration.packet.generation), rejectedGeneration = currentGeneration + 2n, replacementGeneration = currentGeneration + 1n;
      if (rejectedGeneration > 18446744073709551615n) throw Error("Observed generation cannot support bounded next-generation fixture");
      const coldBeforeHash = await imageHash(before), coldRefusals = [];
      for (const sequence of [1n, 18446744073709551615n]) {
        const invalidCold = Uint8Array.from(registration.bytes), header = new DataView(invalidCold.buffer);
        header.setBigUint64(8, rejectedGeneration, true); header.setBigUint64(24, sequence, true);
        let refusal = null;
        try { binding.import_visual_packet(invalidCold, 16777216, 1024); } catch (error) { refusal = String(error); }
        coldRefusals.push({ sequence: String(sequence), generation: String(rejectedGeneration), refusal, afterHash: await imageHash(await image()) });
      }
      const member = candidate.frame.members[0], progress = member.pages.at(-1);
      h.setBigUint64(24, BigInt(candidate.packet.sequence) + 100n, true);
      h.setBigUint64(member.missOffset, 0n, true);
      const lastSlot = progress.valid - 1, offset = progress.packedOffset + Math.floor(lastSlot / 32) * 8;
      const shift = BigInt(lastSlot % 32 * 2); h.setBigUint64(offset, h.getBigUint64(offset, true) | (3n << shift), true);
      let refusal = null;
      try { binding.import_visual_packet(corrupted, 16777216, 1024); } catch (error) { refusal = String(error); }
      const after = await image();
      const next = original[candidateIndex + 1];
      const applied = binding.import_visual_packet(Uint8Array.from(next.bytes), 16777216, 1024);
      const nextStateHash = await imageHash(await image());
      const replacement = Uint8Array.from(registration.bytes), replacementHeader = new DataView(replacement.buffer);
      replacementHeader.setBigUint64(8, replacementGeneration, true);
      const replacementApplied = binding.import_visual_packet(replacement, 16777216, 1024);
      let replacementNextApplied = null;
      // Replay genuine committed packets under the valid replacement identity.
      // No time, judgment, capture or completion evidence is constructed here.
      for (const observed of original.slice(0, candidateIndex + 2)) {
        if (observed.packet.kind !== 2) continue;
        const bytes = Uint8Array.from(observed.bytes); new DataView(bytes.buffer).setBigUint64(8, replacementGeneration, true);
        replacementNextApplied = binding.import_visual_packet(bytes, 16777216, 1024);
      }
      const coldSequence = { registration: registration.packet, coldBeforeHash, refusals: coldRefusals,
        originalNextApplied: String(applied), originalNextSequence: next.packet.sequence, nextStateHash,
        replacementGeneration: String(replacementGeneration), replacementApplied: String(replacementApplied),
        replacementNextApplied: String(replacementNextApplied), replacementStateHash: await imageHash(await image()) };
      return { before, after, refusal, candidate: candidate.packet, progress, next: next.packet, applied: String(applied), changedEarlierMisses: true, malformedFinalPackedSlot: lastSlot, coldSequence };
    } finally { binding.free(); }
  };
  probe.modeAdmission = async fixtures => {
    const { BrowserView } = await import(new URL("./pkg/beatkernel_bms_runtime.js", self.location.href).href);
    const cap = 16777216, diagnostics = 1024;
    const retarget = (observed, generation, sequence = null) => {
      const bytes = Uint8Array.from(observed.bytes), header = new DataView(bytes.buffer);
      header.setBigUint64(8, generation, true); header.setBigUint64(16, 9n, true);
      if (sequence !== null) header.setBigUint64(24, sequence, true);
      return bytes;
    };
    const imageHash = async (binding, canvas) => {
      for (let attempt = 0; attempt < 4; attempt++) {
        binding.draw_visual(); if (!binding.needs_redraw()) break;
        await new Promise(resolveDraw => setTimeout(resolveDraw, 16));
      }
      if (binding.needs_redraw()) throw Error("Mode admission mirror did not submit GPU geometry");
      const bytes = await (await canvas.convertToBlob({ type: "image/png" })).arrayBuffer();
      return Array.from(new Uint8Array(await crypto.subtle.digest("SHA-256", bytes))).map(byte => byte.toString(16).padStart(2, "0")).join("");
    };
    const cases = [
      { name: "two-member-live", packet: fixtures.local, mode: 1 },
      { name: "two-member-replay", packet: fixtures.local, mode: 3 },
      { name: "empty-local", packet: fixtures.preview, mode: 2 },
      { name: "nonempty-preview", packet: fixtures.solo, mode: 0 },
      { name: "unknown-mode", packet: fixtures.solo, mode: 4 },
      { name: "nonregistration-kind", packet: fixtures.previewState, mode: 0 },
    ];
    const refusals = [];
    for (const entry of cases) {
      const canvas = new OffscreenCanvas(960, 720), binding = await BrowserView.create(canvas);
      try {
        const baselineApplied = binding.import_visual_registration(retarget(fixtures.preview, 100n), cap, diagnostics, 0);
        binding.import_visual_packet(retarget(fixtures.previewState, 100n, 1n), cap, diagnostics);
        const beforeHash = await imageHash(binding, canvas);
        let refusal = null;
        try { binding.import_visual_registration(retarget(entry.packet, 200n), cap, diagnostics, entry.mode); }
        catch (error) { refusal = String(error); }
        const afterHash = await imageHash(binding, canvas);
        // Only envelope identities/sequence change; every chart and committed
        // visual body remains genuinely captured from the actual game owner.
        const originalNextApplied = binding.import_visual_packet(retarget(fixtures.previewState, 100n, 2n), cap, diagnostics);
        const recoveries = [];
        for (const generation of [199n, 200n, 201n]) {
          const registrationApplied = binding.import_visual_registration(retarget(fixtures.preview, generation), cap, diagnostics, 0);
          const nextApplied = binding.import_visual_packet(retarget(fixtures.previewState, generation, 1n), cap, diagnostics);
          recoveries.push({ generation: String(generation), registrationApplied: String(registrationApplied),
            nextApplied: String(nextApplied), imageHash: await imageHash(binding, canvas) });
        }
        refusals.push({ name: entry.name, mode: entry.mode, baselineApplied: String(baselineApplied), refusal,
          beforeHash, afterHash, originalNextApplied: String(originalNextApplied), recoveries });
      } finally { binding.free(); }
    }
    // Compare valid canonical P1 LOCAL against the preserved legacy local
    // painter using the very same original solo registration/frame bodies.
    const localCanvas = new OffscreenCanvas(960, 720), referenceCanvas = new OffscreenCanvas(960, 720);
    const local = await BrowserView.create(localCanvas);
    let reference = null;
    try {
      reference = await BrowserView.create(referenceCanvas);
      const localApplied = local.import_visual_registration(retarget(fixtures.solo, 300n), cap, diagnostics, 2);
      const frameApplied = local.import_visual_packet(retarget(fixtures.soloState, 300n), cap, diagnostics);
      reference.import_visual_packet(retarget(fixtures.solo, 300n), cap, diagnostics);
      reference.set_visual_local(true);
      reference.import_visual_packet(retarget(fixtures.soloState, 300n), cap, diagnostics);
      return { refusals, oneMemberLocal: { registrationApplied: String(localApplied), frameApplied: String(frameApplied),
        frameSequence: fixtures.soloState.packet.sequence, imageHash: await imageHash(local, localCanvas),
        legacyLocalImageHash: await imageHash(reference, referenceCanvas) },
        captured: Object.fromEntries(Object.entries(fixtures).map(([name, observed]) => [name, observed.packet])) };
    } finally { local.free(); reference?.free(); }
  };
  probe.modeRefusal = observed => {
    if (!probe.port || !probe.identity) throw Error("No actual renderer owner for incompatible-mode injection");
    const generation = probe.identity.generation + 100n, content = probe.identity.content;
    const packet = Uint8Array.from(observed.bytes), header = new DataView(packet.buffer);
    header.setBigUint64(8, generation, true); header.setBigUint64(16, content, true);
    const operationId = 18446744073709551615n;
    probe.port.dispatchEvent(new MessageEvent("message", { data: { kind: "packet", packet,
      mode: "live", generation, content, operationId } }));
    return { generation: String(generation), content: String(content), operationId: String(operationId),
      mode: "live", captured: observed.packet };
  };
  if (typeof WebTransport === "function") {
    const NativeTransport = WebTransport;
    globalThis.WebTransport = class extends NativeTransport {
      constructor(url, options) {
        super(url, options);
        this.ready.then(() => probe.records.push({ kind: "webtransport-ready", url: String(url) }), error => probe.records.push({ kind: "webtransport-ready-rejected", url: String(url), error: String(error), name: error.name, message: error.message, source: error.source, streamErrorCode: error.streamErrorCode }));
        this.closed.then(value => probe.records.push({ kind: "webtransport-closed", url: String(url), value }), error => probe.records.push({ kind: "webtransport-closed-rejected", url: String(url), error: String(error), name: error.name, message: error.message }));
      }
    };
  }
}

async function observedPage(name, instance = browser) {
  const page = await instance.newPage(); pages.add(page);
  page.probeWorkers = new Map(); page.probePending = [];
  page.on("workercreated", worker => {
    page.probeWorkers.set(worker.url(), worker);
    page.probePending.push(worker.evaluate(workerObservation).catch(error => ({ error: String(error) })));
  });
  page.on("pageerror", error => evidence.scenarios.push({ name, pageError: String(error) }));
  await page.setViewport({ width: 1200, height: 1000 });
  await page.evaluateOnNewDocument(windowObservation);
  await page.goto(`https://127.0.0.1:${port}/app/web/`);
  await page.waitForFunction(() => !document.querySelector("#files").disabled || document.querySelector("#status").classList.contains("error"), { timeout: 60000 });
  await Promise.all(page.probePending);
  assert.equal(await page.evaluate(() => document.querySelector("#files").disabled), false, await status(page));
  await (await page.$("#files")).uploadFile(resolve(out, "short.bms"), resolve(out, "long.bms"), resolve(out, "x.wav"));
  await page.waitForFunction(() => !document.querySelector("#prepare").disabled);
  return page;
}
const status = page => page.$eval("#status", element => element.textContent);
async function prepare(page, chart = "short.bms") {
  const previous = await page.evaluate(() => __renderIntegration.messages.filter(x => x.kind === "render-geometry").at(-1)?.generation ?? null);
  await page.select("#chart", chart); await page.click("#prepare");
  await page.waitForFunction(() => document.querySelector("#status").textContent.startsWith("Chart prepared") || document.querySelector("#status").classList.contains("error"));
  assert.match(await status(page), /^Chart prepared/);
  await page.waitForFunction(old => __renderIntegration.messages.some(x => x.kind === "render-geometry" && x.mode === "preview" && x.generation !== old), {}, previous);
}
async function snapshot(page, name) {
  const window = await page.evaluate(() => ({ messages: __renderIntegration.messages, input: __renderIntegration.input, statuses: __renderIntegration.statuses, worklets: __renderIntegration.worklets, captures: __renderIntegration.captures.map(({ bytes, archive, ...rest }) => ({ ...rest, byteLength: bytes.length, archiveByteLength: archive?.length ?? 0 })) }));
  const workers = [];
  for (const [url, worker] of page.probeWorkers) {
    if (page.pausedWorker === worker) { workers.push({ url, externallyPaused: true }); continue; }
    try { workers.push({ url, ...(await worker.evaluate(() => ({ records: __renderPortProbe?.records, held: __renderPortProbe?.held.length }))) }); }
    catch (error) { workers.push({ url, unavailable: String(error) }); }
  }
  evidence.scenarios.push({ name, status: await status(page), window, workers });
  await page.screenshot({ path: resolve(out, `${name}.png`), fullPage: true });
  await (await page.$("#canvas")).screenshot({ path: resolve(out, `${name}-canvas.png`) });
  return { window, workers };
}
async function scenario(name, body) {
  if (selectedCase && selectedCase !== name) return;
  const existing = new Set(pages);
  try { await body(); }
  catch (error) {
    evidence.scenarios.push({ name, failure: String(error), stack: error.stack }); check(name, false, String(error));
    for (const page of pages) if (!existing.has(page)) {
      await snapshot(page, `${name}-failure`).catch(snapshotError => evidence.ceiling.push(`Failure snapshot: ${snapshotError}`));
      await closePage(page).catch(closeError => evidence.ceiling.push(`Failure cleanup: ${closeError}`));
    }
  }
}
async function playing(page) {
  await page.waitForFunction(() => document.querySelector("#status").textContent.startsWith("Playing") || __renderIntegration.messages.some(x => x.direction === "received" && ["play-error", "fatal"].includes(x.kind)), { timeout: 30000 });
  assert.match(await status(page), /^Playing/);
}
async function stopped(page, timeout = 15000) {
  await page.waitForFunction(() => __renderIntegration.messages.some(x => x.direction === "received" && ["play-stopped", "play-error"].includes(x.kind)), { timeout });
  await page.waitForFunction(() => document.querySelector("#stop").disabled, { timeout });
}
async function closePage(page) {
  try { if (!(await page.$eval("#stop", x => x.disabled))) { await page.click("#stop"); await page.waitForFunction(() => document.querySelector("#stop").disabled, { timeout: 15000 }); } } catch {}
  await page.close(); pages.delete(page);
}

async function startRoomBackend() {
  if (process.env.RENDER_ROOM_URL) return process.env.RENDER_ROOM_URL;
  const executable = resolve(process.env.MULTIPLAYER_SERVER ?? resolve(root, "target/debug/beatkernel-bms-runtime"));
  await stat(executable);
  const roomPort = Number(process.env.RENDER_ROOM_PORT ?? port + 1);
  const args = ["serve-multiplayer", "--bind", `127.0.0.1:${roomPort}`, "--cert", tls.cert, "--key", tls.key,
    "--origin", `https://127.0.0.1:${port}`, "--group-hosts", "2"];
  backend = spawn(executable, args, { stdio: ["ignore", "pipe", "pipe"] });
  evidence.backend = { executable, args, pid: backend.pid, diagnostics: "" };
  for (const stream of [backend.stdout, backend.stderr]) stream.on("data", bytes => { evidence.backend.diagnostics = (evidence.backend.diagnostics + bytes.toString()).slice(-16384); });
  await Promise.race([
    delay(200),
    new Promise((_, reject) => { backend.once("error", reject); backend.once("exit", code => reject(Error(`Room backend exited ${code}: ${evidence.backend.diagnostics}`))); }),
  ]);
  return `https://127.0.0.1:${roomPort}/rooms/render_integration`;
}

async function roomClients(puppeteer, roomURL, chart, name) {
  // Independent browser processes preserve the application's genuine blur /
  // visibility stop policy. No page focus or input policy is overridden.
  const peerBrowser = await puppeteer.launch({ ...browserOptions, args: [...browserOptions.args, `--log-net-log=${resolve(out, `network-peer-${browsers.size}.json`)}`] }); browsers.add(peerBrowser);
  evidence.workers.push({ ownedPeerBrowserPid: peerBrowser.process().pid, scenario: name });
  const first = await observedPage(`${name}-first`), second = await observedPage(`${name}-second`, peerBrowser);
  for (const page of [first, second]) {
    await prepare(page, chart); await page.click("#record"); await page.click("#multiplayer");
    await page.select("#multiplayer-mode", "room");
    await page.$eval("#multiplayer-url", (x, url) => { x.value = url; x.dispatchEvent(new Event("input", { bubbles: true })); }, roomURL);
    await page.click("#play");
  }
  await first.waitForFunction(() => !document.querySelector("#room-seal").disabled || __renderIntegration.messages.some(x => x.kind === "play-error"), { timeout: 20000 });
  assert.equal(await first.$eval("#room-seal", x => x.disabled), false, await status(first));
  await first.click("#room-seal");
  for (const page of [first, second]) {
    await page.waitForFunction(() => !document.querySelector("#room-ready").disabled || __renderIntegration.messages.some(x => x.kind === "play-error"), { timeout: 15000 });
    assert.equal(await page.$eval("#room-ready", x => x.disabled), false, await status(page));
    await page.click("#room-ready");
  }
  await Promise.all([playing(first), playing(second)]);
  return [first, second];
}

async function main() {
  const require = createRequire(import.meta.url);
  const puppeteer = process.env.PUPPETEER_MODULE ? require(resolve(process.env.PUPPETEER_MODULE)) : (await import("puppeteer-core")).default;
  await serve();
  evidence.source = { root, command: "node app/web/render-integration.browser.mjs", main: "app/web/main.js", game: "app/web/worker.js", renderer: "app/web/renderer-worker.js", actualWorklet: "app/web/audio-worklet.js" };
  browserOptions = { executablePath: process.env.CHROMIUM ?? "/usr/bin/chromium", headless: true, args: ["--no-sandbox", "--disable-dev-shm-usage", "--enable-unsafe-webgpu", "--use-angle=swiftshader", "--enable-features=Vulkan", "--use-vulkan=swiftshader", "--ignore-certificate-errors", `--ignore-certificate-errors-spki-list=${tls.spki}`, "--autoplay-policy=no-user-gesture-required"] };
  if (process.env.RENDER_WEBTRANSPORT_DEVELOPER_MODE === "1") browserOptions.args.push("--webtransport-developer-mode");
  evidence.source.certificateTrust = { generatedLocalCertificate: !process.env.TLS_CERT, spkiPinned: true, webtransportDeveloperMode: process.env.RENDER_WEBTRANSPORT_DEVELOPER_MODE === "1", productionTLSAcceptance: false };
  browser = await puppeteer.launch({ ...browserOptions, args: [...browserOptions.args, `--log-net-log=${resolve(out, "network-main.json")}`] }); browsers.add(browser);
  console.log(`Owned browser PID ${browser.process().pid}; evidence ${out}`);
  await scenario("ready-preview", async () => {
    const page = await observedPage("preview"); await prepare(page);
    const info = await snapshot(page, "ready-preview");
    assert.equal(page.probeWorkers.size, 2); assert(await page.evaluate(() => crossOriginIsolated));
    check("ready-preview", true, { workers: [...page.probeWorkers.keys()], geometry: info.window.messages.filter(x => x.kind === "render-geometry") });
    const renderer = [...page.probeWorkers.values()].find(x => x.url().endsWith("/renderer-worker.js"));
    await page.evaluate(() => { document.querySelector("#viewport").style.width = "0px"; document.querySelector("#viewport").style.height = "0px"; });
    await delay(200);
    const zero = await renderer.evaluate(() => __renderPortProbe.records.slice(-12));
    assert(zero.some(x => x.kind === "render-wait"));
    const zeroResize = zero.findLast(x => x.direction === "received" && x.kind === "resize" && x.width === 0 && x.height === 0);
    assert(zeroResize, "Zero surface was not admitted by the actual renderer");
    assert(zero.some(x => x.kind === "control-ack" && x.geometryVersion === zeroResize.geometryVersion));
    assert(!zero.some(x => x.kind === "geometry-ack" && x.geometryVersion === zeroResize.geometryVersion), "Zero surface claimed successful geometry submission");
    await page.evaluate(() => { document.querySelector("#viewport").style.width = "640px"; document.querySelector("#viewport").style.height = "480px"; });
    await delay(250); const resized = await snapshot(page, "surface-zero-resize");
    assert(resized.window.messages.some(x => x.kind === "render-geometry" && x.width === 640 && x.height === 480));
    check("surface-zero-resize", true, { zero, geometry: resized.window.messages.filter(x => x.kind === "render-geometry").slice(-2) });
    const previous = await page.evaluate(() => __renderIntegration.messages.filter(x => x.kind === "render-geometry").at(-1));
    await prepare(page, "long.bms");
    const changed = await page.evaluate(() => __renderIntegration.messages.filter(x => x.kind === "render-geometry").at(-1));
    assert.notEqual(changed.generation, previous.generation);
    await page.evaluate(old => {
      for (const key of ["generation", "content", "geometryVersion"]) old[key] = BigInt(old[key]);
      __renderIntegration.owners.find(x => x.probeURL.endsWith("/worker.js")).dispatchEvent(new MessageEvent("message", { data: old }));
    }, previous);
    await delay(50); assert.match(await status(page), /^Chart prepared/);
    check("stale-owner", true, { previous, current: changed, replayedOriginalOldGeometry: true });
    await closePage(page);
  });
  await scenario("genuine-completed", async () => {
    const page = await observedPage("completed"); await prepare(page);
    await page.click("#record"); await page.click("#play"); await playing(page);
    await stopped(page, 20000); const info = await snapshot(page, "genuine-completed");
    const terminal = info.window.messages.find(x => x.kind === "play-stopped");
    assert(terminal?.replayComplete, `No genuine natural completion: ${await status(page)}`);
    assert(terminal.completedResults?.proof, "No genuine completed Results owner");
    check("genuine-completed", true, terminal);
    const captures = await page.evaluate(() => __renderIntegration.captures);
    const complete = captures.find(x => x.complete); assert(complete?.archive?.length);
    await writeFile(resolve(out, "genuine-complete.bkr"), Uint8Array.from(complete.bytes));
    await writeFile(resolve(out, "genuine-complete.bkr.bkresult"), Uint8Array.from(complete.archive));
    await page.waitForFunction(() => !document.querySelector("#records-save").disabled);
    await page.click("#records-save"); await page.waitForFunction(() => document.querySelector("#records").value !== "" && !document.querySelector("#records-use").disabled);
    await prepare(page, "short.bms"); // Ordinary preview navigation retires the completed presentation.
    await page.click("#records-use"); await page.waitForFunction(() => document.querySelector("#status").textContent.includes("Stored historical result displayed") || document.querySelector("#status").classList.contains("error"));
    assert.match(await status(page), /Stored historical result displayed/);
    const historical = await snapshot(page, "history");
    assert(historical.workers.find(x => x.url.endsWith("/renderer-worker.js"))?.records.some(x => x.direction === "received" && x.packet?.kind === 4));
    check("history", true, "Actual saved original completion archive through main.js RecordsStore UI and native HISTORY packet");
    await page.click("#replay-play"); await playing(page); await stopped(page);
    const replay = await snapshot(page, "replay");
    const replayStart = replay.window.messages.findLast(x => x.direction === "sent" && x.kind === "play-start" && x.mode === "replay");
    assert(replayStart && replayStart.playId !== terminal.playId);
    assert(replay.window.messages.some(x => x.direction === "received" && x.kind === "play-stopped" && x.playId === replayStart.playId));
    assert(replay.window.messages.some(x => x.kind === "play-render-done" && x.playId === replayStart.playId));
    check("replay", true, "Actual captured original replay through main.js and AudioWorklet, correlated to its new play owner");
    await closePage(page);
  });
  await scenario("local-touch", async () => {
    const page = await observedPage("local"); await prepare(page, "long.bms");
    await page.click("#record"); await page.click("#touch-input");
    await page.$eval("#local-count", x => { x.closest("details").open = true; x.value = "2"; x.dispatchEvent(new Event("change", { bubbles: true })); });
    await page.waitForFunction(() => !document.querySelector("#local-discover").disabled);
    await page.click("#local-discover");
    await page.waitForFunction(() => document.querySelector("#local-source-1")?.options.length >= 3 && !document.querySelector("#local-source-1").disabled);
    await page.select("#local-source-1", "1"); await page.select("#local-source-2", "2");
    await page.click("#play"); await playing(page);
    await page.keyboard.down("z"); await page.keyboard.up("z");
    await page.$eval("#canvas", x => x.scrollIntoView({ block: "center" }));
    const input = await page.createCDPSession();
    const bounds = await page.$eval("#canvas", x => { const b = x.getBoundingClientRect(); return { x: b.x + b.width * .7, y: b.y + b.height * .7 }; });
    await input.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ ...bounds, id: 2 }] });
    await input.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] }); await input.detach();
    await delay(150); const info = await snapshot(page, "local-touch");
    const originalFailure = info.window.statuses.find(x => /Playback failed:/.test(x.text));
    assert.equal(await page.$eval("#stop", x => x.disabled), false, originalFailure?.text ?? "Local owner stopped during original input");
    assert(!info.window.messages.some(x => x.kind === "play-error"));
    assert(info.window.messages.some(x => x.kind === "render-geometry" && x.mode === "local" && x.page === 0));
    const submitted = info.window.messages.filter(x => x.kind === "play-step" && x.events?.length).flatMap(x => x.events);
    assert(submitted.some(x => x.kind === "touch" && x.page === 0));
    assert(submitted.some(x => typeof x.key === "number" && typeof x.down === "boolean"));
    await page.click("#stop"); await stopped(page);
    const captures = await page.evaluate(() => __renderIntegration.captures.map(x => ({ player: x.player, bytes: x.bytes.length, complete: x.complete })));
    assert.equal(new Set(captures.map(x => x.player)).size, 2);
    check("local-touch", true, { submitted, captures, originalAcquisitions: info.window.input.filter(x => x.trusted && (x.pointerType === "touch" || x.kind === "keydown")) });
    await closePage(page);
  });
  await scenario("mode-admission-atomic", async () => {
    const page = await observedPage("mode-admission"); await prepare(page, "long.bms");
    const renderer = [...page.probeWorkers.values()].find(x => x.url().endsWith("/renderer-worker.js"));
    const capture = async (mode, stateKind) => renderer.evaluate(async ({ mode, stateKind }) => {
      for (let attempt = 0; attempt < 250; attempt++) {
        const cold = __renderPortProbe.packets.findLast(x => x.packet.kind === 1 && x.mode === mode);
        const state = __renderPortProbe.packets.findLast(x => x.packet.kind === stateKind && x.packet.generation === cold?.packet.generation);
        if (cold && state) return { cold, state };
        await new Promise(resolvePacket => setTimeout(resolvePacket, 20));
      }
      throw Error(`No genuine ${mode} registration/state for mode admission`);
    }, { mode, stateKind });
    const preview = await capture("preview", 3);
    await page.click("#play"); await playing(page);
    const solo = await capture("live", 2);
    await page.click("#stop"); await stopped(page);
    await prepare(page, "long.bms");
    await page.click("#touch-input");
    await page.$eval("#local-count", x => { x.closest("details").open = true; x.value = "2"; x.dispatchEvent(new Event("change", { bubbles: true })); });
    await page.waitForFunction(() => !document.querySelector("#local-discover").disabled);
    await page.click("#local-discover");
    await page.waitForFunction(() => document.querySelector("#local-source-1")?.options.length >= 3 && !document.querySelector("#local-source-1").disabled);
    await page.select("#local-source-1", "1"); await page.select("#local-source-2", "2");
    await page.click("#play"); await playing(page);
    const local = (await capture("local", 2)).cold;
    await page.click("#stop"); await stopped(page);
    const fixtures = { preview: preview.cold, previewState: preview.state, solo: solo.cold, soloState: solo.state, local };
    const admission = await renderer.evaluate(values => __renderPortProbe.modeAdmission(values), fixtures);
    assert.equal(admission.refusals.length, 6);
    for (const refused of admission.refusals) {
      assert(refused.refusal, `Generated WASM accepted ${refused.name}`);
      assert.equal(refused.baselineApplied, "0");
      assert.equal(refused.afterHash, refused.beforeHash, `${refused.name} changed native presentation before refusal`);
      assert.equal(refused.originalNextApplied, "2", `${refused.name} changed prior identity or sequence admission`);
      assert.deepEqual(refused.recoveries.map(x => x.generation), ["199", "200", "201"]);
      for (const recovery of refused.recoveries) {
        assert.equal(recovery.registrationApplied, "0", `${refused.name} fenced valid recovery ${recovery.generation}`);
        assert.equal(recovery.nextApplied, "1");
        assert.equal(recovery.imageHash, refused.beforeHash, "Valid recovery did not retain original captured preview state");
      }
    }
    assert.equal(admission.oneMemberLocal.registrationApplied, "0");
    assert.equal(admission.oneMemberLocal.frameApplied, admission.oneMemberLocal.frameSequence);
    assert.equal(admission.oneMemberLocal.imageHash, admission.oneMemberLocal.legacyLocalImageHash,
      "Explicit canonical P1 LOCAL differs from the preserved native local painter");
    const injection = await renderer.evaluate(observed => __renderPortProbe.modeRefusal(observed), local);
    await delay(100);
    const records = await renderer.evaluate(() => __renderPortProbe.records);
    assert(records.some(x => x.kind === "render-error" && x.operationId === injection.operationId
      && x.generation === injection.generation && x.content === injection.content), "Actual renderer did not correlate incompatible mode refusal");
    assert(!records.some(x => x.kind === "state-ack" && x.operationId === injection.operationId), "Actual renderer ACKed incompatible two-member live mode");
    check("mode-admission-atomic", true, { ...admission, actualRendererRefusal: injection,
      scope: "Fresh generated BrowserView and production renderer; captured original preview/solo/two-member local bodies, GPU preservation and sequence/generation recovery; no constructed gameplay/completion authority" });
    await snapshot(page, "mode-admission-atomic"); await closePage(page);
  });
  await scenario("live-stalled-input-audio", async () => {
    const page = await observedPage("stall"); await prepare(page, "long.bms");
    await page.click("#record"); await page.click("#touch-input"); await page.click("#play"); await playing(page);
    const renderer = [...page.probeWorkers.values()].find(x => x.url().endsWith("/renderer-worker.js"));
    const initial = await snapshot(page, "stall-before");
    await renderer.client.send("Debugger.enable");
    const paused = new Promise(resolvePause => renderer.client.once("Debugger.paused", resolvePause));
    await renderer.client.send("Debugger.pause");
    await Promise.race([paused, delay(5000).then(() => { throw Error("Renderer did not actually pause"); })]);
    page.pausedWorker = renderer;
    await page.keyboard.down("z"); await page.keyboard.up("z");
    const input = await page.createCDPSession(); const bounds = await page.$eval("#canvas", x => { const b=x.getBoundingClientRect(); return { x:b.x+b.width/2, y:b.y+b.height/2 }; });
    await input.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ ...bounds, id: 1 }] });
    await input.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] }); await input.detach();
    await delay(300); const stalled = await snapshot(page, "stall-input-audio");
    const count = data => data.window.messages.filter(x => x.direction === "received" && x.kind === "play-render-done").length;
    assert(count(stalled) > count(initial), `No continued original audio ACK: ${await status(page)}`);
    assert(stalled.window.input.some(x => x.trusted && x.kind === "keydown"));
    assert(stalled.window.input.some(x => x.trusted && x.pointerType === "touch"));
    assert(stalled.window.messages.some(x => x.direction === "received" && x.kind === "play-step-done"));
    check("live-stalled-input-audio", true, { actualExecutionPaused: true, audioBefore: count(initial), audioAfter: count(stalled), trustedAcquisitions: stalled.window.input });
    await renderer.client.send("Debugger.resume"); page.pausedWorker = null;
    await renderer.evaluate(() => { __renderPortProbe.hold = true; });
    await delay(150);
    const held = await renderer.evaluate(() => __renderPortProbe.held.map(([, message]) => String(message.sequence)));
    assert(held.length > 0, "No original state acknowledgement held");
    const heldFrame = await renderer.evaluate(sequence => __renderPortProbe.records.find(x => x.direction === "received" && x.packet?.kind === 2 && x.packet.sequence === sequence)?.frame, held.at(-1));
    assert(heldFrame?.members[0]?.score, "Original held frame cannot be read by the bounded fixture parser");
    const target = Number(heldFrame.members[0].score.misses) + 2;
    await page.waitForFunction(misses => __renderIntegration.messages.some(x => x.kind === "play-render-done" && Number(x.misses) >= misses), { timeout: 5000 }, target);
    await renderer.evaluate(() => __renderPortProbe.release()); await delay(200); const resumed = await snapshot(page, "after-execution-pause-and-held-ack");
    const acknowledgements = resumed.workers.find(x => x.url.endsWith("/renderer-worker.js"))?.records.filter(x => x.kind === "state-ack") ?? [];
    assert(acknowledgements.some(x => BigInt(x.sequence) > BigInt(held.at(-1))), "No later correlated state after releasing held ACK");
    check("next-frame-after-held-ack", true, { held, acknowledgements: acknowledgements.slice(-4), scope: "Supplemental transport backpressure evidence" });
    const cumulative = resumed.workers.find(x => x.url.endsWith("/renderer-worker.js"))?.records.find(x => x.direction === "received" && x.packet?.kind === 2 && BigInt(x.packet.sequence) > BigInt(held.at(-1)) && Number(x.frame?.members[0]?.score?.misses) >= target && x.frame.members[0].pages.length);
    assert(cumulative, "No original cumulative packed progress after skipped frames");
    const member = cumulative.frame.members[0], progress = member.pages[0], completed = Number(member.score.hits) + Number(member.score.misses);
    assert.equal(progress.valid, 8); assert.equal(progress.completed, completed);
    assert.deepEqual(progress.states, Array.from({ length: 8 }, (_, i) => i < completed ? 2 : 0));
    assert(acknowledgements.some(x => x.sequence === cumulative.packet.sequence && x.generation === cumulative.packet.generation && x.content === cumulative.packet.content));
    check("cumulative-after-ack", true, { held: { sequence: held.at(-1), frame: heldFrame }, cumulative, expectedCompletedFromOriginalScore: completed });
    try {
      const atomic = await renderer.evaluate(() => __renderPortProbe.atomic());
      await writeFile(resolve(out, "atomic-before.png"), Uint8Array.from(atomic.before));
      await writeFile(resolve(out, "atomic-after-refusal.png"), Uint8Array.from(atomic.after));
      assert(atomic.refusal, "Malformed final progress slot was accepted");
      assert.deepEqual(atomic.after, atomic.before, "Malformed final page changed earlier score or retained visible state");
      assert.equal(atomic.applied, atomic.next.sequence, "Rejected higher sequence changed the applied sequence floor");
      assert.equal(atomic.coldSequence.refusals.length, 2);
      for (const rejected of atomic.coldSequence.refusals) {
        assert(rejected.refusal, `Direct generated WASM accepted cold sequence ${rejected.sequence}`);
        assert.equal(rejected.afterHash, atomic.coldSequence.coldBeforeHash, "Invalid cold sequence changed actual native GPU presentation");
      }
      assert.equal(atomic.coldSequence.originalNextApplied, atomic.coldSequence.originalNextSequence, "Invalid cold sequence changed the original state sequence floor");
      assert.equal(atomic.coldSequence.replacementApplied, "0", "Valid replacement cold registration was fenced by the refused higher generation");
      assert.equal(atomic.coldSequence.replacementNextApplied, atomic.coldSequence.originalNextSequence, "Valid replacement did not accept original committed frame sequence");
      assert.equal(atomic.coldSequence.replacementStateHash, atomic.coldSequence.nextStateHash, "Valid replacement lost the genuine committed visual model");
      check("cold-sequence-atomic", true, { ...atomic.coldSequence, scope: "Direct generated BrowserView imports captured original cold/frame packets; invalid cold1/MAX preserves GPU image and generation/sequence admission" });
      const { before, after, ...details } = atomic;
      check("malformed-atomic", true, { ...details, identicalGPUImages: true, scope: "Actual BrowserView mirror imports original actual-main packets; no gameplay authority constructed" });
    } catch (error) { check("malformed-atomic", false, `Actual retained-state comparison incomplete: ${error}`); check("cold-sequence-atomic", false, `Direct generated-WASM sequence admission comparison incomplete: ${error}`); }
    const injection = await renderer.evaluate(() => __renderPortProbe.malformed()); await stopped(page);
    await page.waitForFunction(() => __renderIntegration.messages.some(x => x.direction === "audio-context-closed"));
    const failed = await snapshot(page, "terminal-capture-cleanup");
    assert(failed.window.captures.some(x => x.kind === "play-error" && x.byteLength > 0 && x.complete === false));
    assert(failed.window.messages.some(x => x.kind === "play-error"));
    const delivered = failed.window.messages.find(x => x.kind === "play-error" && x.released === true);
    assert(delivered, "Gameplay failed stop did not release its owner");
    assert(!failed.window.messages.some(x => x.direction === "terminated" && x.url.endsWith("/worker.js") && x.at < delivered.at), "Gameplay terminated before genuine prefix delivery");
    const rendererRecords = failed.workers.find(x => x.url.endsWith("/renderer-worker.js"))?.records ?? [];
    assert(rendererRecords.some(x => x.kind === "render-error" && x.operationId === injection.operationId && x.generation === injection.generation && x.content === injection.content), "Malformed rejection identity was not correlated");
    assert(rendererRecords.some(x => x.kind === "render-error" && /visual page state or padding/.test(x.message)), "Original frame body did not reach actual packed-page validation");
    assert(!rendererRecords.some(x => x.kind === "state-ack" && x.operationId === injection.operationId), "Malformed candidate was acknowledged");
    check("terminal-capture-cleanup", true, failed.window.captures);
    check("malformed-correlated-rejection", true, { injection, delivered, audioClosed: true });
    await closePage(page);
  });
  await scenario("room", async () => {
    const roomURL = await startRoomBackend();
    const clients = await roomClients(puppeteer, roomURL, "long.bms", "room");
    for (const page of clients) await page.click("#stop");
    for (const page of clients) {
      await stopped(page, 25000);
      const renderer = [...page.probeWorkers.values()].find(x => x.url().endsWith("/renderer-worker.js"));
      await page.waitForFunction(() => document.querySelector("#play").disabled === false, { timeout: 25000 });
      const records = await renderer.evaluate(() => __renderPortProbe.records);
      assert(records.some(x => x.direction === "received" && x.packet?.kind === 6), "No actual frozen room packet after coordinated stop");
      await snapshot(page, `room-${clients.indexOf(page) + 1}`);
    }
    check("room", true, "Two actual main.js clients and genuine locally cancelled room state through the existing WebTransport backend");
    for (const page of clients) await closePage(page);
  });
  await scenario("combined-results-room", async () => {
    const roomURL = process.env.RENDER_ROOM_URL ?? `https://127.0.0.1:${Number(process.env.RENDER_ROOM_PORT ?? port + 1)}/rooms/render_integration_complete`;
    assert(backend || process.env.RENDER_ROOM_URL, "Actual compatible backend was not started");
    const clients = await roomClients(puppeteer, roomURL, "short.bms", "combined");
    const observations = [];
    for (const page of clients) {
      await stopped(page, 30000);
      await page.waitForFunction(() => document.querySelector("#play").disabled === false, { timeout: 25000 });
      const info = await snapshot(page, `combined-${clients.indexOf(page) + 1}`);
      observations.push(info);
    }
    const completed = observations.filter(info => info.window.messages.some(x => x.kind === "play-stopped" && x.completedResults?.proof));
    assert(completed.length > 0, "No genuine completed local owner in the actual room");
    for (const info of completed) {
      const terminal = info.window.messages.find(x => x.kind === "play-stopped");
      assert(terminal?.completedResults?.proof && info.window.captures.some(x => x.playId === terminal.playId && x.complete && x.byteLength > 0 && x.archiveByteLength > 0), "No genuine completed local owner and original archive in the room");
      const records = info.workers.find(x => x.url.endsWith("/renderer-worker.js"))?.records ?? [];
      const results = records.find(x => x.direction === "received" && x.packet?.kind === 5);
      assert(results, "Actual completed Results packet not observed");
      const combined = records.find(x => x.direction === "received" && x.packet?.kind === 6 && x.packet.generation === results.packet.generation && x.packet.content === results.packet.content);
      assert(combined, "No same-generation actual RESULTS plus ROOM combination");
      assert(records.some(x => x.kind === "state-ack" && x.packetKind === 6 && x.generation === combined.packet.generation && x.content === combined.packet.content));
      assert(info.window.messages.some(x => x.kind === "render-geometry" && x.mode === "results" && x.generation === results.packet.generation && x.width > 0 && x.height > 0));
      const peerFailures = observations.filter(x => x.window.messages.some(message => message.kind === "play-error"));
      if (peerFailures.length) {
        assert(combined.room?.pages.some(x => /room (read|closed).*failed/i.test(x.error ?? "")), "Actual peer failure was omitted from combined room presentation");
        for (const failed of peerFailures) {
          assert(!failed.window.messages.some(x => x.kind === "play-stopped" && x.completedResults?.proof));
          assert(failed.window.captures.some(x => x.kind === "play-error" && x.complete === false && x.byteLength > 0));
        }
        evidence.ceiling.push("Peer room audio failure retains original prefix and room diagnostic; dual-host natural completion/audio-clock acceptance remains outside this renderer task.");
      }
    }
    check("combined-results-room", true, { actualCompletedPresentations: completed.length, actualClients: observations.length, preservedPeerFailures: observations.filter(x => x.window.messages.some(message => message.kind === "play-error")).map(x => ({ terminal: x.window.messages.find(message => message.kind === "play-error"), captures: x.window.captures })), scope: "Genuine completed LOCAL record/archive plus same-generation actual Results/room and GPU submission; no clean dual-host completion claim" });
    check("two-host-natural-completion", completed.length === observations.length, "Supplemental audio/room outcome; not a renderer-only completion requirement");
    for (const page of clients) await closePage(page);
  });
  for (const id of required) if (!evidence.checks.some(x => x.id === id)) check(id, false, "Required evidence not reached; scope remains open");
}

try { await main(); }
catch (error) { evidence.fatal = { message: String(error), stack: error.stack }; }
finally {
  for (const page of pages) await closePage(page).catch(error => evidence.ceiling.push(`Page cleanup: ${error}`));
  for (const instance of browsers) await instance.close().catch(error => evidence.ceiling.push(`Browser cleanup: ${error}`));
  if (backend && backend.exitCode === null) {
    const closed = new Promise(resolveExit => backend.once("exit", resolveExit));
    backend.kill("SIGTERM");
    await Promise.race([closed, delay(3000).then(() => { if (backend.exitCode === null) backend.kill("SIGKILL"); })]);
    if (backend.exitCode === null && backend.signalCode === null) await Promise.race([closed, delay(1000)]);
    evidence.backend.stopped = backend.exitCode !== null || backend.signalCode !== null;
  }
  if (server) await new Promise(resolveClose => server.close(resolveClose));
  await mkdir(out, { recursive: true });
  await writeFile(resolve(out, "evidence.json"), JSON.stringify(evidence, null, 2));
  console.log(JSON.stringify({ checks: evidence.checks, fatal: evidence.fatal, output: out }, null, 2));
  if (evidence.fatal || required.some(id => !evidence.checks.some(x => x.id === id && x.passed))) process.exitCode = 1;
}
