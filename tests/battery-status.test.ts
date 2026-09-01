import assert from "node:assert/strict";
import test from "node:test";
import type { DeviceState } from "../src/lib/ailight.ts";
import {
  batteryStatus,
  batteryStatusLabel,
} from "../src/lib/battery-status.ts";

const device = (overrides: Partial<DeviceState>): DeviceState => ({
  address: "AA:BB:CC:DD:EE:FF",
  batteryMv: null,
  batteryPercent: null,
  capabilityBits: null,
  chargeState: null,
  connected: true,
  fwVersion: "0.0.9",
  hardwareVariant: 1,
  name: "StatusLight",
  powerFlags: null,
  powerSource: null,
  reconnecting: false,
  ...overrides,
});

test("keeps an unread battery state unknown", () => {
  const status = batteryStatus(device({ capabilityBits: 0x10 }));

  assert.deepEqual(status, { kind: "unknown" });
  assert.equal(batteryStatusLabel(status), "电池状态未知");
});

test("recognizes a device without battery capability", () => {
  const status = batteryStatus(device({ capabilityBits: 0x0 }));

  assert.deepEqual(status, { kind: "absent" });
  assert.equal(batteryStatusLabel(status), "无电池");
});

test("shows voltage when a present battery has no calibrated percentage", () => {
  const status = batteryStatus(
    device({ batteryMv: 3993, capabilityBits: 0x3f, powerFlags: 0x1 })
  );

  assert.deepEqual(status, {
    kind: "present-unmeasured",
    voltageMv: 3993,
  });
  assert.equal(batteryStatusLabel(status), "电量未标定 · 3993 mV");
});

test("shows a known percentage for a present battery", () => {
  const status = batteryStatus(
    device({
      batteryMv: 3993,
      batteryPercent: 75,
      capabilityBits: 0x3f,
      powerFlags: 0x1,
    })
  );

  assert.deepEqual(status, {
    kind: "present-measured",
    percent: 75,
    voltageMv: 3993,
  });
  assert.equal(batteryStatusLabel(status), "75%");
});
