/**
 * Portable development evidence from actual Chromium + generated audio WASM.
 * PUPPETEER_MODULE=/path/to/puppeteer-core CHROMIUM=/path/to/chromium \
 *   node app/web/worklet-chronology.browser.mjs
 * Optional WORKLET_CHRONOLOGY_OUT, WORKLET_CHRONOLOGY_PORT, TLS_CERT, TLS_KEY.
 * No dependency installs or external services. The owned HTTPS test response
 * wraps registerProcessor; repository production bytes and native clocks stay
 * unchanged. A deliberately omitted native callback is fault injection, not
 * evidence that naturally occurring browser chronology gaps have been repaired.
 */
import assert from "node:assert/strict";
import { createServer } from "node:https";
import { readFile, writeFile, mkdir } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { X509Certificate, createHash } from "node:crypto";
import { createRequire } from "node:module";
import { resolve, dirname, extname, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const out = resolve(process.env.WORKLET_CHRONOLOGY_OUT ?? resolve(root, "target/wf/worklet-chronology-browser-development"));
const port = Number(process.env.WORKLET_CHRONOLOGY_PORT ?? 8121);
const evidence = { kind: "development-worklet-chronology-browser", checks: [], scenarios: [],
  ceiling: ["Deliberate single native-callback omission; no natural gap repair, gameplay completion, replay-prefix or acoustic-output claim."], cleanup: {} };
const required = ["diagnostic-cold-observation", "native-gap", "late-arm", "sample-control"];
evidence.required = required;
let browser, server;

// This bootstrap is served only by the owned test server. Every call delegated
// to Production uses the untouched actual AudioWorkletGlobalScope currentFrame.
const bootstrap = `
const actualRegister = globalThis.registerProcessor.bind(globalThis);
globalThis.registerProcessor = (name, Production) => actualRegister(name, class extends Production {
  constructor(options) {
    super(options);
    this.probe = { successful: [], skipped: null, armedAt: null };
    const post = this.port.postMessage.bind(this.port);
    this.port.postMessage = (message, transfer) => post({ ...message, chronologyProbe: this.probe }, transfer);
  }
  control(message) {
    if (message?.kind === '__chronology-skip-one') { this.skipRequested = true; return; }
    const result = super.control(message);
    if (message?.kind === 'arm' && !this.failed && this.phase === 2) this.probe.armedAt = currentFrame;
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
    const result = super.process(inputs, outputs);
    if (result && phase > 0 && !this.failed) {
      this.probe.successful.push({ currentFrame: frame, blockFrames: outputs[0][0].length, phase });
      if (this.probe.successful.length > 32) this.probe.successful.shift();
    }
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
  server = createServer({ cert: await readFile(cert), key: await readFile(key) }, async (request, response) => {
    try {
      const pathname = new URL(request.url, "https://localhost").pathname;
      const path = resolve(root, `.${decodeURIComponent(pathname)}`);
      const within = relative(root, path);
      if (within === ".." || within.startsWith(`..${sep}`)) throw Error("Outside repository");
      const body = pathname === "/__chronology.html" ? "<!doctype html><title>Worklet chronology evidence</title>"
        : pathname === "/app/web/audio-worklet.js" ? bootstrap + production : await readFile(path);
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
  try {
    await page.goto(`https://127.0.0.1:${port}/__chronology.html`);
    return await page.evaluate(async name => {
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
      let host, commands, samples, caught = null, retained = null, report = null;
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
    }, name);
  } finally { await page.close(); }
}

function verify(result) {
  if (result.name === "normal") {
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
      evidence.checks.push({ id: "normal", passed: true, supplemental: true });
    }
    evidence.checks.push({ id: "diagnostic-cold-observation", passed: true });
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
    args: ["--no-sandbox", "--autoplay-policy=no-user-gesture-required", `--ignore-certificate-errors-spki-list=${spki}`] });
  for (const name of ["normal", "native-gap", "late-arm", "sample-control"]) {
    const result = await runCase(name); evidence.scenarios.push(result);
    try { verify(result); }
    catch (error) { evidence.checks.push({ id: name === "normal" ? "diagnostic-cold-observation" : name, passed: false, detail: String(error) }); process.exitCode = 1; }
  }
  if (required.some(id => !evidence.checks.some(x => x.id === id && x.passed))) process.exitCode = 1;
} catch (error) { evidence.fatal = { message: String(error), stack: error.stack }; process.exitCode = 1; }
finally {
  try { if (browser) { await browser.close(); evidence.cleanup.browserClosed = true; } }
  catch (error) { evidence.cleanup.browserError = String(error); process.exitCode = 1; }
  try { if (server) { server.closeAllConnections(); await new Promise(yes => server.close(yes)); evidence.cleanup.serverClosed = true; } }
  catch (error) { evidence.cleanup.serverError = String(error); process.exitCode = 1; }
  await mkdir(out, { recursive: true });
  await writeFile(resolve(out, "evidence.json"), JSON.stringify(evidence, (_, value) => typeof value === "bigint" ? String(value) : value, 2));
  console.log(JSON.stringify({ checks: evidence.checks, fatal: evidence.fatal, cleanup: evidence.cleanup, output: out }, null, 2));
}
