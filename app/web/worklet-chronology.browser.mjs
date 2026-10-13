/**
 * Portable development evidence from actual Chromium + generated audio WASM.
 * PUPPETEER_MODULE=/path/to/puppeteer-core CHROMIUM=/path/to/chromium \
 *   node app/web/worklet-chronology.browser.mjs
 * Optional WORKLET_CHRONOLOGY_OUT, WORKLET_CHRONOLOGY_PORT, TLS_CERT, TLS_KEY.
 * WORKLET_CHRONOLOGY_REQUIRE_COLD=1 requires three fresh, healthy cold contexts;
 * the default remains the historical diagnostic-only acceptance mode.
 * WORKLET_NATIVE_FRAME_PROBE=1 instead measures independently scheduled native
 * AudioParam edges in untouched production phase0; it does not prove playback.
 * WORKLET_NATIVE_PHASE_PROBE=1 streams three finite native ramp periods in the
 * same fixture; modulo agreement does not admit an absolute production clock.
 * WORKLET_NATIVE_JOINT_PHASE_PROBE=1 measures three fixed coprime native lanes.
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
import { advanceFrame, markerFrame, phaseEvidence, nativeJointOffset } from "./worklet-native-frame-probe.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const out = resolve(process.env.WORKLET_CHRONOLOGY_OUT ?? resolve(root, "target/wf/worklet-chronology-browser-development"));
const port = Number(process.env.WORKLET_CHRONOLOGY_PORT ?? 8121);
const requireCold = process.env.WORKLET_CHRONOLOGY_REQUIRE_COLD === "1";
const nativeProbe = process.env.WORKLET_NATIVE_FRAME_PROBE === "1";
const phaseProbe = process.env.WORKLET_NATIVE_PHASE_PROBE === "1";
const jointProbe = process.env.WORKLET_NATIVE_JOINT_PHASE_PROBE === "1";
const phaseFixture = phaseProbe || jointProbe;
const nativeFixture = nativeProbe || phaseFixture;
const coldCases = requireCold ? ["cold-1", "cold-2", "cold-3"] : ["normal"];
const nativeCases = [
  ...[44100, 48000].flatMap(rate => [1, 17, 63, 127].map(offset => ({ name: `native-${rate}-${offset}`, rate, offset }))),
  ...["constant", "index0", "late-first", "reset-before-late", "identities", "timeout", "stop", "suspend", "callback-limit"].map(name => ({ name, rate: 48000, offset: 17 })),
];
const phaseCases = [
  ...[44100, 48000].map(rate => ({ name: `phase-${rate}`, rate, offset: 17, phaseProbe: true, operation: "stream" })),
  ...["constant", "late", "identities", "timeout", "stop", "suspend", "callback-limit"]
    .map(operation => ({ name: `phase-${operation}`, rate: 48000, offset: 17, phaseProbe: true, operation })),
];
const jointCases = [
  ...[44100, 48000].map(rate => ({ name: `joint-${rate}`, rate, offset: 17, phaseProbe: true, jointProbe: true, operation: "stream" })),
  ...["constant", "late", "identities", "timeout", "stop", "suspend", "callback-limit"]
    .map(operation => ({ name: `joint-${operation}`, rate: 48000, offset: 17, phaseProbe: true, jointProbe: true, operation })),
];
const evidence = { kind: "development-worklet-chronology-browser", checks: [], scenarios: [],
  mode: jointProbe ? "native-phase0-joint-modulo" : phaseProbe ? "native-phase0-modulo" : nativeProbe ? "native-phase0-marker" : requireCold ? "required-healthy-cold" : "historical-diagnostic",
  ceiling: nativeFixture ? [
    "Two native marker edges establish only their measured callback interval; future graph-pull gaps and generic counter continuity remain unproved.",
    "Reused generated WASM is artifact-specific phase0 evidence, not a fresh Rust rebuild or a repair of the prior strict browser chronology failure.",
    "AudioParam fill/scan and private diagnostic messaging are sustained test overhead, not a startup-only or zero-cost production claim.",
    ...(phaseProbe ? ["A single repeating phase cannot distinguish nP-frame gaps; checked samples and conditioned endpoint corroboration establish only this measured interval."] : []),
    ...(jointProbe ? ["Joint residues identify only Number-safe modulo candidates; native samples alone cannot certify epoch, age or future chronology beyond the finite measured interval."] : []),
  ] : ["Measured cold-context acceptance only; no universal browser scheduling, high-frame performance, gameplay completion, replay-prefix or acoustic-output claim."], cleanup: {} };
const required = jointProbe ? [...jointCases.map(x => x.name), "joint-fresh-context", "web-server-boundary"]
  : phaseProbe ? [...phaseCases.map(x => x.name), "phase-fresh-context", "web-server-boundary"]
  : nativeProbe ? [...nativeCases.map(x => x.name), "fresh-context", "web-server-boundary"]
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
import { markerFrame, advanceFrame, nativePhase, phaseIdeal, phaseEvidence, nativeJointOffset } from './worklet-native-frame-probe.mjs';
const phaseMode = ${phaseProbe || jointProbe};
const jointMode = ${jointProbe};
const jointPeriods = [1048576, 1048575, 1048573];
const actualRegister = globalThis.registerProcessor.bind(globalThis);
globalThis.registerProcessor = (name, Production) => actualRegister(name, class extends Production {
  static get parameterDescriptors() {
    return [...(Production.parameterDescriptors ?? []),
      { name: '__native_marker', defaultValue: 0, minValue: 0, maxValue: 1, automationRate: 'a-rate' },
      ...(jointMode ? jointPeriods.map((period, lane) => ({ name: '__native_joint_phase_' + lane,
        defaultValue: 0, minValue: 0, maxValue: period + 1, automationRate: 'a-rate' }))
        : phaseMode ? [{ name: '__native_phase', defaultValue: 0, minValue: 0, maxValue: 1048577, automationRate: 'a-rate' }] : [])];
  }
  constructor(options) {
    super(options);
    this.nativePort = null;
    this.nativeAttached = false;
    this.nativeRetired = false;
    this.nativeId = 0;
    this.nativeOrdinal = 0;
    this.nativeWatch = null;
    if (phaseMode) {
      this.nativeAnchor = null;
      this.nativeCursor = null;
      this.nativeSession = null;
      this.nativeRawSnapshots = 0;
      this.nativeStats = { callbacks: 0, observedFrames: 0, minFrames: null, maxFrames: 0,
        phase0: true, zeroPCM: true, reportUnavailable: true, resultTrue: true,
        rawFrameDiscrepancies: 0, firstRawDiscrepancies: [], fullPhaseArrays: 0, singlePhaseArrays: 0, otherPhaseArrays: 0 };
      if (jointMode) this.nativeStats.laneShapes = jointPeriods.map(() => ({ full: 0, single: 0, other: 0 }));
    }
  }
  nativeReply(message) { this.nativePort?.postMessage({ generation: this.generation, ...message }); }
  phaseSummary(reason) {
    this.nativeReply({ kind: 'phase-summary', id: this.nativeId, reason, ordinal: this.nativeOrdinal,
      anchor: this.nativeAnchor, cursor: this.nativeCursor, rawSnapshots: this.nativeRawSnapshots,
      stats: this.nativeStats, session: this.nativeSession });
  }
  phaseSnapshot(reason, frames, marker, values, fields = {}) {
    if (jointMode && this.nativeRawSnapshots >= 32) { this.retireNative('snapshot-limit'); return; }
    if (++this.nativeRawSnapshots > 32) { this.retireNative('snapshot-limit'); return; }
    this.nativeReply({ kind: 'phase-snapshot', id: this.nativeId, reason, ordinal: this.nativeOrdinal,
      rawGlobalFrame: currentFrame, actualFrames: frames, nativeBase: this.nativeCursor,
      markerLength: marker.length, markerValues: Array.from(marker),
      ...(jointMode ? { laneLengths: this.nativeJointValues.map(value => value?.length ?? null),
        laneValues: this.nativeJointValues.map(value => value != null && Number.isSafeInteger(value.length)
          && value.length <= 4096 ? Array.from(value) : null),
        laneTypes: this.nativeJointValues.map(value => value?.constructor?.name ?? null) }
        : { parameterLength: values.length, values: Array.from(values) }), ...fields });
  }
  checkJointPhase(frames, marker, end) {
    const session = this.nativeSession, frame = this.nativeCursor, arrays = this.nativeJointValues;
    const resets = [], horizons = [];
    for (let lane = 0; lane < 3; lane++) {
      const state = session.lanes[lane], values = arrays[lane];
      const first = Math.max(frame, session.startFrame), last = Math.min(end, state.horizonFrame);
      if (first < last) {
        if (!(values instanceof Float32Array) || values.length !== frames) {
          session.failureSample = { lane, parameterLength: values?.length ?? null, actualFrames: frames };
          return 'joint-shape';
        }
        const checked = phaseEvidence(values.subarray(first - frame, last - frame), last - first,
          first, session.startFrame, sampleRate, state.horizonFrame, state.period);
        if (checked === null) {
          session.failureSample = { lane, parameterLength: values.length, actualFrames: frames };
          for (let index = first - frame; index < last - frame; index++) {
            const sampleFrame = frame + index, value = values[index];
            const ideal = phaseIdeal(sampleFrame, session.startFrame, sampleRate, state.period);
            const phase = nativePhase(value, state.period), expectedPhase = (sampleFrame - session.startFrame) % state.period;
            const error = ideal === null ? null : Math.abs(value - ideal);
            if (phase !== expectedPhase || ideal === null || !(error < 0.25)) {
              session.failureSample = { lane, index, frame: sampleFrame, value, ideal, phase, expectedPhase, error };
              break;
            }
          }
          return 'joint-evidence';
        }
        state.checkedSamples += checked.checkedSamples;
        state.checkedCallbacks++;
        state.maxError = Math.max(state.maxError, checked.maxError);
        state.tailSamples += Math.max(0, last - Math.max(first, session.jointHorizonFrame));
        session.maxError = Math.max(session.maxError, checked.maxError);
        for (let cycle = 1; cycle < 3; cycle++) {
          const resetFrame = session.startFrame + cycle * state.period;
          if (first <= resetFrame && resetFrame < last) {
            const reset = { lane, cycle, frame: resetFrame, index: resetFrame - frame, ordinal: this.nativeOrdinal,
              interior: frame < resetFrame && resetFrame < end };
            state.resets.push(reset);
            resets.push(reset);
          }
        }
      }
    }
    const first = Math.max(frame, session.startFrame), last = Math.min(end, session.jointHorizonFrame);
    let joint = null;
    if (first < last) {
      const firstIndex = first - frame, lastIndex = last - frame - 1;
      const firstOffset = nativeJointOffset(arrays[0][firstIndex], arrays[1][firstIndex], arrays[2][firstIndex]);
      const lastOffset = nativeJointOffset(arrays[0][lastIndex], arrays[1][lastIndex], arrays[2][lastIndex]);
      joint = { firstIndex, lastIndex, firstOffset, lastOffset };
      if (firstOffset !== first - session.startFrame || lastOffset !== last - session.startFrame - 1) {
        session.failureSample = { ...joint, expectedFirst: first - session.startFrame,
          expectedLast: last - session.startFrame - 1 };
        return 'joint-crt';
      }
      session.checkedSamples += last - first;
      session.checkedCallbacks++;
      session.crtChecks += 2;
      session.firstCRT ??= firstOffset;
      session.lastCRT = lastOffset;
      if (session.checkedCallbacks === 1) {
        this.phaseSnapshot('phase-onset', frames, marker, arrays[0], { activeIndex: firstIndex, firstActive: first, joint });
        this.nativeReply({ kind: 'phase-progress', id: this.nativeId, ordinal: this.nativeOrdinal,
          checkedSamples: session.checkedSamples });
      }
    }
    if (end > session.jointHorizonFrame) session.jointHorizonRefused = true;
    for (let lane = 0; lane < 3; lane++) {
      const state = session.lanes[lane], values = arrays[lane];
      if (end > state.horizonFrame) {
        const index = Math.max(0, state.horizonFrame - frame);
        const held = values instanceof Float32Array && (values.length === 1 ? values[0] === 1
          : values.length === frames && values.subarray(index).every(value => value === 1));
        if (!held) { session.failureSample = { lane, index, horizonFrame: state.horizonFrame }; return 'joint-hold'; }
        state.holdFrames += end - Math.max(frame, state.horizonFrame);
        if (!state.horizonRefused) {
          state.horizonRefused = phaseEvidence(values, frames, frame, session.startFrame,
            sampleRate, state.horizonFrame, state.period) === null;
          horizons.push({ lane, frame: state.horizonFrame, index, ordinal: this.nativeOrdinal,
            refused: state.horizonRefused, holdFrames: state.holdFrames });
          if (!state.horizonRefused) return 'joint-horizon';
        }
      }
    }
    if (resets.length) this.phaseSnapshot('reset', frames, marker, arrays[0], { resets, joint });
    if (horizons.length) this.phaseSnapshot('horizon', frames, marker, arrays[0],
      { horizons, joint, jointHorizonRefused: session.jointHorizonRefused });
    return null;
  }
  retireNative(reason) {
    if (!this.nativePort) return;
    if (phaseMode && !this.nativeSession?.done) this.phaseSummary(reason);
    this.nativeReply({ kind: 'retired', reason, id: this.nativeId,
      pendingInvalidated: this.nativeWatch !== null || (phaseMode && this.nativeSession !== null), ordinal: this.nativeOrdinal });
    this.nativeWatch = null;
    if (phaseMode) this.nativeSession = null;
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
        if (jointMode && request.kind === 'joint-start') {
          const shape = Array.isArray(request.periods) && request.periods.length === 3
            && Array.isArray(request.horizonFrames) && request.horizonFrames.length === 3
            && Array.isArray(request.futureEligible) && request.futureEligible.length === 3
            && Number.isSafeInteger(request.startFrame) && request.startFrame > 0
            && [0, 1, 2].every(lane => request.periods[lane] === jointPeriods[lane]
              && Number.isSafeInteger(request.horizonFrames[lane])
              && request.horizonFrames[lane] === advanceFrame(request.startFrame, 3 * jointPeriods[lane])
              && typeof request.futureEligible[lane] === 'boolean')
            && request.jointHorizonFrame === request.horizonFrames[2];
          const reason = this.nativeAnchor === null ? 'origin' : this.nativeSession !== null ? 'phase-active'
            : !shape ? 'joint-range' : request.futureEligible.some(value => !value) ? 'joint-late'
            : request.startFrame <= this.nativeCursor ? 'joint-range' : null;
          if (reason) { this.nativeReply({ kind: 'refused', id: request.id, reason }); return; }
          this.nativeSession = { joint: true, startFrame: request.startFrame, periods: [...request.periods],
            horizonFrames: [...request.horizonFrames], jointHorizonFrame: request.jointHorizonFrame,
            futureEligible: [...request.futureEligible], lanes: jointPeriods.map((period, lane) => ({ period,
              horizonFrame: request.horizonFrames[lane], checkedSamples: 0, checkedCallbacks: 0, maxError: 0,
              resets: [], horizonRefused: false, holdFrames: 0, tailSamples: 0 })), checkedSamples: 0,
            checkedCallbacks: 0, maxError: 0, crtChecks: 0, firstCRT: null, lastCRT: null,
            jointHorizonRefused: false, closingReset: null, closing: null, done: false, failure: null };
          this.nativeReply({ kind: 'accepted', id: request.id, operation: request.kind,
            phase: this.phase, reportAvailable: this.owner.report_word(0, false), ordinal: this.nativeOrdinal,
            nativeCursor: this.nativeCursor });
          return;
        }
        if (phaseMode && !jointMode && request.kind === 'phase-start') {
          const reason = this.nativeAnchor === null ? 'origin' : this.nativeSession !== null ? 'phase-active'
            : request.period !== 1048576 || !Number.isSafeInteger(request.startFrame)
              || request.startFrame <= this.nativeCursor || !Number.isSafeInteger(request.horizonFrame)
              || request.horizonFrame - request.startFrame !== 3 * request.period ? 'phase-range' : null;
          if (reason) { this.nativeReply({ kind: 'refused', id: request.id, reason }); return; }
          this.nativeSession = { startFrame: request.startFrame, horizonFrame: request.horizonFrame, period: request.period,
            checkedSamples: 0, checkedCallbacks: 0, maxError: 0, resets: [], horizonRefused: false,
            holdFrames: 0, closingReset: null, closing: null, done: false, failure: null };
          this.nativeReply({ kind: 'accepted', id: request.id, operation: request.kind,
            phase: this.phase, reportAvailable: this.owner.report_word(0, false), ordinal: this.nativeOrdinal,
            nativeCursor: this.nativeCursor });
          return;
        }
        if (request.kind === 'watch' && (!Number.isSafeInteger(request.targetFrame) || request.targetFrame <= 0)) {
          this.nativeReply({ kind: 'refused', id: request.id, reason: 'target' }); return;
        }
        if (request.kind !== 'watch' && request.kind !== 'inspect') {
          this.nativeReply({ kind: 'refused', id: request.id, reason: 'operation' }); return;
        }
        if (phaseMode && request.kind === 'watch' && request.closing
          && (!Number.isSafeInteger(request.resetFrame) || request.resetFrame <= this.nativeCursor
            || request.resetFrame >= request.targetFrame || request.futureEligible !== true || this.nativeSession === null)) {
          this.nativeReply({ kind: 'refused', id: request.id, reason: 'closing-prerequisite' }); return;
        }
        if (request.kind === 'watch') this.nativeWatch = { id: request.id, targetFrame: request.targetFrame,
          ...(phaseMode ? { initial: request.initial === true, closing: request.closing === true,
            resetFrame: request.resetFrame, futureEligible: request.futureEligible === true, startedOrdinal: this.nativeOrdinal } : {}) };
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
    if (++this.nativeOrdinal > (phaseMode ? 32768 : 1024)) { this.retireNative('callback-limit'); return result; }
    const frames = outputs[0]?.[0]?.length ?? 0;
    const values = parameters.__native_marker;
    const targetFrame = this.nativeWatch?.targetFrame ?? null;
    const candidate = targetFrame === null ? null : markerFrame(values, frames, targetFrame);
    let edge = 0;
    while (edge < values.length && values[edge] === 0) edge++;
    const zeroPCM = outputs.every(bus => bus.every(channel => channel.every(value => value === 0)));
    if (phaseMode) {
      if (jointMode) this.nativeJointValues = [parameters.__native_joint_phase_0,
        parameters.__native_joint_phase_1, parameters.__native_joint_phase_2];
      const phaseValues = jointMode ? this.nativeJointValues[0] : parameters.__native_phase;
      const stats = this.nativeStats;
      stats.callbacks++;
      stats.observedFrames += frames;
      stats.minFrames = stats.minFrames === null ? frames : Math.min(stats.minFrames, frames);
      stats.maxFrames = Math.max(stats.maxFrames, frames);
      stats.phase0 &&= this.phase === 0;
      stats.zeroPCM &&= zeroPCM;
      stats.reportUnavailable &&= this.owner.report_word(0, false) === 0;
      stats.resultTrue &&= result === true;
      if (phaseValues?.length === frames) stats.fullPhaseArrays++;
      else if (phaseValues?.length === 1) stats.singlePhaseArrays++;
      else stats.otherPhaseArrays++;
      if (jointMode) for (let lane = 0; lane < 3; lane++) {
        const values = this.nativeJointValues[lane], shape = stats.laneShapes[lane];
        if (values instanceof Float32Array && values.length === frames) shape.full++;
        else if (values instanceof Float32Array && values.length === 1) shape.single++;
        else shape.other++;
      }
      if (this.nativeAnchor === null && this.nativeWatch?.initial && candidate !== null) {
        this.nativeAnchor = { base: candidate, ordinal: this.nativeOrdinal, targetFrame, edge,
          actualFrames: frames, values: Array.from(values) };
        this.nativeCursor = candidate;
      }
      if (this.nativeCursor !== null && currentFrame !== this.nativeCursor) {
        stats.rawFrameDiscrepancies++;
        if (stats.firstRawDiscrepancies.length < 8) stats.firstRawDiscrepancies.push({ ordinal: this.nativeOrdinal,
          rawGlobalFrame: currentFrame, nativeBase: this.nativeCursor, actualFrames: frames });
      }
      const session = this.nativeSession;
      if (session && !session.done) {
        const frame = this.nativeCursor;
        const end = advanceFrame(frame, frames);
        let failure = end === null || frames < 1 ? 'extent' : null;
        if (session.joint) {
          if (!failure) failure = this.checkJointPhase(frames, values, end);
        } else {
        const first = Math.max(frame, session.startFrame), last = Math.min(end, session.horizonFrame);
        if (!failure && first < last) {
          const checked = phaseValues.length === frames
            ? phaseEvidence(phaseValues.subarray(first - frame, last - frame), last - first, first,
              session.startFrame, sampleRate, session.horizonFrame, session.period) : null;
          if (checked === null) {
            failure = 'phase-evidence';
            session.failureSample = { parameterLength: phaseValues.length, actualFrames: frames };
            if (phaseValues.length === frames) for (let index = first - frame; index < last - frame; index++) {
              const sampleFrame = frame + index, value = phaseValues[index];
              const ideal = phaseIdeal(sampleFrame, session.startFrame, sampleRate, session.period);
              const phase = nativePhase(value, session.period), expectedPhase = (sampleFrame - session.startFrame) % session.period;
              const error = ideal === null ? null : Math.abs(value - ideal);
              if (phase !== expectedPhase || ideal === null || !(error < 0.25)) {
                session.failureSample = { index, frame: sampleFrame, value, ideal, phase, expectedPhase, error };
                break;
              }
            }
          }
          else {
            session.checkedSamples += checked.checkedSamples;
            session.checkedCallbacks++;
            session.maxError = Math.max(session.maxError, checked.maxError);
            for (let cycle = 1; cycle < 3; cycle++) {
              const resetFrame = session.startFrame + cycle * session.period;
              if (first <= resetFrame && resetFrame < last) {
                const reset = { frame: resetFrame, index: resetFrame - frame, ordinal: this.nativeOrdinal,
                  interior: frame < resetFrame && resetFrame < end };
                session.resets.push(reset);
                this.phaseSnapshot('reset', frames, values, phaseValues, { reset });
              }
            }
            if (session.checkedCallbacks === 1) {
              this.phaseSnapshot('phase-onset', frames, values, phaseValues, { activeIndex: first - frame, firstActive: first });
              this.nativeReply({ kind: 'phase-progress', id: this.nativeId,
                ordinal: this.nativeOrdinal, checkedSamples: session.checkedSamples });
            }
          }
        }
        if (!failure && end > session.horizonFrame) {
          const holdIndex = Math.max(0, session.horizonFrame - frame);
          const held = phaseValues.length === 1 ? phaseValues[0] === 1
            : phaseValues.length === frames && phaseValues.subarray(holdIndex).every(value => value === 1);
          if (!held) failure = 'horizon-hold';
          else {
            session.holdFrames += end - Math.max(frame, session.horizonFrame);
            if (!session.horizonRefused) {
              session.horizonRefused = phaseEvidence(phaseValues, frames, frame, session.startFrame,
                sampleRate, session.horizonFrame, session.period) === null;
              this.phaseSnapshot('horizon', frames, values, phaseValues, { holdIndex, refused: session.horizonRefused });
              if (!session.horizonRefused) failure = 'horizon-admission';
            }
          }
        }
        }
        if (!failure && this.nativeWatch?.closing) {
          if (session.closingReset === null) {
            const zeroIndex = values.length === 1 ? (values[0] === 0 ? 0 : -1)
              : values.findIndex((value, index) => value === 0 && frame + index >= this.nativeWatch.resetFrame);
            if (zeroIndex >= 0 && frame + zeroIndex >= this.nativeWatch.resetFrame) {
              session.closingReset = { ordinal: this.nativeOrdinal, nativeBase: frame, index: zeroIndex,
                resetFrame: this.nativeWatch.resetFrame };
              this.phaseSnapshot('closing-reset', frames, values, phaseValues, { reset: session.closingReset });
            }
          }
          if (candidate !== null) {
            session.closing = { ordinal: this.nativeOrdinal, targetFrame, candidate, expectedBase: frame,
              intervalCallbacks: this.nativeOrdinal - this.nativeAnchor.ordinal,
              intervalFrames: frame - this.nativeAnchor.base,
              futureEligible: this.nativeWatch.futureEligible, resetObserved: session.closingReset !== null };
            this.phaseSnapshot('closing-marker', frames, values, phaseValues, { closing: session.closing });
            if (!session.closing.futureEligible || !session.closing.resetObserved || candidate !== frame) failure = 'closing-marker';
          }
        }
        if (failure) {
          session.failure = { reason: failure, ordinal: this.nativeOrdinal, frame,
            checkedSamples: session.checkedSamples, maxError: session.maxError };
          this.phaseSnapshot('first-failure', frames, values, phaseValues, { failure: session.failure });
          this.retireNative('phase-failure');
          return result;
        }
        if (session.closing !== null && (session.joint
          ? session.lanes.every(lane => lane.horizonRefused && lane.holdFrames > 0) : session.holdFrames > 0)) {
          session.done = true;
          this.phaseSummary('phase-complete');
        }
      }
      const warmup = jointMode ? 2 : 8;
      if (this.nativeOrdinal <= warmup || this.nativeOrdinal <= (this.nativeWatch?.startedOrdinal ?? -warmup) + warmup || candidate !== null) {
        if (jointMode && this.nativeRawSnapshots >= 32) { this.retireNative('snapshot-limit'); return result; }
        if (++this.nativeRawSnapshots > 32) { this.retireNative('snapshot-limit'); return result; }
        this.nativeReply({ kind: 'callback', id: this.nativeWatch?.id ?? this.nativeId,
          ordinal: this.nativeOrdinal, rawGlobalFrame: currentFrame, actualFrames: frames,
          parameterLength: values.length, edge, spatialCandidate: candidate,
          shape: values.every(value => value === 0) ? 'zero' : values.every(value => value === 1) ? 'one' : 'varying',
          values: Array.from(values),
          ...(jointMode ? { laneLengths: this.nativeJointValues.map(value => value?.length ?? null),
            laneValues: this.nativeJointValues.map(value => value != null && Number.isSafeInteger(value.length)
              && value.length <= 4096 ? Array.from(value) : null),
            laneTypes: this.nativeJointValues.map(value => value?.constructor?.name ?? null) }
            : { phaseParameterLength: phaseValues.length, phaseValues: Array.from(phaseValues) }),
          nativeBase: this.nativeCursor, phase: this.phase, result, zeroPCM, reportAvailable: this.owner.report_word(0, false) });
      }
      if (this.nativeOrdinal % 4096 === 0) this.nativeReply({ kind: 'phase-progress', id: this.nativeId,
        ordinal: this.nativeOrdinal, checkedSamples: session?.checkedSamples ?? 0 });
      if (this.nativeCursor !== null) {
        this.nativeCursor = advanceFrame(this.nativeCursor, frames);
        if (this.nativeCursor === null) this.retireNative('extent-overflow');
      }
      return result;
    }
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
  const webRoot = nativeFixture ? await realpath(resolve(root, "app/web")) : null;
  server = createServer({ cert: await readFile(cert), key: await readFile(key) }, async (request, response) => {
    try {
      const pathname = new URL(request.url, "https://localhost").pathname;
      const path = resolve(root, `.${decodeURIComponent(pathname)}`);
      const within = relative(root, path);
      if (within === ".." || within.startsWith(`..${sep}`)) throw Error("Outside repository");
      if (nativeFixture && pathname !== "/__chronology.html") {
        const lexical = relative(resolve(root, "app/web"), path);
        if (lexical === ".." || lexical.startsWith(`..${sep}`)) throw Error("Outside web fixture");
        const resolved = relative(webRoot, await realpath(path));
        if (resolved === ".." || resolved.startsWith(`..${sep}`)) throw Error("Outside resolved web fixture");
      }
      const body = pathname === "/__chronology.html" ? "<!doctype html><title>Worklet chronology evidence</title>"
        : pathname === "/app/web/audio-worklet.js" ? (nativeFixture ? nativeBootstrap : bootstrap) + production : await readFile(path);
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
  const caseStarted = performance.now();
  const consoleMessages = [], pageErrors = [];
  let page, result, controllerTimer, controllerExpired = false, deadlineError;
  const closePhasePage = async ownedPage => {
    let timer;
    try {
      await Promise.race([ownedPage.close(), new Promise((_, no) => {
        timer = setTimeout(() => no(Error("Owned phase page close deadline exceeded")), 3000);
      })]);
    } finally { clearTimeout(timer); }
  };
  const controllerDeadline = spec.phaseProbe ? new Promise((_, no) => {
    controllerTimer = setTimeout(() => {
      controllerExpired = true;
      deadlineError = Error("Native phase case120s controller watchdog exceeded");
      deadlineError.cleanup = { pageAcquisitionTimedOut: page === undefined,
        latePageReceived: false, pageClosed: false, pageError: null };
      no(deadlineError);
    }, 120000);
  }) : null;
  try {
    const acquisition = browser.newPage().then(ownedPage => {
      if (spec.phaseProbe && controllerExpired) {
        deadlineError.cleanup.latePageReceived = true;
        closePhasePage(ownedPage).then(() => { deadlineError.cleanup.pageClosed = true; },
          error => { deadlineError.cleanup.pageError = String(error); });
      }
      return ownedPage;
    });
    page = await (spec.phaseProbe ? Promise.race([acquisition, controllerDeadline]) : acquisition);
    page.on("console", message => { if (consoleMessages.length < 32) consoleMessages.push({ type: message.type(), text: message.text().slice(0, 2000) }); });
    page.on("pageerror", error => { if (pageErrors.length < 16) pageErrors.push(String(error).slice(0, 2000)); });
    const navigation = page.goto(`https://127.0.0.1:${port}/__chronology.html`, ...(spec.phaseProbe ? [{ timeout: Math.max(1, 120000 - (performance.now() - caseStarted)) }] : []));
    await (spec.phaseProbe ? Promise.race([navigation, controllerDeadline]) : navigation);
    const evaluation = page.evaluate(async ({ spec, generation, caseBudgetMs }) => {
      let AudioHost, markerTime, advanceFrame, module;
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
      let caseTimer, caseExpired = false;
      const wait = async (predicate, timeoutMs = 5000, workload = false, cleanup = false) => {
        const deadline = performance.now() + timeoutMs;
        while (!predicate()) {
          if (spec.phaseProbe && caseExpired && !cleanup) throw Error("Native phase case deadline exceeded");
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
          if (replies.length >= (spec.phaseProbe ? 128 : 1100)) throw Error("Private probe reply bound exceeded");
          replies.push(event.data);
          if (channel.port1 === privatePort) {
            if (messages.length >= (spec.phaseProbe ? 128 : 1100)) throw Error("Native snapshot bound exceeded");
            messages.push(event.data);
            if (event.data.kind === "retired") observationRetired = true;
          }
        };
        channel.port1.start();
        nodes[0].port.postMessage({ kind: "__native-attach", generation: attachGeneration, id: attachId, port: channel.port2 }, [channel.port2]);
        await wait(() => replies.some(x => x.kind === "attached" || x.kind === "refused"));
        return { channel, replies };
      };
      const request = async (kind, fields = {}, cleanup = false) => {
        const requestId = ++id;
        privatePort.postMessage({ kind, generation, id: requestId, ...fields });
        await wait(() => messages.some(x => x.id === requestId && (spec.phaseProbe
          ? ["accepted", "refused", "retired"].includes(x.kind) : x.kind !== "callback")), cleanup ? 3000 : 5000, false, cleanup);
        return requestId;
      };
      const watch = (targetFrame, fields = {}) => request("watch", { targetFrame, ...fields });
      const set = (value, targetFrame) => {
        if (spec.phaseProbe && caseExpired) throw Error("Native phase case deadline exceeded");
        const time = markerTime(targetFrame, host.sampleRate);
        if (time === null) throw Error("Ambiguous scheduled marker time");
        const before = contexts[0].currentTime;
        nodes[0].parameters.get("__native_marker").setValueAtTime(value, time);
        events.push({ value, targetFrame, time, beforeContextTime: before, afterContextTime: contexts[0].currentTime,
          ...(spec.phaseProbe ? { parameter: "__native_marker", method: "setValueAtTime" } : {}) });
        return time;
      };
      try {
        const operation = (async () => {
        ({ AudioHost } = await import("/app/web/audio-host.mjs"));
        ({ markerTime, advanceFrame } = await import("/app/web/worklet-native-frame-probe.mjs"));
        module = await WebAssembly.compile(await (await fetch("/app/web/audio-pkg/beatkernel_bms_runtime_bg.wasm")).arrayBuffer());
        if (spec.phaseProbe && caseExpired) throw Error("Native phase case deadline exceeded");
        host = await AudioHost.open({ module, generation, channels: 2, timeoutMs: 5000,
          contextOptions: { latencyHint: "interactive", sampleRate: spec.rate },
          pcmLimits: { maxAssetBytes: 65536, maxTotalBytes: 131072, maxSamples: 4 },
          audioLimits: { queueCapacity: 32, maxVoices: 8, pendingCapacity: 32, maxFrames: 128, maxCommandsPerRender: 32 } });
        result.sampleRate = host.sampleRate;
        result.beforeReport = await host.poll();
        result.prerequisites.nativeDefault = nodes[0].parameters.get("__native_marker").defaultValue;
        if (spec.jointProbe) result.prerequisites.jointDefaults = [0, 1, 2].map(lane => nodes[0].parameters.get("__native_joint_phase_" + lane).defaultValue);
        else if (spec.phaseProbe) result.prerequisites.phaseDefault = nodes[0].parameters.get("__native_phase").defaultValue;
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
        if (spec.phaseProbe) {
          const period = 2 ** 20;
          const phaseSet = (method, value, targetFrame, parameter = "__native_phase") => {
            if (caseExpired) throw Error("Native phase case deadline exceeded");
            const time = markerTime(targetFrame, host.sampleRate);
            if (time === null) throw Error("Ambiguous native phase event time");
            const before = contexts[0].currentTime;
            nodes[0].parameters.get(parameter)[method](value, time);
            events.push({ parameter, method, value, targetFrame, time,
              beforeContextTime: before, afterContextTime: contexts[0].currentTime });
            return time;
          };
          const openMarker = async () => {
            if (spec.offset >= quantum) throw Error("Native phase requires an actual interior marker");
            const firstTarget = futureTarget(spec.offset);
            const firstId = await watch(firstTarget, { initial: true });
            result.firstTarget = firstTarget;
            result.firstTime = set(1, firstTarget);
            await wait(() => messages.some(x => x.kind === "callback" && x.id === firstId && x.spatialCandidate !== null), 5000, true);
            result.first = messages.find(x => x.kind === "callback" && x.id === firstId && x.spatialCandidate !== null);
            result.firstAdmission = { eventCount: events.length, previousEvents: false,
              eligible: events.length === 1 && events[0].value === 1 && result.prerequisites.nativeDefault === 0 };
            if (!result.firstAdmission.eligible) throw Error("Native phase opening marker has a contaminated timeline");
          };
          const openPhase = async () => {
            await openMarker();
            const startFrame = futureTarget(spec.offset), horizonFrame = startFrame + 3 * period;
            result.phaseSchedule = { period, periods: 3, startFrame, horizonFrame };
            const startId = await request("phase-start", result.phaseSchedule);
            const admitted = messages.find(x => x.id === startId && x.kind !== "callback");
            if (admitted.kind !== "accepted") throw Error("Native phase start refused: " + admitted.reason);
            phaseSet("setValueAtTime", 1, startFrame);
            for (let cycle = 1; cycle <= 3; cycle++) {
              const target = startFrame + cycle * period;
              phaseSet("linearRampToValueAtTime", period + 1, target);
              phaseSet("setValueAtTime", 1, target);
            }
            const phaseEvents = events.filter(x => x.parameter === "__native_phase");
            result.phaseSchedule.futureEligible = phaseEvents.every(x => x.beforeContextTime < x.time && x.afterContextTime < x.time);
            if (!result.phaseSchedule.futureEligible) throw Error("Native phase schedule was already late");
          };
          const jointPeriods = [1048576, 1048575, 1048573];
          const jointGeometry = startFrame => ({ periods: [...jointPeriods], startFrame,
            horizonFrames: jointPeriods.map(period => startFrame + 3 * period),
            jointHorizonFrame: startFrame + 3 * jointPeriods[2], futureEligible: [true, true, true] });
          const submitJointLane = (lane, startFrame) => {
            const period = jointPeriods[lane], name = "__native_joint_phase_" + lane;
            phaseSet("setValueAtTime", 1, startFrame, name);
            for (let cycle = 1; cycle <= 3; cycle++) {
              const target = startFrame + cycle * period;
              phaseSet("linearRampToValueAtTime", period + 1, target, name);
              phaseSet("setValueAtTime", 1, target, name);
            }
          };
          const openJoint = async (omitLane = false, lateLane = false) => {
            await openMarker();
            const startFrame = futureTarget(spec.offset);
            result.jointSchedule = jointGeometry(startFrame);
            submitJointLane(0, startFrame);
            submitJointLane(1, startFrame);
            let watched;
            if (lateLane) {
              await wait(() => contexts[0].currentTime > markerTime(startFrame, host.sampleRate) + 0.025, 5000, true);
              submitJointLane(2, startFrame);
              watched = await watch(startFrame, { initial: false });
              result.lateWatchId = watched;
            } else if (!omitLane) submitJointLane(2, startFrame);
            result.jointSchedule.futureEligible = jointPeriods.map((_, lane) => events
              .filter(x => x.parameter === "__native_joint_phase_" + lane)
              .every(x => x.beforeContextTime < x.time && x.afterContextTime < x.time));
            const startId = await request("joint-start", result.jointSchedule);
            const reply = messages.find(x => x.id === startId && ["accepted", "refused"].includes(x.kind));
            if (lateLane) {
              result.lateRefusal = reply;
              if (reply.kind !== "refused" || reply.reason !== "joint-late") throw Error("Late joint lane was not refused");
              await wait(() => messages.filter(x => x.kind === "callback" && x.id === watched).length >= 2);
              result.lateCallbacks = messages.filter(x => x.kind === "callback" && x.id === watched);
              result.certifiedJointOffset = null;
            } else if (reply.kind !== "accepted") throw Error("Joint start refused: " + reply.reason);
          };
          if (spec.jointProbe && ["constant", "late"].includes(spec.operation)) {
            await openJoint(spec.operation === "constant", spec.operation === "late");
            if (spec.operation === "constant") {
              await wait(() => messages.some(x => x.kind === "phase-summary"), 5000);
              result.summary = messages.findLast(x => x.kind === "phase-summary");
              result.certifiedJointOffset = null;
            }
          } else if (spec.operation === "stream" || ["timeout", "stop", "suspend"].includes(spec.operation)) {
            if (spec.jointProbe) await openJoint(); else await openPhase();
            if (spec.operation === "stream") {
              const horizonFrame = spec.jointProbe ? result.jointSchedule.horizonFrames[0] : result.phaseSchedule.horizonFrame;
              const resetFrame = horizonFrame + (spec.jointProbe ? quantum : -2 * quantum);
              const targetFrame = horizonFrame + (spec.jointProbe ? 3 * quantum : 2 * quantum);
              const resetTime = markerTime(resetFrame, host.sampleRate), onsetTime = markerTime(targetFrame, host.sampleRate);
              const before = contexts[0].currentTime;
              if (resetTime === null || onsetTime === null || before >= resetTime || before >= onsetTime) throw Error("Closing marker is not future eligible");
              await watch(targetFrame, { closing: true, resetFrame, futureEligible: true });
              result.resetTime = set(0, resetFrame);
              result.secondTime = set(1, targetFrame);
              result.secondTarget = targetFrame;
              const closingEvents = events.filter(x => x.parameter === "__native_marker").slice(1);
              result.closingAdmission = { resetFrame, targetFrame, eventCount: closingEvents.length,
                futureEligible: closingEvents.length === 2 && closingEvents.every(x => x.beforeContextTime < x.time && x.afterContextTime < x.time) };
              if (!result.closingAdmission.futureEligible) throw Error("Closing reset/onset submission was late");
              await wait(() => messages.some(x => x.kind === "phase-summary"), 120000, true);
              result.summary = messages.findLast(x => x.kind === "phase-summary");
              if (result.summary.reason !== "phase-complete") throw Error("Native phase failed: " + JSON.stringify(result.summary.session?.failure ?? result.summary.reason));
            } else {
              await wait(() => messages.some(x => x.kind === "phase-progress" && x.checkedSamples > 0)
                || messages.some(x => x.kind === "phase-summary"), 5000, true);
              if (messages.some(x => x.kind === "phase-summary")) throw Error("Native phase retired before active retirement exercise");
              if (spec.operation === "timeout") {
                try { await wait(() => messages.some(x => x.kind === "phase-summary" && x.reason === "phase-complete"), 40); }
                catch (error) { result.timeout = String(error); }
                if (!result.timeout) throw Error("Finite phase completed before the intended timeout");
                await request("retire", { reason: "timeout" });
                const rejected = await attach(generation, id + 1);
                result.retiredAttachment = rejected.replies[0];
                rejected.channel.port1.close(); rejected.channel.port2.close();
              } else if (spec.operation === "stop") {
                await bounded(host.stop(), "phase stop");
                await wait(() => observationRetired);
              } else {
                await request("retire", { reason: "suspend" });
                await bounded(contexts[0].suspend(), "phase suspend", 5000);
                await wait(() => stateEvents.some(event => event.state === "suspended") && host.state === "failed");
                result.suspendedState = stateEvents.findLast(event => event.state === "suspended").state;
                try { await host.poll(); }
                catch (error) { result.suspendFailure = { code: error.code, operation: error.operation, message: String(error) }; }
              }
              result.summary = messages.findLast(x => x.kind === "phase-summary");
            }
          } else if (spec.operation === "identities") {
            if (spec.jointProbe) {
              await request("joint-start", jointGeometry(futureTarget(spec.offset)));
              await openMarker();
            }
            const stale = await attach(generation - 1, id + 1), repeated = await attach(generation, id), duplicate = await attach(generation, id + 1);
            result.rejectedAttachments = [stale.replies[0], repeated.replies[0], duplicate.replies[0]];
            for (const rejected of [stale, repeated, duplicate]) { rejected.channel.port1.close(); rejected.channel.port2.close(); }
            privatePort.postMessage({ kind: "inspect", generation: generation - 1, id: id + 1 });
            privatePort.postMessage({ kind: "inspect", generation, id });
            await wait(() => messages.filter(x => x.kind === "refused").length === (spec.jointProbe ? 3 : 2));
            await request("watch", { targetFrame: -1 });
            await request("unknown");
            if (spec.jointProbe) {
              const geometry = jointGeometry(futureTarget(spec.offset));
              await request("joint-start", { ...geometry, periods: [1048575, 1048576, 1048573] });
              await request("joint-start", { ...geometry, horizonFrames: [Number.MAX_SAFE_INTEGER + 1, ...geometry.horizonFrames.slice(1)] });
              await request("joint-start", { ...geometry, futureEligible: [true, true, false] });
            } else await request("phase-start", { startFrame: 1, horizonFrame: 1 + 3 * period, period });
            await watch(futureTarget(spec.offset), { closing: true, resetFrame: 1, futureEligible: true });
            await request("inspect");
            result.retainedIdentity = id;
            result.certifiedOrigin = null;
          } else if (spec.operation === "late") {
            await wait(() => contexts[0].currentTime > 0.05);
            const target = Math.max(1, Math.floor((contexts[0].currentTime - 0.025) * host.sampleRate));
            const watched = await watch(target, { initial: false });
            result.negativeTarget = target;
            result.negativeTime = set(1, target);
            phaseSet("setValueAtTime", 1, target);
            phaseSet("linearRampToValueAtTime", period + 1, target + period);
            const refusedId = await request("phase-start", { startFrame: target, horizonFrame: target + 3 * period, period });
            result.lateRefusal = messages.find(x => x.id === refusedId && x.kind === "refused");
            result.firstAdmission = { eventCount: events.length, previousEvents: false, eligible: false };
            await wait(() => messages.some(x => x.kind === "callback" && x.id === watched && x.shape === "one"));
            result.certifiedOrigin = null;
            result.rawSpatialCandidates = messages.filter(x => x.kind === "callback" && x.id === watched && x.spatialCandidate !== null);
          } else {
            const watched = await watch(futureTarget(spec.offset), { initial: false });
            if (spec.operation === "constant") {
              await wait(() => messages.filter(x => x.kind === "callback" && x.id === watched).length >= 8);
              result.constantCallbacks = messages.filter(x => x.kind === "callback" && x.id === watched);
            } else if (spec.operation === "callback-limit") await wait(() => observationRetired, 120000);
            result.certifiedOrigin = null;
          }
        } else if (positive) {
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
        })();
        if (spec.phaseProbe) {
          await Promise.race([operation, new Promise((_, no) => {
            caseTimer = setTimeout(() => { caseExpired = true; no(Error("Native phase case120s watchdog exceeded")); }, caseBudgetMs);
          })]);
        } else await operation;
      } catch (error) {
        result.unexpected = { message: String(error), stack: error.stack };
      } finally {
        clearTimeout(caseTimer);
        if (spec.phaseProbe) {
          const errors = [];
          const attempt = async (operation, action, timeoutMs) => {
            try { await bounded(action(), operation, timeoutMs); }
            catch (error) { errors.push({ operation, error: String(error) }); }
          };
          if (privatePort && !observationRetired) await attempt("private retirement", () => request("retire", { reason: "case-complete" }, true), 3000);
          if (host) await attempt("owned Host stop", () => host.stop(), 8000);
          for (const context of contexts) if (context.state !== "closed") await attempt("owned context close", () => context.close(), 3000);
          for (const ownedPort of ports) {
            try { ownedPort.onmessage = null; ownedPort.close(); }
            catch (error) { errors.push({ operation: "owned port close", error: String(error) }); }
          }
          result.cleanup.errors = errors;
          if (errors.length) result.cleanup.error = JSON.stringify(errors);
          result.cleanup.contextsConstructed = contexts.length;
          result.cleanup.nodesConstructed = nodes.length;
          result.cleanup.contexts = contexts.map(context => context.state);
          result.cleanup.contextsClosed = contexts.every(context => context.state === "closed");
          result.cleanup.ownedPortEndpointsClosed = ports.length;
          result.cleanup.observationRetired = observationRetired;
          result.cleanup.pendingInvalidated = messages.findLast(x => x.kind === "retired")?.pendingInvalidated ?? false;
          result.summary ??= messages.findLast(x => x.kind === "phase-summary");
        } else {
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
      }
      return result;
    }, { spec, generation, caseBudgetMs: spec.phaseProbe ? Math.max(1, 120000 - (performance.now() - caseStarted)) : null });
    result = await (spec.phaseProbe ? Promise.race([evaluation, controllerDeadline]) : evaluation);
    return { ...result, consoleMessages, pageErrors };
  } finally {
    clearTimeout(controllerTimer);
    if (spec.phaseProbe && page) {
      try {
        await closePhasePage(page);
        if (result) result.cleanup.pageClosed = true;
        if (deadlineError) deadlineError.cleanup.pageClosed = true;
      } catch (error) {
        if (result) result.cleanup.pageError = String(error);
        else if (deadlineError) deadlineError.cleanup.pageError = String(error);
        else throw error;
      }
    } else if (page) await page.close();
  }
}

function verifyPhase(result) {
  assert.equal(result.pageErrors.length, 0, "Unexpected phase probe page error");
  assert(!result.unexpected, JSON.stringify(result.unexpected));
  assert(!result.cleanup.error, result.cleanup.error);
  assert(!result.cleanup.pageError, result.cleanup.pageError);
  assert(result.cleanup.pageClosed && result.cleanup.contextsClosed && result.cleanup.observationRetired,
    "Owned phase page/context/observer did not close");
  assert.equal(result.cleanup.contextsConstructed, 1);
  assert.equal(result.cleanup.nodesConstructed, 1);
  assert(result.cleanup.ownedPortEndpointsClosed >= 2);
  assert.equal(result.prerequisites.nativeDefault, 0);
  if (result.jointProbe) assert.deepEqual(result.prerequisites.jointDefaults, [0, 0, 0]);
  else assert.equal(result.prerequisites.phaseDefault, 0);
  assert.equal(result.prerequisites.valueWrites, 0);
  assert.equal(result.beforeReport.available, false);
  if (result.afterReport) assert.equal(result.afterReport.available, false);
  assert.equal(result.sampleRate, result.rate);
  assert(result.hostMessages.every(x => ["ready", "ack", "terminal"].includes(x.kind)));
  assert(result.messages.length <= 128);
  const summary = result.summary;
  assert(summary, "No bounded Worklet phase summary");
  assert(summary.rawSnapshots > 0 && summary.rawSnapshots <= 32);
  const stats = summary.stats;
  assert(stats.callbacks > 0 && stats.callbacks <= 32768);
  assert(stats.phase0 && stats.zeroPCM && stats.reportUnavailable && stats.resultTrue,
    "Phase observer changed production phase0 behavior");
  assert(stats.minFrames > 0 && stats.maxFrames <= 128);
  assert.equal(stats.fullPhaseArrays + stats.singlePhaseArrays + stats.otherPhaseArrays, stats.callbacks);
  assert(stats.firstRawDiscrepancies.length <= 8);
  const callbacks = result.messages.filter(x => x.kind === "callback");
  assert(callbacks.length > 0 && callbacks.length <= 32);
  for (const callback of callbacks) {
    assert.equal(callback.generation, result.generation);
    assert.equal(callback.phase, 0);
    assert.equal(callback.zeroPCM, true);
    assert.equal(callback.result, true);
    assert.equal(callback.reportAvailable, 0);
  }
  if (result.jointProbe) { verifyJoint(result, summary, stats, callbacks); return; }
  const session = summary.session;
  if (result.operation === "stream" || ["timeout", "stop", "suspend"].includes(result.operation)) {
    assert(result.firstAdmission.eligible && result.firstAdmission.eventCount === 1);
    assert.equal(result.first.edge, result.offset);
    assert.equal(markerFrame(new Float32Array(result.first.values), result.first.actualFrames, result.firstTarget), result.first.spatialCandidate);
    assert.equal(summary.anchor.base, result.first.spatialCandidate);
    assert.equal(summary.anchor.ordinal, result.first.ordinal);
    assert.equal(result.phaseSchedule.period, 2 ** 20);
    assert.equal(result.phaseSchedule.periods, 3);
    assert.equal(result.phaseSchedule.horizonFrame - result.phaseSchedule.startFrame, 3 * 2 ** 20);
    assert(result.phaseSchedule.futureEligible);
    assert(session && session.checkedSamples > 0 && session.failure === null,
      "No valid active phase samples: " + JSON.stringify(session?.failureSample ?? session?.failure));
    assert(session.maxError < 0.25);
    if (result.operation === "stream") {
      assert.equal(summary.reason, "phase-complete");
      assert(session.done && session.horizonRefused && session.holdFrames > 0);
      assert.equal(session.checkedSamples, 3 * 2 ** 20, "The finite three-period interval lost or duplicated actual samples");
      assert.equal(session.resets.length, 2);
      assert(session.resets.every(x => x.interior), "No actual interior reset evidence");
      assert(result.closingAdmission.futureEligible && result.closingAdmission.eventCount === 2);
      assert(session.closingReset !== null && session.closing.futureEligible && session.closing.resetObserved);
      assert.equal(session.closing.candidate, session.closing.expectedBase, "Closing contradiction must fail without rebasing");
      assert.equal(session.closing.intervalCallbacks, session.closing.ordinal - summary.anchor.ordinal);
      assert.equal(session.closing.intervalFrames, session.closing.expectedBase - summary.anchor.base);
      const snapshots = result.messages.filter(x => x.kind === "phase-snapshot");
      const onset = snapshots.find(x => x.reason === "phase-onset");
      assert(onset && onset.activeIndex > 0 && onset.activeIndex < onset.actualFrames, "No actual interior phase onset");
      assert.equal(onset.parameterLength, onset.actualFrames);
      assert.equal(onset.firstActive, session.startFrame);
      assert(onset.values.slice(0, onset.activeIndex).every(value => value === 0), "Fresh phase parameter lost its default0 prefix");
      const onsetEvidence = phaseEvidence(new Float32Array(onset.values).subarray(onset.activeIndex),
        onset.actualFrames - onset.activeIndex, session.startFrame, session.startFrame, result.sampleRate,
        session.horizonFrame, session.period);
      assert(onsetEvidence && onsetEvidence.firstPhase === 0 && onsetEvidence.maxError < 0.25,
        "Independent actual native onset recomputation failed");
      assert.equal(snapshots.filter(x => x.reason === "reset").length, 2);
      for (const snapshot of snapshots.filter(x => x.reason === "reset")) {
        assert.equal(snapshot.parameterLength, snapshot.actualFrames);
        const first = Math.max(snapshot.nativeBase, session.startFrame);
        const last = Math.min(snapshot.nativeBase + snapshot.actualFrames, session.horizonFrame);
        assert(phaseEvidence(new Float32Array(snapshot.values).subarray(first - snapshot.nativeBase, last - snapshot.nativeBase),
          last - first, first, session.startFrame, result.sampleRate, session.horizonFrame, session.period),
        "Independent reset snapshot recomputation failed");
      }
      const closing = snapshots.find(x => x.reason === "closing-marker");
      assert(closing);
      assert.equal(markerFrame(new Float32Array(closing.markerValues), closing.actualFrames, result.secondTarget), session.closing.candidate);
      const horizon = snapshots.find(x => x.reason === "horizon");
      assert(horizon && horizon.refused);
      assert.equal(phaseEvidence(new Float32Array(horizon.values), horizon.actualFrames, horizon.nativeBase,
        session.startFrame, result.sampleRate, session.horizonFrame, session.period), null);
    } else {
      assert(session.checkedSamples < 3 * 2 ** 20 && !session.done);
      assert(result.cleanup.pendingInvalidated);
      const retired = result.messages.find(x => x.kind === "retired");
      assert.equal(retired.reason, result.operation === "stop" ? "production-stop" : result.operation);
      if (result.operation === "timeout") {
        assert(result.timeout);
        assert.equal(result.retiredAttachment.reason, "retired");
        assert(result.retiredAttachment.rejectedPortClosed);
      }
      if (result.operation === "suspend") {
        assert.equal(result.suspendedState, "suspended");
        assert.equal(result.hostState, "failed");
        assert.equal(result.suspendFailure?.code, "state");
        assert.equal(result.suspendFailure?.operation, "context");
      }
    }
  } else {
    assert.equal(result.certifiedOrigin, null);
    assert.equal(summary.anchor, null);
    if (result.operation === "constant") {
      assert(result.constantCallbacks.length >= 8);
      for (const callback of result.constantCallbacks) {
        assert(callback.phaseValues.every(value => value === 0));
        assert.equal(phaseEvidence(new Float32Array(callback.phaseValues), callback.actualFrames, 1, 1,
          result.sampleRate, 1 + 3 * 2 ** 20, 2 ** 20), null);
      }
    } else if (result.operation === "late") {
      assert.equal(result.firstAdmission.eligible, false);
      assert.equal(result.lateRefusal.reason, "origin");
      assert(result.events.filter(x => x.targetFrame === result.negativeTarget).every(x => x.time < x.beforeContextTime));
    } else if (result.operation === "identities") {
      assert.deepEqual(result.rejectedAttachments.map(x => x.reason), ["generation", "identity", "duplicate"]);
      assert(result.rejectedAttachments.every(x => x.rejectedPortClosed));
      assert.deepEqual(result.messages.filter(x => x.kind === "refused").map(x => x.reason),
        ["generation", "identity", "target", "operation", "origin", "closing-prerequisite"]);
      assert(result.messages.some(x => x.kind === "accepted" && x.operation === "inspect" && x.id === result.retainedIdentity));
    } else if (result.operation === "callback-limit") {
      assert.equal(stats.callbacks, 32768);
      assert.equal(summary.ordinal, 32769);
      assert.equal(result.messages.find(x => x.kind === "retired").reason, "callback-limit");
      assert(result.cleanup.pendingInvalidated);
    }
  }
  evidence.checks.push({ id: result.name, passed: true, callbacks: stats.callbacks,
    checkedSamples: session?.checkedSamples ?? 0, maxError: session?.maxError ?? null,
    resets: session?.resets.length ?? 0, horizonRefused: session?.horizonRefused ?? false,
    closingCorroborated: Boolean(session?.closing && session.closing.candidate === session.closing.expectedBase),
    rawFrameDiscrepancies: stats.rawFrameDiscrepancies, rawSnapshots: summary.rawSnapshots });
}

function verifyJoint(result, summary, stats, callbacks) {
  const periods = [1048576, 1048575, 1048573], session = summary.session;
  assert.equal(stats.laneShapes.length, 3);
  for (const shape of stats.laneShapes) assert.equal(shape.full + shape.single + shape.other, stats.callbacks);
  const raw = result.messages.filter(x => x.kind === "callback" || x.kind === "phase-snapshot");
  assert(raw.length <= 32 && raw.length >= summary.rawSnapshots);
  for (const snapshot of raw) {
    assert.equal(snapshot.laneValues.length, 3);
    assert.equal(snapshot.laneLengths.length, 3);
    assert(snapshot.laneValues.every((values, lane) => Array.isArray(values) && values.length === snapshot.laneLengths[lane]));
  }
  if (result.operation !== "callback-limit") {
    assert(result.firstAdmission.eligible && result.firstAdmission.eventCount === 1);
    assert.equal(result.first.edge, result.offset);
    assert.equal(markerFrame(new Float32Array(result.first.values), result.first.actualFrames, result.firstTarget), result.first.spatialCandidate);
    assert.equal(summary.anchor.base, result.first.spatialCandidate);
    assert.equal(summary.anchor.ordinal, result.first.ordinal);
  }
  if (["stream", "constant", "timeout", "stop", "suspend"].includes(result.operation)) {
    assert(session && session.joint);
    assert.deepEqual(session.periods, periods);
    assert.deepEqual(session.horizonFrames, periods.map(period => session.startFrame + 3 * period));
    assert.equal(session.jointHorizonFrame, session.horizonFrames[2]);
    assert(result.jointSchedule.futureEligible.every(value => value));
    assert.equal(session.lanes.length, 3);
    if (result.operation === "constant") {
      assert.equal(result.certifiedJointOffset, null);
      assert.equal(summary.reason, "phase-failure");
      assert(["joint-shape", "joint-evidence"].includes(session.failure.reason));
      assert.equal(session.failureSample.lane, 2);
      assert.equal(session.checkedSamples, 0);
      assert(session.lanes[0].checkedSamples > 0 && session.lanes[1].checkedSamples > 0);
      assert.equal(session.lanes[2].checkedSamples, 0);
      assert.equal(result.events.filter(x => x.parameter === "__native_joint_phase_2").length, 0);
      const failure = raw.find(x => x.reason === "first-failure");
      assert(failure && failure.laneValues[2].every(value => value === 0));
      assert(result.cleanup.pendingInvalidated);
    } else {
      assert.equal(session.failure, null, JSON.stringify(session.failureSample));
      assert(session.checkedSamples > 0 && session.checkedCallbacks > 0);
      assert.equal(session.crtChecks, 2 * session.checkedCallbacks);
      assert.equal(session.firstCRT, 0);
      assert.equal(session.lastCRT, session.checkedSamples - 1);
      for (const lane of session.lanes) assert(lane.checkedSamples > 0 && lane.maxError < 0.25);
      if (result.operation === "stream") {
        assert.equal(summary.reason, "phase-complete");
        assert(session.done && session.jointHorizonRefused);
        assert.equal(session.checkedSamples, 3 * periods[2]);
        assert.deepEqual(session.lanes.map(lane => lane.checkedSamples), periods.map(period => 3 * period));
        assert.deepEqual(session.lanes.map(lane => lane.tailSamples), [9, 6, 0]);
        assert(session.lanes.every(lane => lane.horizonRefused && lane.holdFrames > 0 && lane.resets.length === 2));
        const resets = session.lanes.flatMap(lane => lane.resets);
        assert(resets.every(reset => reset.interior));
        const snapshots = raw.filter(x => x.kind === "phase-snapshot");
        const resetSnapshots = snapshots.filter(x => x.reason === "reset");
        assert.equal(resetSnapshots.length, new Set(resets.map(x => x.ordinal)).size);
        assert.equal(resetSnapshots.flatMap(x => x.resets).length, 6);
        const horizonSnapshots = snapshots.filter(x => x.reason === "horizon");
        assert.equal(horizonSnapshots.flatMap(x => x.horizons).length, 3);
        assert.equal(horizonSnapshots.length, new Set(horizonSnapshots.flatMap(x => x.horizons.map(fact => fact.ordinal))).size);
        const onset = snapshots.find(x => x.reason === "phase-onset");
        assert(onset && onset.activeIndex > 0 && onset.activeIndex < onset.actualFrames);
        assert.equal(onset.firstActive, session.startFrame);
        assert(onset.laneValues.every(values => values.slice(0, onset.activeIndex).every(value => value === 0)));
        for (const snapshot of snapshots) {
          const frame = snapshot.nativeBase, end = frame + snapshot.actualFrames;
          for (let lane = 0; lane < 3; lane++) {
            const first = Math.max(frame, session.startFrame), last = Math.min(end, session.horizonFrames[lane]);
            if (first < last) {
              assert.equal(snapshot.laneLengths[lane], snapshot.actualFrames);
              assert(phaseEvidence(new Float32Array(snapshot.laneValues[lane]).subarray(first - frame, last - frame),
                last - first, first, session.startFrame, result.sampleRate, session.horizonFrames[lane], periods[lane]),
              "Independent native lane snapshot recomputation failed");
            }
          }
          if (snapshot.joint) {
            const { firstIndex, lastIndex, firstOffset, lastOffset } = snapshot.joint;
            assert.equal(nativeJointOffset(...snapshot.laneValues.map(values => values[firstIndex])), firstOffset);
            assert.equal(nativeJointOffset(...snapshot.laneValues.map(values => values[lastIndex])), lastOffset);
            assert.equal(firstOffset, frame + firstIndex - session.startFrame);
            assert.equal(lastOffset, frame + lastIndex - session.startFrame);
          }
        }
        assert(result.closingAdmission.futureEligible && result.closingAdmission.eventCount === 2);
        assert(session.closingReset !== null && session.closing.futureEligible && session.closing.resetObserved);
        assert.equal(session.closing.candidate, session.closing.expectedBase);
        assert.equal(session.closing.intervalCallbacks, session.closing.ordinal - summary.anchor.ordinal);
        assert.equal(session.closing.intervalFrames, session.closing.expectedBase - summary.anchor.base);
        const closing = snapshots.find(x => x.reason === "closing-marker");
        assert(closing);
        assert.equal(markerFrame(new Float32Array(closing.markerValues), closing.actualFrames, result.secondTarget), session.closing.candidate);
      } else {
        assert(!session.done && session.checkedSamples < 3 * periods[2]);
        assert(result.cleanup.pendingInvalidated);
        assert.equal(result.messages.find(x => x.kind === "retired").reason,
          result.operation === "stop" ? "production-stop" : result.operation);
        if (result.operation === "timeout") {
          assert(result.timeout);
          assert.equal(result.retiredAttachment.reason, "retired");
          assert(result.retiredAttachment.rejectedPortClosed);
        }
        if (result.operation === "suspend") {
          assert.equal(result.suspendedState, "suspended");
          assert.equal(result.hostState, "failed");
          assert.equal(result.suspendFailure?.code, "state");
          assert.equal(result.suspendFailure?.operation, "context");
        }
      }
    }
  } else if (result.operation === "late") {
    assert.equal(session, null);
    assert.equal(result.certifiedJointOffset, null);
    assert.equal(result.lateRefusal.reason, "joint-late");
    assert.deepEqual(result.jointSchedule.futureEligible, [true, true, false]);
    assert(result.lateCallbacks.length >= 2);
    assert(result.lateCallbacks.every(x => x.ordinal < result.messages.find(y => y.kind === "retired").ordinal));
    assert(result.lateCallbacks.some(x => x.laneValues[0].some(value => value > 1)
      && x.laneValues[1].some(value => value > 1) && x.laneValues[2].some(value => value >= 1)));
  } else if (result.operation === "identities") {
    assert.equal(session, null);
    assert.deepEqual(result.rejectedAttachments.map(x => x.reason), ["generation", "identity", "duplicate"]);
    assert(result.rejectedAttachments.every(x => x.rejectedPortClosed));
    assert.deepEqual(result.messages.filter(x => x.kind === "refused").map(x => x.reason),
      ["origin", "generation", "identity", "target", "operation", "joint-range", "joint-range", "joint-late", "closing-prerequisite"]);
    assert(result.messages.some(x => x.kind === "accepted" && x.operation === "inspect" && x.id === result.retainedIdentity));
  } else if (result.operation === "callback-limit") {
    assert.equal(summary.anchor, null);
    assert.equal(stats.callbacks, 32768);
    assert.equal(summary.ordinal, 32769);
    assert.equal(result.messages.find(x => x.kind === "retired").reason, "callback-limit");
    assert(result.cleanup.pendingInvalidated);
  }
  evidence.checks.push({ id: result.name, passed: true, callbacks: stats.callbacks,
    checkedSamples: session?.checkedSamples ?? 0, ownSamples: session?.lanes.map(lane => lane.checkedSamples) ?? [],
    maxErrors: session?.lanes.map(lane => lane.maxError) ?? [], tailSamples: session?.lanes.map(lane => lane.tailSamples) ?? [],
    crtChecks: session?.crtChecks ?? 0, firstCRT: session?.firstCRT ?? null, lastCRT: session?.lastCRT ?? null,
    rawFrameDiscrepancies: stats.rawFrameDiscrepancies, rawSnapshots: summary.rawSnapshots, retainedRawSnapshots: raw.length });
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
  assert(!(phaseProbe && (nativeProbe || requireCold)), "WORKLET_NATIVE_PHASE_PROBE conflicts with the marker/required-cold modes");
  assert(!(jointProbe && (nativeProbe || phaseProbe || requireCold)), "WORKLET_NATIVE_JOINT_PHASE_PROBE conflicts with the marker/single-phase/required-cold modes");
  const spki = await serve();
  const require = createRequire(import.meta.url);
  const puppeteer = require(process.env.PUPPETEER_MODULE ?? "puppeteer-core");
  browser = await puppeteer.launch({ executablePath: process.env.CHROMIUM ?? "/usr/bin/chromium", headless: true,
    ...(phaseFixture ? { protocolTimeout: 150000 } : nativeProbe ? { protocolTimeout: 30000 } : {}),
    args: ["--no-sandbox", "--autoplay-policy=no-user-gesture-required", `--ignore-certificate-errors-spki-list=${spki}`] });
  if (nativeFixture) {
    evidence.browser = { version: await browser.version(), ownedPid: browser.process()?.pid ?? null };
    const cases = jointProbe ? jointCases : phaseProbe ? phaseCases : nativeCases;
    for (let index = 0; index < cases.length; index++) {
      const result = await runNativeCase(cases[index], index + (jointProbe ? 211 : phaseProbe ? 111 : 11));
      evidence.scenarios.push(result);
      try { if (phaseFixture) verifyPhase(result); else verifyNative(result); }
      catch (error) { evidence.checks.push({ id: result.name, passed: false, detail: String(error),
        ...(phaseFixture ? { callbacks: result.summary?.stats?.callbacks ?? 0,
          checkedSamples: result.summary?.session?.checkedSamples ?? 0,
          maxError: result.summary?.session?.maxError ?? null,
          failure: result.summary?.session?.failure ?? null,
          failureSample: result.summary?.session?.failureSample ?? null,
          ...(jointProbe ? { ownSamples: result.summary?.session?.lanes?.map(lane => lane.checkedSamples) ?? [],
            maxErrors: result.summary?.session?.lanes?.map(lane => lane.maxError) ?? [],
            tailSamples: result.summary?.session?.lanes?.map(lane => lane.tailSamples) ?? [],
            crtChecks: result.summary?.session?.crtChecks ?? 0,
            firstCRT: result.summary?.session?.firstCRT ?? null,
            lastCRT: result.summary?.session?.lastCRT ?? null } : {}) } : {}) }); process.exitCode = 1; }
    }
    if (phaseFixture) {
      const fresh = evidence.scenarios.filter(x => x.operation === "stream");
      const passed = fresh.length === 2 && new Set(fresh.map(x => x.generation)).size === 2
        && fresh.every(x => x.firstAdmission?.eligible && x.prerequisites.previousOrigin === false && x.cleanup.contextsClosed);
      evidence.checks.push({ id: jointProbe ? "joint-fresh-context" : "phase-fresh-context", passed });
    } else {
    const fresh = evidence.scenarios.filter(x => x.name.startsWith("native-"));
    const freshPassed = fresh.length === 8 && new Set(fresh.map(x => x.generation)).size === 8
      && fresh.every(x => x.firstAdmission?.eligible && x.prerequisites.previousOrigin === false && x.cleanup.contextsClosed);
    evidence.checks.push({ id: "fresh-context", passed: freshPassed });
    }
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
} catch (error) { evidence.fatal = { message: String(error), stack: error.stack,
  ...(error.cleanup ? { cleanup: error.cleanup } : {}) }; process.exitCode = 1; }
finally {
  try { if (browser) { await browser.close(); evidence.cleanup.browserClosed = true;
    if (nativeFixture) evidence.cleanup.ownedBrowserExitCode = browser.process()?.exitCode ?? null; } }
  catch (error) { evidence.cleanup.browserError = String(error); process.exitCode = 1; }
  try { if (server) { server.closeAllConnections(); await new Promise(yes => server.close(yes)); evidence.cleanup.serverClosed = true; } }
  catch (error) { evidence.cleanup.serverError = String(error); process.exitCode = 1; }
  await mkdir(out, { recursive: true });
  await writeFile(resolve(out, "evidence.json"), JSON.stringify(evidence, (_, value) => typeof value === "bigint" ? String(value) : value, 2));
  console.log(JSON.stringify({ checks: evidence.checks, fatal: evidence.fatal, cleanup: evidence.cleanup, output: out }, null, 2));
}
