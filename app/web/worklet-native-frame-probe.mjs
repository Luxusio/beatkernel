// Diagnostic helpers only; these do not supply a production audio clock.
const frameInteger = value => Number.isSafeInteger(value) && value >= 0;
const phasePeriod = value => Number.isSafeInteger(value) && value >= 2 && value <= 2 ** 20;

/** Choose a time whose represented sample position has the unambiguous ceil T. */
export function markerTime(targetFrame, sampleRate) {
  if (!frameInteger(targetFrame) || targetFrame === 0
    || !frameInteger(sampleRate) || sampleRate === 0) return null;
  const time = (targetFrame - 0.25) / sampleRate;
  const scaled = time * sampleRate;
  if (!Number.isFinite(time) || !Number.isFinite(scaled)
    || !(scaled > targetFrame - 1 && scaled < targetFrame)
    || Math.ceil(scaled) !== targetFrame) return null;
  return time;
}

/**
 * Recover a callback base from a KNOWN ACTUAL native onset, not a scheduled time.
 * An interior edge alone cannot authenticate onset after native late clamping.
 */
export function markerFrame(values, actualFrames, targetFrame) {
  if (!(values instanceof Float32Array) || !frameInteger(actualFrames)
    || values.length !== actualFrames || !frameInteger(targetFrame)) return null;
  let edge = 0;
  while (edge < actualFrames && values[edge] === 0) edge += 1;
  if (edge === 0 || edge === actualFrames) return null;
  for (let index = edge; index < actualFrames; index += 1) {
    if (values[index] !== 1) return null;
  }
  const frame = targetFrame - edge;
  return frameInteger(frame) ? frame : null;
}

/** Count the observed callback extent without losing integer precision. */
export function advanceFrame(frame, actualFrames) {
  if (!frameInteger(frame) || !frameInteger(actualFrames)) return null;
  const next = frame + actualFrames;
  return frameInteger(next) ? next : null;
}

/** Decode a modulo candidate only; whole-period gaps remain indistinguishable. */
export function nativePhase(value, period) {
  if (!Number.isFinite(value) || !phasePeriod(period)
    || value - Math.floor(value) === 0.5) return null;
  const code = Math.round(value) - 1;
  if (code < 0 || code > period) return null;
  return code === period ? 0 : code;
}

/**
 * Recover a diagnostic modulo offset in the Number-safe domain only.
 * Full-modulus aliases remain; samples alone do not certify an epoch or age.
 */
export function nativeJointOffset(value0, value1, value2) {
  const phase0 = nativePhase(value0, 1048576);
  const phase1 = nativePhase(value1, 1048575);
  const phase2 = nativePhase(value2, 1048573);
  if (phase0 === null || phase1 === null || phase2 === null) return null;
  const r0 = BigInt(phase0);
  const t1 = (BigInt(phase1) - r0 + 1048575n) % 1048575n;
  const x01 = r0 + 1048576n * t1;
  const remainder2 = ((BigInt(phase2) - x01) % 1048573n + 1048573n) % 1048573n;
  const t2 = remainder2 * 873811n % 1048573n;
  const offset = x01 + 1048576n * 1048575n * t2;
  return offset <= BigInt(Number.MAX_SAFE_INTEGER) ? Number(offset) : null;
}

/** Double ideal for the represented quarter-early native ramp event times. */
export function phaseIdeal(frame, startFrame, sampleRate, period) {
  if (!frameInteger(frame) || !frameInteger(startFrame) || startFrame === 0
    || !frameInteger(sampleRate) || sampleRate === 0 || !phasePeriod(period)
    || frame < startFrame) return null;
  const cycleStart = frame - ((frame - startFrame) % period);
  const cycleEnd = advanceFrame(cycleStart, period);
  if (cycleEnd === null) return null;
  const startTime = markerTime(cycleStart, sampleRate);
  const endTime = markerTime(cycleEnd, sampleRate);
  if (startTime === null || endTime === null || endTime <= startTime) return null;
  const ideal = 1 + period * (frame / sampleRate - startTime) / (endTime - startTime);
  return Number.isFinite(ideal) ? ideal : null;
}

/** Check every actual sample without changing the input or admitting a clock. */
export function phaseEvidence(values, actualFrames, frame, startFrame, sampleRate, horizonFrame, period) {
  if (!(values instanceof Float32Array) || !frameInteger(actualFrames) || actualFrames === 0
    || values.length !== actualFrames || !frameInteger(frame)
    || !frameInteger(startFrame) || startFrame === 0
    || !frameInteger(sampleRate) || sampleRate === 0 || !frameInteger(horizonFrame)
    || !phasePeriod(period) || frame < startFrame || horizonFrame <= startFrame
    || (horizonFrame - startFrame) % period !== 0) return null;
  const endFrame = advanceFrame(frame, actualFrames);
  if (endFrame === null || endFrame > horizonFrame
    || markerTime(horizonFrame, sampleRate) === null) return null;
  let maxError = 0, firstPhase = null, lastPhase = null;
  for (let index = 0; index < actualFrames; index += 1) {
    const sampleFrame = frame + index;
    const phase = nativePhase(values[index], period);
    const ideal = phaseIdeal(sampleFrame, startFrame, sampleRate, period);
    if (phase === null || phase !== (sampleFrame - startFrame) % period || ideal === null) return null;
    const error = Math.abs(values[index] - ideal);
    if (!(error < 0.25)) return null;
    maxError = Math.max(maxError, error);
    if (index === 0) firstPhase = phase;
    lastPhase = phase;
  }
  return { checkedSamples: actualFrames, maxError, firstPhase, lastPhase };
}
