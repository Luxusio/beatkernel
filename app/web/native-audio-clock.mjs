import { markerTime, nativePhase, phaseIdeal } from "./worklet-native-frame-probe.mjs";

export const CLOCK_VERSION = 1;
export const CLOCK_ID = 1;
export const CLOCK_PERIODS = Object.freeze([1048576, 1048575, 1048573]);
export const CLOCK_NAMES = Object.freeze([
  "__beatkernel_clock_marker",
  "__beatkernel_clock_0",
  "__beatkernel_clock_1",
  "__beatkernel_clock_2",
]);

const frameInteger = value => Number.isSafeInteger(value) && value >= 0;
const sampleRateInteger = value => Number.isSafeInteger(value) && value > 0 && value <= 0xffffffff;

/** Build the initial finite native program on the control path only. */
export function clockGeometry(origin, maxFrames, sampleRate) {
  if (!frameInteger(origin) || origin === 0
    || !Number.isSafeInteger(maxFrames) || maxFrames < 1 || maxFrames > 1048576
    || !sampleRateInteger(sampleRate) || markerTime(origin, sampleRate) === null) return null;
  const minimumPeriod = CLOCK_PERIODS[2];
  const lowWater = Math.max(minimumPeriod, 4 * maxFrames);
  const periods = Math.ceil((lowWater + 2 * maxFrames) / minimumPeriod) + 1;
  const h0 = origin + periods * CLOCK_PERIODS[0];
  const h1 = origin + periods * CLOCK_PERIODS[1];
  const h2 = origin + periods * CLOCK_PERIODS[2];
  if (!frameInteger(h0) || !frameInteger(h1) || !frameInteger(h2)) return null;
  for (let lane = 0; lane < CLOCK_PERIODS.length; lane += 1) {
    for (let cycle = 1; cycle <= periods; cycle += 1) {
      if (markerTime(origin + cycle * CLOCK_PERIODS[lane], sampleRate) === null) return null;
    }
  }
  return { origin, h0, h1, h2, lowWater, periods };
}

/** Validate the complete original native lane before publishing its candidate. */
export function validClockLane(values, frames, frame, origin, horizon, period, rate, firstIndex) {
  if (!(values instanceof Float32Array) || !Number.isSafeInteger(frames)
    || frames < 1 || frames > 1048576 || values.length !== frames
    || !frameInteger(frame) || !frameInteger(origin) || origin === 0
    || !frameInteger(horizon) || horizon <= origin
    || !Number.isSafeInteger(period) || period < 2 || period > 1048576
    || (horizon - origin) % period !== 0 || !sampleRateInteger(rate)
    || !Number.isSafeInteger(firstIndex) || firstIndex < 0 || firstIndex >= frames
    || markerTime(horizon, rate) === null) return false;
  const end = frame + frames;
  const first = frame + firstIndex;
  if (!frameInteger(end) || end > horizon || !frameInteger(first)
    || (firstIndex === 0 ? first < origin : first !== origin)) return false;
  for (let index = 0; index < firstIndex; index += 1) {
    if (values[index] !== 0) return false;
  }
  for (let index = firstIndex; index < frames; index += 1) {
    const sampleFrame = frame + index;
    const phase = nativePhase(values[index], period);
    const ideal = phaseIdeal(sampleFrame, origin, rate, period);
    if (phase === null || phase !== (sampleFrame - origin) % period
      || ideal === null || !(Math.abs(values[index] - ideal) < 0.25)) return false;
  }
  return true;
}
