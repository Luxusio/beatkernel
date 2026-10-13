/**
 * Portable development evidence from actual Chromium + generated audio WASM.
 * PUPPETEER_MODULE=/path/to/puppeteer-core CHROMIUM=/path/to/chromium \
 *   node app/web/worklet-chronology.browser.mjs
 * Optional WORKLET_CHRONOLOGY_OUT, WORKLET_CHRONOLOGY_PORT, TLS_CERT, TLS_KEY.
 * WORKLET_CHRONOLOGY_REQUIRE_COLD=1 requires three fresh, healthy cold contexts;
 * the default remains the historical diagnostic-only acceptance mode.
 * WORKLET_NATIVE_FRAME_PROBE=1 instead measures independently scheduled native
 * AudioParam edges in untouched production phase0; it does not prove playback.
 * No dependency installs or external services. The owned HTTPS test response
 * wraps registerProcessor; repository production bytes and native clocks stay
 * unchanged. A deliberately omitted native callback is fault injection, not
 * evidence that naturally occurring browser chronology gaps have been repaired.
 */
import assert from "node:assert/strict";
import { createServer } from "node:https";
import { readFile, writeFile, mkdir, realpath } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { X509Certificate, createHash } from "node:crypto";
import { createRequire } from "node:module";
import { resolve, dirname, extname, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { advanceFrame, markerFrame } from "./worklet-native-frame-probe.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const out = resolve(process.env.WORKLET_CHRONOLOGY_OUT ?? resolve(root, "target/wf/worklet-chronology-browser-development"));
const port = Number(process.env.WORKLET_CHRONOLOGY_PORT ?? 8121);
const requireCold = process.env.WORKLET_CHRONOLOGY_REQUIRE_COLD === "1";
const nativeProbe = process.env.WORKLET_NATIVE_FRAME_PROBE === "1";
const coldCases = requireCold ? ["cold-1", "cold-2", "cold-3"] : ["normal"];
const nativeCases = [
  ...[44100, 48000].flatMap(rate => [1, 17, 63, 127].map(offset => ({ name: `native-${rate}-${offset}`, rate, offset }))),
  ...["constant", "index0", "late-first", "reset-before-late", "identities", "timeout", "stop", "suspend", "callback-limit"].map(name => ({ name, rate: 48000, offset: 17 })),
];
const evidence = { kind: "development-worklet-chronology-browser", checks: [], scenarios: [],
  mode: nativeProbe ? "native-phase0-marker" : requireCold ? "required-healthy-cold" : "historical-diagnostic",
  ceiling: nativeProbe ? [
    "Two native marker edges establish only their measured callback interval; future graph-pull gaps and generic counter continuity remain unproved.",
    "Reused generated WASM is artifact-specific phase0 evidence, not a fresh Rust rebuild or a repair of the prior strict browser chronology failure.",
    "AudioParam fill/scan and private diagnostic messaging are sustained test overhead, not a startup-only or zero-cost production claim.",
  ] : ["Measured cold-context acceptance only; no universal browser scheduling, high-frame performance, gameplay completion, replay-prefix or acoustic-output claim."], cleanup: {} };
const required = nativeProbe ? [...nativeCases.map(x => x.name), "fresh-context", "web-server-boundary"]
  : [...(requireCold ? coldCases : ["diagnostic-cold-observation"]), "native-gap", "late-arm", "sample-control"];
evidence.required = required;
let browser, server;

// This bootstrap is served only by the owned test server. Every call delegated
// to Production uses the untouched actual AudioWorkletGlobalScope currentFrame.
const bootstrap = `
const actualRegister = globalThis.registerProcessor.bind(globalThis);
globalThis.registerProcessor = (name, Production) => actualRegister(name, class extends Production {
  constructor(options) {
    super(options);
    this.probe = { successful: [], skipped: null, armedAt: null,
      firstUnarmedGap: null, armSnapshot: null, firstMixer: null };
    this.previousUnarmedEnd = null;
    this.mixerProbeAttempts = 0;
    const post = this.port.postMessage.bind(this.port);
    this.port.postMessage = (message, transfer) => post({ ...message, chronologyProbe: this.probe }, transfer);
  }
  control(message) {
    if (message?.kind === '__chronology-skip-one') { this.skipRequested = true; return; }
    const result = super.control(message);
    if (message?.kind === 'arm' && !this.failed && this.phase === 2) {
      this.probe.armedAt = currentFrame;
      this.probe.armSnapshot = { currentFrame, reportAvailable: this.owner.report_word(0, false),
        expectedPresent: this.owner.report_word(23, false),
        expectedLow: this.owner.report_word(24, false), expectedHigh: this.owner.report_word(24, true),
        startPresent: this.owner.report_word(25, false),
        startLow: this.owner.report_word(26, false), startHigh: this.owner.report_word(26, true) };
    }
    return result;
  }
  process(inputs, outputs) {
    if (this.skipRequested && !this.probe.skipped && this.phase === 2 && !this.failed
      && this.probe.successful.filter(x => x.phase === 2).length >= 4) {
      this.probe.skipped = { currentFrame, blockFrames: outputs[0][0].length };
      this.silence(outputs);
      return true;
    }
    const phase = this.phase;
    const frame = currentFrame;
    const frames = outputs[0][0].length;
    if (phase === 1 && frames > 0 && this.previousUnarmedEnd !== null
      && frame > this.previousUnarmedEnd && this.probe.firstUnarmedGap === null) {
      this.probe.firstUnarmedGap = { previousEnd: this.previousUnarmedEnd, currentFrame: frame,
        blockFrames: frames, reportAvailableBefore: this.owner.report_word(0, false), accepted: false };
    }
    const result = super.process(inputs, outputs);
    if (result && phase > 0 && !this.failed) {
      if (phase === 1 && frames > 0) {
        this.previousUnarmedEnd = frame + frames;
        if (this.probe.firstUnarmedGap?.currentFrame === frame) {
          this.probe.firstUnarmedGap.accepted = true;
          this.probe.firstUnarmedGap.reportAvailableAfter = this.owner.report_word(0, false);
        }
      }
      if (phase === 2 && this.probe.firstMixer === null && this.mixerProbeAttempts++ < 2048
        && this.owner.report_word(0, false) === 1) {
        this.probe.firstMixer = { currentFrame: frame, blockFrames: frames,
          startLow: this.owner.report_word(1, false), startHigh: this.owner.report_word(1, true),
          frames: this.owner.report_word(2, false),
          renderedLow: this.owner.report_word(12, false), renderedHigh: this.owner.report_word(12, true) };
      }
      this.probe.successful.push({ currentFrame: frame, blockFrames: frames, phase });
      if (this.probe.successful.length > 32) this.probe.successful.shift();
    }
    return result;
  }
});
`;

// Test-only native automation and private observations. Production protocol,
// sequence, PCM, clocks and render arguments are always delegated unchanged.
const nativeBootstrap = `
import { markerFrame } from './worklet-native-frame-probe.mjs';
const actualRegister = globalThis.registerProcessor.bind(globalThis);
globalThis.registerProcessor = (name, Production) => actualRegister(name, class extends Production {
  static get parameterDescriptors() {
    return [...(Production.parameterDescriptors ?? []),
      { name: '__native_marker', defaultValue: 0, minValue: 0, maxValue: 1, automationRate: 'a-rate' }];
  }
  constructor(options) {
    super(options);
    this.nativePort = null;
    this.nativeAttached = false;
    this.nativeRetired = false;
    this.nativeId = 0;
    this.nativeOrdinal = 0;
    this.nativeWatch = null;
  }
  nativeReply(message) { this.nativePort?.postMessage({ generation: this.generation, ...message }); }
  retireNative(reason) {
    if (!this.nativePort) return;
    this.nativeReply({ kind: 'retired', reason, id: this.nativeId,
      pendingInvalidated: this.nativeWatch !== null, ordinal: this.nativeOrdinal });
    this.nativeWatch = null;
    this.nativeRetired = true;
    this.nativePort.onmessage = null;
    this.nativePort.close();
    this.nativePort = null;
  }
  control(message) {
    if (message?.kind === '__native-attach') {
      const port = message.port;
      const reason = message.generation !== this.generation ? 'generation'
        : !Number.isSafeInteger(message.id) || message.id <= this.nativeId ? 'identity'
        : this.nativeRetired ? 'retired' : this.nativeAttached ? 'duplicate' : null;
      if (reason) {
        try { port?.postMessage({ kind: 'refused', generation: this.generation,
          id: message.id, reason, rejectedPortClosed: true }); }
        finally { port?.close(); }
        return;
      }
      this.nativeAttached = true;
      this.nativeId = message.id;
      this.nativePort = port;
      port.onmessage = event => {
        const request = event.data;
        const reason = request?.generation !== this.generation ? 'generation'
          : !Number.isSafeInteger(request.id) || request.id <= this.nativeId ? 'identity' : null;
        if (reason) { this.nativeReply({ kind: 'refused', id: request?.id, reason }); return; }
        this.nativeId = request.id;
        if (request.kind === 'retire') { this.retireNative(request.reason); return; }
        if (request.kind === 'watch' && (!Number.isSafeInteger(request.targetFrame) || request.targetFrame <= 0)) {
          this.nativeReply({ kind: 'refused', id: request.id, reason: 'target' }); return;
        }
        if (request.kind !== 'watch' && request.kind !== 'inspect') {
          this.nativeReply({ kind: 'refused', id: request.id, reason: 'operation' }); return;
        }
        if (request.kind === 'watch') this.nativeWatch = { id: request.id, targetFrame: request.targetFrame };
        this.nativeReply({ kind: 'accepted', id: request.id, operation: request.kind,
          phase: this.phase, reportAvailable: this.owner.report_word(0, false), ordinal: this.nativeOrdinal });
      };
      port.start();
      this.nativeReply({ kind: 'attached', id: message.id, phase: this.phase, defaultValue: 0 });
      return;
    }
    if (message?.kind === 'stop') this.retireNative('production-stop');
    return super.control(message);
  }
  process(inputs, outputs, parameters) {
    const result = super.process(inputs, outputs, parameters);
    if (!this.nativePort) return result;
    if (++this.nativeOrdinal > 1024) { this.retireNative('callback-limit'); return result; }
    const frames = outputs[0]?.[0]?.length ?? 0;
    const values = parameters.__native_marker;
    const targetFrame = this.nativeWatch?.targetFrame ?? null;
    const candidate = targetFrame === null ? null : markerFrame(values, frames, targetFrame);
    let edge = 0;
    while (edge < values.length && values[edge] === 0) edge++;
    const zeroPCM = outputs.every(bus => bus.every(channel => channel.every(value => value === 0)));
    this.nativeReply({ kind: 'callback', id: this.nativeWatch?.id ?? this.nativeId,
      ordinal: this.nativeOrdinal, rawGlobalFrame: currentFrame, actualFrames: frames,
      parameterLength: values.length, edge, spatialCandidate: candidate,
      shape: values.every(value => value === 0) ? 'zero' : values.every(value => value === 1) ? 'one' : 'varying',
      values: values.length > 1 && values.length <= 4096 ? Array.from(values) : null,
      phase: this.phase, result, zeroPCM, reportAvailable: this.owner.report_word(0, false) });
    return result;
  }
});
`;

async function serve() {
  await mkdir(out, { recursive: true });
  const cert = process.env.TLS_CERT ?? resolve(out, "cert.pem");
  const key = process.env.TLS_KEY ?? resolve(out, "key.pem");
  assert.equal(Boolean(process.env.TLS_CERT), Boolean(process.env.TLS_KEY), "Supply both TLS_CERT and TLS_KEY");
  if (!process.env.TLS_CERT) {
    const result = spawnSync("openssl", ["req", "-x509", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:prime256v1", "-nodes", "-keyout", key, "-out", cert, "-days", "1", "-subj", "/CN=localhost", "-addext", "subjectAltName=IP:127.0.0.1,DNS:localhost"], { encoding: "utf8", timeout: 10000 });
    assert.equal(result.status, 0, `Owned certificate generation failed: ${result.stderr}`);
  }
  const certificate = new X509Certificate(await readFile(cert));
  const spki = createHash("sha256").update(certificate.publicKey.export({ type: "spki", format: "der" })).digest("base64");
  const production = await readFile(resolve(root, "app/web/audio-worklet.js"), "utf8");
  const wasm = await readFile(resolve(root, "app/web/audio-pkg/beatkernel_bms_runtime_bg.wasm"));
  evidence.source = { workletSha256: createHash("sha256").update(production).digest("hex"), wasmSha256: createHash("sha256").update(wasm).digest("hex") };
  const webRoot = nativeProbe ? await realpath(resolve(root, "app/web")) : null;
  server = createServer({ cert: await readFile(cert), key: await readFile(key) }, async (request, response) => {
    try {
      const pathname = new URL(request.url, "https://localhost").pathname;
      const path = resolve(root, `.${decodeURIComponent(pathname)}`);
      const within = relative(root, path);
      if (within === ".." || within.startsWith(`..${sep}`)) throw Error("Outside repository");
      if (nativeProbe && pathname !== "/__chronology.html") {
        const lexical = relative(resolve(root, "app/web"), path);
        if (lexical === ".." || lexical.startsWith(`..${sep}`)) throw Error("Outside web fixture");
        const resolved = relative(webRoot, await realpath(path));
        if (resolved === ".." || resolved.startsWith(`..${sep}`)) throw Error("Outside resolved web fixture");
      }
      const body = pathname === "/__chronology.html" ? "<!doctype html><title>Worklet chronology evidence</title>"
        : pathname === "/app/web/audio-worklet.js" ? (nativeProbe ? nativeBootstrap : bootstrap) + production : await readFile(path);
      response.writeHead(200, { "Content-Type": pathname.endsWith(".html") ? "text/html" : extname(path) === ".wasm" ? "application/wasm" : "text/javascript",
        "Cross-Origin-Opener-Policy": "same-origin", "Cross-Origin-Embedder-Policy": "require-corp", "Cache-Control": "no-store" });
      response.end(body);
    } catch { response.writeHead(404); response.end(); }
  });
  await new Promise((yes, no) => { server.once("error", no); server.listen(port, "127.0.0.1", yes); });
  return spki;
}

async function runCase(name) {
  const page = await browser.newPage();
  const consoleMessages = [], pageErrors = [];
  page.on("console", message => { if (consoleMessages.length < 32) consoleMessages.push({ type: message.type(), text: message.text().slice(0, 2000) }); });
  page.on("pageerror", error => { if (pageErrors.length < 16) pageErrors.push(String(error).slice(0, 2000)); });
  try {
    await page.goto(`https://127.0.0.1:${port}/__chronology.html`);
    const result = await page.evaluate(async ({ name, requireCold }) => {
      const { AudioHost } = await import("/app/web/audio-host.mjs");
      const { AudioCommandClient } = await import("/app/web/audio-command-client.mjs");
      const { AudioSampleClient } = await import("/app/web/audio-sample-client.mjs");
      const { readAudioFailureDiagnostics } = await import("/app/web/audio-failure.mjs");
      const module = await WebAssembly.compile(await (await fetch("/app/web/audio-pkg/beatkernel_bms_runtime_bg.wasm")).arrayBuffer());
      const observed = [], contexts = [], nodes = [];
      const Context = AudioContext, Node = AudioWorkletNode;
      globalThis.AudioContext = class extends Context { constructor(options) { super(options); contexts.push(this); } };
      globalThis.AudioWorkletNode = class extends Node {
        constructor(...args) { super(...args); nodes.push(this); this.port.addEventListener("message", e => observed.push(e.data)); }
      };
      const wait = async predicate => {
        const limit = performance.now() + 5000;
        while (!predicate()) { if (performance.now() > limit) throw Error("Timed out waiting for actual Worklet delivery"); await new Promise(yes => setTimeout(yes, 5)); }
      };
      const snapshotError = error => error && ({ code: error.code, status: error.status, admitted: error.admitted, sequence: error.sequence,
        diagnostics: error.diagnostics, frozen: error.diagnostics === null || Object.isFrozen(error.diagnostics) });
      let host, commands, samples, caught = null, retained = null, report = null, setupReport = null;
      try {
        host = await AudioHost.open({ module, generation: 1, channels: 2, timeoutMs: 5000,
          contextOptions: { latencyHint: "interactive", sampleRate: 48000 },
          pcmLimits: { maxAssetBytes: 65536, maxTotalBytes: 131072, maxSamples: 4 },
          audioLimits: { queueCapacity: 32, maxVoices: 8, pendingCapacity: 32, maxFrames: 128, maxCommandsPerRender: 32 } });
        if (name === "sample-control") {
          const setup = await host.openSamplePort();
          setup.port.addEventListener("message", e => observed.push({ ...e.data, deliveredTo: "sample" }));
          samples = new AudioSampleClient(setup);
          try { await samples.sample({ id: 1n, rate: host.sampleRate, channels: 2, pcm: new Float32Array([NaN, 0]) }); }
          catch (error) { caught = error; }
        } else {
          // Targeted diagnostic fixtures wait in ordinary native phase0; no
          // native chronology has begun. The cold normal lane stays unchanged.
          // A previous cold run observed real current0 ->1664 / expected128,
          // status6 before arming. That failure is retained, never waived.
          if (name === "native-gap" || name === "late-arm") {
            await wait(() => host.currentFrame >= BigInt(Math.floor(host.sampleRate / 5)));
          }
          await host.finish();
          const setup = await host.openCommandPort();
          setup.port.addEventListener("message", e => observed.push({ ...e.data, deliveredTo: "command" }));
          commands = new AudioCommandClient(setup);
          if (name === "normal" && requireCold) setupReport = await commands.poll();
          try { await host.arm(name === "late-arm" ? 0n : host.currentFrame + BigInt(host.sampleRate)); }
          catch (error) { caught = error; }
          if (name === "native-gap") {
            report = await commands.poll();
            nodes[0].port.postMessage({ kind: "__chronology-skip-one" });
            await wait(() => commands.failure !== null);
            caught = commands.failure;
          } else if (name === "normal") {
            const deadline = performance.now() + 5000;
            do {
              await new Promise(yes => setTimeout(yes, 10));
              report = await commands.poll();
              if (performance.now() > deadline) throw Error("No actual started render report before deadline");
            } while (!report.available);
            await host.poll();
          }
        }
        if (name !== "normal") {
          await wait(() => host.state === "failed");
          try { await host.poll(); } catch (error) { retained = error; }
          if (name === "sample-control") {
            let again; try { await samples.sample({ id: 2n, rate: host.sampleRate, channels: 2, pcm: new Float32Array(2) }); } catch (error) { again = error; }
            if (again !== caught) throw Error("Sample error identity changed after terminal delivery");
          }
          let again; try { await host.poll(); } catch (error) { again = error; }
          if (again !== retained) throw Error("Host error identity changed after terminal delivery");
        }
        return { name, sampleRate: host.sampleRate, hostState: host.state, caught: snapshotError(caught), retained: snapshotError(retained),
          messages: observed, report: report && { available: report.available, words: Array.from(report.words) },
          setupReport: setupReport && { available: setupReport.available, words: Array.from(setupReport.words) },
          legacyDiagnostics: readAudioFailureDiagnostics({ kind: "terminal", generation: 1, status: 6 }),
          contexts: contexts.map(x => x.state) };
      } catch (error) {
        if (name === "normal" && error.status === 6) {
          await wait(() => host.state === "failed" && commands?.failure !== null);
          let first, again;
          try { await host.poll(); } catch (failure) { first = failure; }
          try { await host.poll(); } catch (failure) { again = failure; }
          if (first !== again) throw Error("Cold Host error identity changed after failure");
          return { name, unexpected: { message: String(error), ...snapshotError(error) },
            retained: snapshotError(first), commandFailure: snapshotError(commands.failure), messages: observed };
        }
        return { name, unexpected: { message: String(error), ...snapshotError(error) }, messages: observed };
      } finally {
        commands?.close(); samples?.close();
        if (host) await host.stop();
        if (contexts.some(x => x.state !== "closed")) throw Error("Owned AudioContext did not close");
      }
    }, { name, requireCold });
    return { ...result, consoleMessages, pageErrors };
  } finally { await page.close(); }
}

async function runNativeCase(spec, generation) {
  const page = await browser.newPage();
  const consoleMessages = [], pageErrors = [];
  page.on("console", message => { if (consoleMessages.length < 32) consoleMessages.push({ type: message.type(), text: message.text().slice(0, 2000) }); });
  page.on("pageerror", error => { if (pageErrors.length < 16) pageErrors.push(String(error).slice(0, 2000)); });
  try {
    await page.goto(`https://127.0.0.1:${port}/__chronology.html`);
    const result = await page.evaluate(async ({ spec, generation }) => {
      const { AudioHost } = await import("/app/web/audio-host.mjs");
      const { markerTime, advanceFrame } = await import("/app/web/worklet-native-frame-probe.mjs");
      const module = await WebAssembly.compile(await (await fetch("/app/web/audio-pkg/beatkernel_bms_runtime_bg.wasm")).arrayBuffer());
      const contexts = [], nodes = [], hostMessages = [], messages = [], ports = [], events = [], timestamps = [], stateEvents = [];
      const Context = AudioContext, Node = AudioWorkletNode;
      globalThis.AudioContext = class extends Context {
        constructor(options) {
          super(options); contexts.push(this);
          // Register before AudioHost assigns its terminal state handler. The
          // actual suspended event remains evidence even if Host closes next.
          this.addEventListener("statechange", () => {
            if (stateEvents.length < 16) stateEvents.push({ state: this.state, contextTime: this.currentTime });
          });
        }
      };
      globalThis.AudioWorkletNode = class extends Node {
        constructor(...args) {
          super(...args); nodes.push(this);
          this.port.addEventListener("message", event => { if (hostMessages.length < 64) hostMessages.push(event.data); });
        }
      };
      const result = { ...spec, generation, messages, events, timestamps, stateEvents, hostMessages, cleanup: {},
        prerequisites: { freshContext: true, previousOrigin: false, valueWrites: 0, nativeDefault: null } };
      let host, privatePort, id = 1, observationRetired = false, ticks = 0;
      const wait = async (predicate, timeoutMs = 5000, workload = false) => {
        const deadline = performance.now() + timeoutMs;
        while (!predicate()) {
          if (performance.now() >= deadline) throw Error("Native probe deadline exceeded");
          if (workload) {
            const before = performance.now();
            for (let count = 0; count < 200; count++) {
              const timestamp = contexts[0].getOutputTimestamp();
              if (count === 0 && timestamps.length < 32) timestamps.push(timestamp);
            }
            while (performance.now() - before < 2) Math.sqrt(++ticks);
          }
          await new Promise(yes => setTimeout(yes, 5));
        }
      };
      const bounded = async (promise, operation, timeoutMs = 12000) => {
        let timer;
        try {
          return await Promise.race([promise, new Promise((_, no) => {
            timer = setTimeout(() => no(Error("Native " + operation + " deadline exceeded")), timeoutMs);
          })]);
        } finally { clearTimeout(timer); }
      };
      const attach = async (attachGeneration, attachId) => {
        const channel = new MessageChannel(), replies = [];
        ports.push(channel.port1, channel.port2);
        channel.port1.onmessage = event => {
          if (replies.length >= 1100) throw Error("Private probe reply bound exceeded");
          replies.push(event.data);
          if (channel.port1 === privatePort) {
            if (messages.length >= 1100) throw Error("Native snapshot bound exceeded");
            messages.push(event.data);
            if (event.data.kind === "retired") observationRetired = true;
          }
        };
        channel.port1.start();
        nodes[0].port.postMessage({ kind: "__native-attach", generation: attachGeneration, id: attachId, port: channel.port2 }, [channel.port2]);
        await wait(() => replies.some(x => x.kind === "attached" || x.kind === "refused"));
        return { channel, replies };
      };
      const request = async (kind, fields = {}) => {
        const requestId = ++id;
        privatePort.postMessage({ kind, generation, id: requestId, ...fields });
        await wait(() => messages.some(x => x.id === requestId && x.kind !== "callback"));
        return requestId;
      };
      const watch = targetFrame => request("watch", { targetFrame });
      const set = (value, targetFrame) => {
        const time = markerTime(targetFrame, host.sampleRate);
        if (time === null) throw Error("Ambiguous scheduled marker time");
        const before = contexts[0].currentTime;
        nodes[0].parameters.get("__native_marker").setValueAtTime(value, time);
        events.push({ value, targetFrame, time, beforeContextTime: before, afterContextTime: contexts[0].currentTime });
        return time;
      };
      try {
        host = await AudioHost.open({ module, generation, channels: 2, timeoutMs: 5000,
          contextOptions: { latencyHint: "interactive", sampleRate: spec.rate },
          pcmLimits: { maxAssetBytes: 65536, maxTotalBytes: 131072, maxSamples: 4 },
          audioLimits: { queueCapacity: 32, maxVoices: 8, pendingCapacity: 32, maxFrames: 128, maxCommandsPerRender: 32 } });
        result.sampleRate = host.sampleRate;
        result.beforeReport = await host.poll();
        result.prerequisites.nativeDefault = nodes[0].parameters.get("__native_marker").defaultValue;
        const connected = await attach(generation, id);
        privatePort = connected.channel.port1;
        messages.push(...connected.replies);
        await wait(() => messages.some(x => x.kind === "callback"));
        const initial = messages.find(x => x.kind === "callback");
        const quantum = initial.actualFrames;
        if (!Number.isSafeInteger(quantum) || quantum < 1) throw Error("No actual native callback extent");
        const futureTarget = offset => {
          const controlFrame = Math.ceil(contexts[0].currentTime * host.sampleRate);
          return (Math.ceil(controlFrame / quantum) + Math.ceil(host.sampleRate * 0.2 / quantum)) * quantum + offset;
        };
        const positive = spec.name.startsWith("native-");
        if (positive) {
          if (spec.offset >= quantum) throw Error("Requested interior offset exceeds measured callback shape");
          const firstTarget = futureTarget(spec.offset);
          const firstId = await watch(firstTarget);
          result.firstTarget = firstTarget;
          result.firstTime = set(1, firstTarget); // Exactly the first and only event on the fresh default0 parameter.
          await wait(() => messages.some(x => x.kind === "callback" && x.id === firstId && x.spatialCandidate !== null), 5000, true);
          const first = messages.find(x => x.kind === "callback" && x.id === firstId && x.spatialCandidate !== null);
          result.first = first;
          result.firstAdmission = { eventCount: events.length, previousEvents: false,
            eligible: events.length === 1 && events[0].value === 1 && result.prerequisites.nativeDefault === 0 };
          if (!result.firstAdmission.eligible) throw Error("First native onset has a contaminated timeline");
          const secondTarget = futureTarget(spec.offset);
          const secondId = await watch(secondTarget);
          result.secondTarget = secondTarget;
          result.resetTime = set(0, secondTarget - 2 * quantum);
          result.secondTime = set(1, secondTarget);
          if (result.resetTime <= contexts[0].currentTime) throw Error("Second marker reset did not remain future scheduled");
          await wait(() => messages.some(x => x.kind === "callback" && x.id === secondId && x.spatialCandidate !== null), 5000, true);
          const second = messages.find(x => x.kind === "callback" && x.id === secondId && x.spatialCandidate !== null);
          result.second = second;
          const interval = messages.filter(x => x.kind === "callback" && x.ordinal >= first.ordinal && x.ordinal < second.ordinal);
          let expected = first.spatialCandidate;
          for (const callback of interval) {
            expected = advanceFrame(expected, callback.actualFrames);
            if (expected === null) throw Error("Observed native extent overflowed");
          }
          result.expectedSecondBase = expected;
          result.intervalOrdinals = interval.map(x => x.ordinal);
          if (second.spatialCandidate !== expected) throw Error("Second independent native basis contradicts actual callback extents; no rebase allowed");
        } else if (spec.name === "identities") {
          const stale = await attach(generation - 1, id + 1);
          const repeated = await attach(generation, id);
          const duplicate = await attach(generation, id + 1);
          result.rejectedAttachments = [stale.replies[0], repeated.replies[0], duplicate.replies[0]];
          stale.channel.port1.close(); stale.channel.port2.close();
          repeated.channel.port1.close(); repeated.channel.port2.close();
          duplicate.channel.port1.close(); duplicate.channel.port2.close();
          privatePort.postMessage({ kind: "inspect", generation: generation - 1, id: id + 1 });
          privatePort.postMessage({ kind: "inspect", generation, id });
          await wait(() => messages.filter(x => x.kind === "refused").length === 2);
          await request("watch", { targetFrame: -1 });
          await request("unknown");
          await request("inspect");
          result.retainedIdentity = id;
        } else if (spec.name === "index0" || spec.name === "late-first" || spec.name === "reset-before-late") {
          let target = futureTarget(0);
          if (spec.name !== "index0") {
            await wait(() => contexts[0].currentTime > 0.05);
            target = Math.max(1, Math.floor((contexts[0].currentTime - 0.025) * host.sampleRate));
          }
          if (spec.name === "reset-before-late") set(0, 1);
          result.negativeTarget = target;
          const watched = await watch(target);
          result.negativeTime = set(1, target);
          result.firstAdmission = { eventCount: events.length, previousEvents: events.length !== 1, eligible: false };
          await wait(() => messages.some(x => x.kind === "callback" && x.id === watched && x.shape === "one"));
          result.certifiedOrigin = null;
          result.rawSpatialCandidates = messages.filter(x => x.kind === "callback" && x.id === watched && x.spatialCandidate !== null);
        } else {
          const watched = await watch(futureTarget(spec.offset));
          if (spec.name === "constant") {
            await wait(() => messages.filter(x => x.kind === "callback" && x.id === watched).length >= 8);
          } else if (spec.name === "timeout") {
            try { await wait(() => messages.some(x => x.kind === "callback" && x.id === watched && x.spatialCandidate !== null), 40); }
            catch (error) { result.timeout = String(error); }
            if (!result.timeout) throw Error("Constant native parameter fabricated an onset before timeout");
            await request("retire", { reason: "timeout" });
            const rejected = await attach(generation, id + 1);
            result.retiredAttachment = rejected.replies[0];
            rejected.channel.port1.close(); rejected.channel.port2.close();
          } else if (spec.name === "stop") {
            await bounded(host.stop(), "stop");
            await wait(() => observationRetired);
          } else if (spec.name === "suspend") {
            await request("retire", { reason: "suspend" });
            await bounded(contexts[0].suspend(), "suspend", 5000);
            await wait(() => stateEvents.some(event => event.state === "suspended") && host.state === "failed");
            result.suspendedState = stateEvents.findLast(event => event.state === "suspended").state;
            try { await host.poll(); }
            catch (error) { result.suspendFailure = { code: error.code, operation: error.operation, message: String(error) }; }
            await bounded(host.stop(), "suspended Host cleanup");
          } else if (spec.name === "callback-limit") {
            await wait(() => observationRetired);
          }
          result.certifiedOrigin = null;
        }
        result.hostState = host.state;
        if (host.state === "setup") result.afterReport = await host.poll();
      } catch (error) {
        result.unexpected = { message: String(error), stack: error.stack };
      } finally {
        try {
          if (privatePort && !observationRetired) await request("retire", { reason: "case-complete" });
          if (host) await bounded(host.stop(), "owned Host cleanup");
          result.cleanup.contextsConstructed = contexts.length;
          result.cleanup.nodesConstructed = nodes.length;
          result.cleanup.contexts = contexts.map(context => context.state);
          result.cleanup.contextsClosed = contexts.every(context => context.state === "closed");
        } catch (error) { result.cleanup.error = String(error); }
        for (const ownedPort of ports) { ownedPort.onmessage = null; ownedPort.close(); }
        result.cleanup.ownedPortEndpointsClosed = ports.length;
        result.cleanup.observationRetired = observationRetired;
        result.cleanup.pendingInvalidated = messages.findLast(x => x.kind === "retired")?.pendingInvalidated ?? false;
      }
      return result;
    }, { spec, generation });
    return { ...result, consoleMessages, pageErrors };
  } finally { await page.close(); }
}

function verifyNative(result) {
  assert.equal(result.pageErrors.length, 0, "Unexpected native probe page error");
  assert(!result.unexpected, JSON.stringify(result.unexpected));
  assert(!result.cleanup.error, result.cleanup.error);
  assert(result.cleanup.contextsClosed && result.cleanup.observationRetired, "Owned native observer/context did not retire");
  assert.equal(result.cleanup.contextsConstructed, 1);
  assert.equal(result.cleanup.nodesConstructed, 1);
  assert(result.cleanup.ownedPortEndpointsClosed >= 2);
  assert.equal(result.prerequisites.nativeDefault, 0);
  assert.equal(result.prerequisites.valueWrites, 0);
  assert.equal(result.beforeReport.available, false);
  if (result.afterReport) assert.equal(result.afterReport.available, false);
  assert(result.hostMessages.every(x => ["ready", "ack", "terminal"].includes(x.kind)), "Private reply corrupted the production Host protocol");
  const callbacks = result.messages.filter(x => x.kind === "callback");
  assert(callbacks.length > 0 && callbacks.length <= 1024);
  for (const callback of callbacks) {
    assert.equal(callback.generation, result.generation);
    assert.equal(callback.phase, 0);
    assert.equal(callback.zeroPCM, true);
    assert.equal(callback.result, true);
    assert.equal(callback.reportAvailable, 0);
    assert(Number.isSafeInteger(callback.actualFrames) && callback.actualFrames > 0);
  }
  if (result.name.startsWith("native-")) {
    assert(result.firstAdmission.eligible && result.firstAdmission.eventCount === 1);
    assert.equal(result.events[0].value, 1);
    assert.equal(result.events[0].targetFrame, result.firstTarget);
    assert.equal(result.first.edge, result.offset);
    assert.equal(result.second.edge, result.offset);
    assert(result.timestamps.length > 0, "Output timestamp workload was not exercised");
    for (const [marker, target] of [[result.first, result.firstTarget], [result.second, result.secondTarget]]) {
      assert.equal(marker.parameterLength, marker.actualFrames);
      assert.equal(markerFrame(new Float32Array(marker.values), marker.actualFrames, target), marker.spatialCandidate);
    }
    const interval = callbacks.filter(x => x.ordinal >= result.first.ordinal && x.ordinal < result.second.ordinal);
    assert.equal(interval.length, result.second.ordinal - result.first.ordinal);
    let cursor = result.first.spatialCandidate;
    for (let index = 0; index < interval.length; index++) {
      assert.equal(interval[index].ordinal, result.first.ordinal + index, "An actual callback observation was omitted");
      cursor = advanceFrame(cursor, interval[index].actualFrames);
      assert.notEqual(cursor, null);
    }
    assert.equal(cursor, result.second.spatialCandidate, "Independent native marker contradiction must fail without rebasing");
    assert.equal(result.expectedSecondBase, cursor);
  } else if (result.name === "identities") {
    assert.deepEqual(result.rejectedAttachments.map(x => x.reason), ["generation", "identity", "duplicate"]);
    assert(result.rejectedAttachments.every(x => x.rejectedPortClosed));
    assert.deepEqual(result.messages.filter(x => x.kind === "refused").map(x => x.reason), ["generation", "identity", "target", "operation"]);
    assert(result.messages.some(x => x.kind === "accepted" && x.operation === "inspect" && x.id === result.retainedIdentity));
  } else {
    assert.equal(result.certifiedOrigin, null);
    if (result.name === "constant" || result.name === "timeout") assert(callbacks.every(x => x.spatialCandidate === null && x.shape === "zero"));
    if (result.name === "index0" || result.name === "late-first") assert.equal(result.rawSpatialCandidates.length, 0, "Fresh boundary/late event unexpectedly yielded an interior anchor");
    if (result.name === "reset-before-late") assert.equal(result.firstAdmission.previousEvents, true);
    if (["timeout", "stop", "suspend", "callback-limit"].includes(result.name)) {
      assert.equal(result.cleanup.pendingInvalidated, true);
      const retired = result.messages.find(x => x.kind === "retired");
      assert.equal(retired.reason, result.name === "stop" ? "production-stop" : result.name);
      if (result.name === "callback-limit") assert.equal(callbacks.length, 1024);
    }
    if (result.name === "suspend") {
      assert.equal(result.suspendedState, "suspended");
      assert(result.stateEvents.some(event => event.state === "suspended"));
      assert.equal(result.hostState, "failed");
      assert.equal(result.suspendFailure?.code, "state");
      assert.equal(result.suspendFailure?.operation, "context");
    }
    if (result.name === "timeout") {
      assert.equal(result.retiredAttachment.reason, "retired");
      assert(result.retiredAttachment.rejectedPortClosed);
    }
  }
  evidence.checks.push({ id: result.name, passed: true });
}

const pair = (low, high) => BigInt(low) | BigInt(high) << 32n;
function verify(result, id = result.name) {
  assert.equal(result.pageErrors.length, 0, "Unexpected browser page error");
  if (result.name === "normal") {
    if (requireCold) assert(!result.unexpected, `Required healthy ${id} failed: ${JSON.stringify(result.unexpected)}`);
    if (result.unexpected) {
      const terminal = result.messages.find(x => x.kind === "terminal" && !x.deliveredTo);
      assert(terminal, "Cold failure has no actual terminal");
      assert.equal(terminal.status, 6); assert.equal(result.unexpected.status, 6);
      const d = result.retained?.diagnostics;
      assert(d && result.retained.frozen && result.commandFailure?.frozen);
      assert.equal(result.retained.status, 6); assert.equal(result.commandFailure.status, 6);
      assert.deepEqual(result.commandFailure.diagnostics, d);
      assert.deepEqual(result.unexpected.diagnostics, d);
      for (const [key, value] of Object.entries(d)) assert.equal(terminal[key], value);
      assert.equal(d.diagnosticVersion, 1); assert.equal(d.origin, 2);
      assert(d.ownerPhase === 1 || d.ownerPhase === 2);
      assert.equal(d.currentFramePresent, 1); assert.equal(d.blockFramesPresent, 1);
      assert.equal(d.expectedFramePresent, 1);
      const expected = BigInt(d.expectedFrameLow) | BigInt(d.expectedFrameHigh) << 32n;
      assert.notEqual(expected, BigInt(d.currentFrame), "Cold observation has no actual native chronology mismatch");
      const probe = terminal.chronologyProbe;
      assert.equal(probe.skipped, null, "Cold lane performed fault injection");
      const prior = probe.successful.at(-1);
      assert(prior, "Cold gap has no preceding successful native callback");
      assert.equal(BigInt(prior.currentFrame + prior.blockFrames), expected);
      assert.equal(d.blockFrames, prior.blockFrames);
      const commandTerminal = result.messages.find(x => x.kind === "terminal" && x.deliveredTo === "command");
      assert(commandTerminal); for (const [key, value] of Object.entries(d)) assert.equal(commandTerminal[key], value);
      evidence.checks.push({ id: "normal", passed: false, supplemental: true, detail: "Actual uninjected cold chronology failure remains unresolved.", diagnostics: d });
      evidence.ceiling.push("Cold audio health remains false: actual uninjected native expected/current mismatch; diagnostic acceptance does not repair chronology.");
    } else {
      assert.equal(result.legacyDiagnostics, null);
      assert.equal(result.hostState, "armed");
      assert.equal(result.report.available, true);
      const callbacks = result.messages.flatMap(x => x.chronologyProbe?.successful ?? []);
      assert(callbacks.some(x => x.phase === 2), "No actual successful armed callbacks");
      assert(!result.messages.some(x => x.kind === "terminal"));
      if (requireCold) {
        assert.equal(result.setupReport.available, false, "Setup credited a Mixer report");
        const probe = result.messages.findLast(x => x.chronologyProbe?.firstMixer)?.chronologyProbe;
        assert(probe, "No actual first Mixer callback snapshot");
        assert.equal(probe.skipped, null, "Cold lane injected an omitted callback");
        const arm = probe.armSnapshot;
        assert(arm, "No actual post-control arm snapshot");
        assert.equal(arm.reportAvailable, 0, "Arm fabricated a Mixer report");
        assert.equal(arm.expectedPresent, 1); assert.equal(arm.startPresent, 1);
        assert.equal(pair(arm.expectedLow, arm.expectedHigh), BigInt(arm.currentFrame), "Arm did not adopt exact actual currentFrame");
        assert.equal(arm.currentFrame, probe.armedAt);
        const start = pair(arm.startLow, arm.startHigh);
        const first = probe.firstMixer;
        assert.equal(pair(first.startLow, first.startHigh), 0n, "Setup gap credited to Mixer origin");
        assert.equal(pair(first.renderedLow, first.renderedHigh), BigInt(first.frames), "First Mixer counters credited setup frames");
        assert(first.frames > 0 && first.frames <= first.blockFrames);
        assert(BigInt(first.currentFrame) <= start && start < BigInt(first.currentFrame + first.blockFrames));
        assert.equal(BigInt(first.currentFrame + first.blockFrames) - start, BigInt(first.frames));
        const words = result.report.words;
        assert.equal(pair(words[48], words[49]) - start, pair(words[2], words[3]) + pair(words[4], words[5]), "Actual report left the start-relative Mixer grid");
        if (probe.firstUnarmedGap) {
          const gap = probe.firstUnarmedGap;
          assert(gap.currentFrame > gap.previousEnd && gap.blockFrames > 0);
          assert.equal(gap.accepted, true); assert.equal(gap.reportAvailableBefore, 0); assert.equal(gap.reportAvailableAfter, 0);
        }
      }
      evidence.checks.push({ id: "normal", passed: true, supplemental: true });
    }
    evidence.checks.push({ id: requireCold ? id : "diagnostic-cold-observation", passed: true });
    return;
  } else {
    assert(!result.unexpected, JSON.stringify(result.unexpected));
    assert.equal(result.legacyDiagnostics, null);
    const terminal = result.messages.find(x => x.kind === "terminal" && !x.deliveredTo);
    assert(terminal, "No actual host terminal");
    const d = result.retained.diagnostics;
    assert.equal(terminal.diagnosticVersion, 1);
    assert(result.retained.frozen && result.caught.frozen);
    for (const [key, value] of Object.entries(d)) assert.equal(terminal[key], value, `Retained host fact ${key}`);
    assert.equal(result.retained.status, terminal.status);
    assert.deepEqual(result.caught.diagnostics, d);
    assert.equal(result.caught.status, terminal.status);
    if (result.name === "native-gap") {
      assert.equal(terminal.status, 6);
      assert.equal(d.origin, 2); assert.equal(d.ownerPhase, 2);
      assert.equal(d.currentFramePresent, 1); assert.equal(d.blockFramesPresent, 1);
      assert.equal(d.expectedFramePresent, 1); assert.equal(d.startFramePresent, 1);
      assert.equal(d.successfulArmFramePresent, 1);
      const expected = BigInt(d.expectedFrameLow) | BigInt(d.expectedFrameHigh) << 32n;
      const probe = terminal.chronologyProbe;
      assert(probe.skipped); assert.equal(expected, BigInt(probe.skipped.currentFrame));
      assert.equal(BigInt(d.currentFrame), expected + BigInt(probe.skipped.blockFrames));
      assert.equal(d.blockFrames, probe.skipped.blockFrames);
      assert.equal(d.successfulArmFrame, probe.armedAt);
      const callbacks = probe.successful.filter(x => x.phase === 2);
      assert(callbacks.length >= 4);
      assert.equal(callbacks.at(-1).currentFrame + callbacks.at(-1).blockFrames, probe.skipped.currentFrame);
      const commandTerminal = result.messages.find(x => x.kind === "terminal" && x.deliveredTo === "command");
      assert(commandTerminal); for (const [key, value] of Object.entries(d)) assert.equal(commandTerminal[key], value);
    } else {
      assert.equal(result.caught.code, "remote"); assert.equal(result.caught.admitted, 0);
      const ackIndex = result.messages.findIndex(x => x.kind === "ack" && x.status !== 0);
      assert(ackIndex >= 0); assert(ackIndex < result.messages.indexOf(terminal), "Failure ACK did not precede terminal");
      assert.equal(d.origin, result.name === "late-arm" ? 1 : 0);
      assert.equal(d.blockFramesPresent, 0);
      assert.equal(d.successfulArmFramePresent, 0);
    }
  }
  evidence.checks.push({ id: result.name, passed: true });
}

try {
  const spki = await serve();
  const require = createRequire(import.meta.url);
  const puppeteer = require(process.env.PUPPETEER_MODULE ?? "puppeteer-core");
  browser = await puppeteer.launch({ executablePath: process.env.CHROMIUM ?? "/usr/bin/chromium", headless: true,
    ...(nativeProbe ? { protocolTimeout: 30000 } : {}),
    args: ["--no-sandbox", "--autoplay-policy=no-user-gesture-required", `--ignore-certificate-errors-spki-list=${spki}`] });
  if (nativeProbe) {
    evidence.browser = { version: await browser.version(), ownedPid: browser.process()?.pid ?? null };
    for (let index = 0; index < nativeCases.length; index++) {
      const result = await runNativeCase(nativeCases[index], index + 11);
      evidence.scenarios.push(result);
      try { verifyNative(result); }
      catch (error) { evidence.checks.push({ id: result.name, passed: false, detail: String(error) }); process.exitCode = 1; }
    }
    const fresh = evidence.scenarios.filter(x => x.name.startsWith("native-"));
    const freshPassed = fresh.length === 8 && new Set(fresh.map(x => x.generation)).size === 8
      && fresh.every(x => x.firstAdmission?.eligible && x.prerequisites.previousOrigin === false && x.cleanup.contextsClosed);
    evidence.checks.push({ id: "fresh-context", passed: freshPassed });
    const boundary = await browser.newPage();
    try {
      await boundary.goto(`https://127.0.0.1:${port}/__chronology.html`);
      evidence.serverBoundary = await boundary.evaluate(async () => {
        const paths = ["/CONTRACTS.md", "/doc/changes/2026-10-13-browser-native-frame-diagnostic.md", "/app/web/../../CONTRACTS.md"];
        return Promise.all(paths.map(async path => ({ path, status: (await fetch(path)).status })));
      });
      evidence.checks.push({ id: "web-server-boundary", passed: evidence.serverBoundary.every(x => x.status === 404) });
    } finally { await boundary.close(); }
  } else {
    for (const id of [...coldCases, "native-gap", "late-arm", "sample-control"]) {
      const name = coldCases.includes(id) ? "normal" : id;
      const result = await runCase(name); evidence.scenarios.push({ ...result, id });
      try { verify(result, id); }
      catch (error) { evidence.checks.push({ id: name === "normal" && !requireCold ? "diagnostic-cold-observation" : id, passed: false, detail: String(error) }); process.exitCode = 1; }
    }
  }
  if (required.some(id => !evidence.checks.some(x => x.id === id && x.passed))) process.exitCode = 1;
} catch (error) { evidence.fatal = { message: String(error), stack: error.stack }; process.exitCode = 1; }
finally {
  try { if (browser) { await browser.close(); evidence.cleanup.browserClosed = true;
    if (nativeProbe) evidence.cleanup.ownedBrowserExitCode = browser.process()?.exitCode ?? null; } }
  catch (error) { evidence.cleanup.browserError = String(error); process.exitCode = 1; }
  try { if (server) { server.closeAllConnections(); await new Promise(yes => server.close(yes)); evidence.cleanup.serverClosed = true; } }
  catch (error) { evidence.cleanup.serverError = String(error); process.exitCode = 1; }
  await mkdir(out, { recursive: true });
  await writeFile(resolve(out, "evidence.json"), JSON.stringify(evidence, (_, value) => typeof value === "bigint" ? String(value) : value, 2));
  console.log(JSON.stringify({ checks: evidence.checks, fatal: evidence.fatal, cleanup: evidence.cleanup, output: out }, null, 2));
}
