import type { LedTrack } from "@/lib/ailight";

export type Rgb = [number, number, number];

const HEX_COLOR_RE = /^#?([0-9a-f]{6})$/i;

function hexToRgb(hex: string): Rgb {
  const match = HEX_COLOR_RE.exec(hex);
  if (!match) {
    return [0, 0, 0];
  }
  const value = Number.parseInt(match[1], 16);
  return [
    Math.floor(value / 65_536),
    Math.floor((value % 65_536) / 256),
    value % 256,
  ];
}

function scaleRgb(rgb: Rgb, factor: number): Rgb {
  return rgb.map((channel) => Math.round(channel * factor)) as Rgb;
}

function curveValue(
  curve: LedTrack["curve"],
  position: number,
  dutyPercent: number
): number {
  switch (curve) {
    case "CONSTANT":
      return 1;
    case "SQUARE":
      return position < dutyPercent / 100 ? 1 : 0;
    case "TRIANGLE":
      return position < 0.5 ? 2 * position : 2 - 2 * position;
    case "SAW_UP":
      return position;
    case "SAW_DOWN":
      return 1 - position;
    default:
      return 1;
  }
}

function endLevelRgb(track: LedTrack, brightness: number): Rgb {
  switch (track.end_level ?? "OFF") {
    case "LOW":
      return scaleRgb(hexToRgb(track.low ?? "#000000"), brightness);
    case "HIGH":
      return scaleRgb(hexToRgb(track.high), brightness);
    case "OFF":
      return [0, 0, 0];
    default:
      return [0, 0, 0];
  }
}

/** Simulates one LED track using BLE protocol V0.4 sections 7.1-7.4. */
export function sampleLedTrack(
  track: LedTrack | null,
  elapsedMs: number
): Rgb | null {
  if (!track) {
    return null;
  }

  const brightness = track.brightness / 100;
  const high = hexToRgb(track.high);
  if (track.curve === "CONSTANT" || !track.period_ms) {
    return scaleRgb(high, brightness);
  }

  const period = track.period_ms;
  const phaseMs = (period * (track.phase_deg ?? 0)) / 360;
  const trackTime = Math.max(0, elapsedMs) + phaseMs;
  if ((track.repeat ?? 0) > 0 && trackTime >= period * (track.repeat ?? 0)) {
    return endLevelRgb(track, brightness);
  }

  const position = (trackTime % period) / period;
  const value = curveValue(track.curve, position, track.duty_percent ?? 50);
  const low = hexToRgb(track.low ?? "#000000");
  return [0, 1, 2].map((index) =>
    Math.round(low[index] + (high[index] - low[index]) * value * brightness)
  ) as Rgb;
}

export function blendRgb(from: Rgb, to: Rgb, progress: number): Rgb {
  const clamped = Math.max(0, Math.min(1, progress));
  return [0, 1, 2].map((index) =>
    Math.round(from[index] + (to[index] - from[index]) * clamped)
  ) as Rgb;
}
