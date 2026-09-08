import assert from "node:assert/strict";
import test from "node:test";
import type { LedTrack } from "../src/lib/ailight.ts";
import { blendRgb, sampleLedTrack } from "../src/lib/light-simulator.ts";

const track = (overrides: Partial<LedTrack> = {}): LedTrack => ({
  brightness: 100,
  curve: "TRIANGLE",
  high: "#FFFFFF",
  low: "#000000",
  period_ms: 1000,
  ...overrides,
});

test("samples every supported curve", () => {
  assert.deepEqual(
    sampleLedTrack(track({ curve: "CONSTANT" }), 250),
    [255, 255, 255]
  );
  assert.deepEqual(
    sampleLedTrack(track({ curve: "SQUARE", duty_percent: 25 }), 249),
    [255, 255, 255]
  );
  assert.deepEqual(
    sampleLedTrack(track({ curve: "SQUARE", duty_percent: 25 }), 250),
    [0, 0, 0]
  );
  assert.deepEqual(sampleLedTrack(track(), 250), [128, 128, 128]);
  assert.deepEqual(
    sampleLedTrack(track({ curve: "SAW_UP" }), 250),
    [64, 64, 64]
  );
  assert.deepEqual(
    sampleLedTrack(track({ curve: "SAW_DOWN" }), 250),
    [191, 191, 191]
  );
});

test("applies phase before sampling", () => {
  assert.deepEqual(
    sampleLedTrack(track({ phase_deg: 90 }), 0),
    [128, 128, 128]
  );
});

test("holds the configured end level after finite repeats", () => {
  assert.deepEqual(
    sampleLedTrack(track({ end_level: "OFF", repeat: 1 }), 1000),
    [0, 0, 0]
  );
  assert.deepEqual(
    sampleLedTrack(
      track({ brightness: 50, end_level: "LOW", low: "#804020", repeat: 1 }),
      1000
    ),
    [64, 32, 16]
  );
  assert.deepEqual(
    sampleLedTrack(
      track({ brightness: 50, end_level: "HIGH", repeat: 1 }),
      1000
    ),
    [128, 128, 128]
  );
});

test("blends scene transition colors", () => {
  assert.deepEqual(blendRgb([0, 0, 0], [200, 100, 50], 0.5), [100, 50, 25]);
  assert.deepEqual(blendRgb([0, 0, 0], [200, 100, 50], 2), [200, 100, 50]);
});
