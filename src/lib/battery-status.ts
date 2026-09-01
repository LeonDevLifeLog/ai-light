import type { DeviceState } from "./ailight";

const CAP_BATTERY_PRESENT = 16;
const POWER_FLAG_BATTERY_PRESENT = 1;

const hasFlag = (value: number, flag: number) =>
  Math.floor(value / flag) % 2 === 1;

export type BatteryStatus =
  | { kind: "unknown" }
  | { kind: "absent" }
  | { kind: "present-unmeasured"; voltageMv: number | null }
  | {
      kind: "present-measured";
      percent: number;
      voltageMv: number | null;
    };

export function batteryStatus(device: DeviceState): BatteryStatus {
  if (device.powerFlags == null) {
    if (
      device.capabilityBits != null &&
      !hasFlag(device.capabilityBits, CAP_BATTERY_PRESENT)
    ) {
      return { kind: "absent" };
    }
    return { kind: "unknown" };
  }

  if (!hasFlag(device.powerFlags, POWER_FLAG_BATTERY_PRESENT)) {
    return { kind: "absent" };
  }

  if (device.batteryPercent == null) {
    return {
      kind: "present-unmeasured",
      voltageMv: device.batteryMv,
    };
  }

  return {
    kind: "present-measured",
    percent: device.batteryPercent,
    voltageMv: device.batteryMv,
  };
}

export function batteryStatusLabel(status: BatteryStatus): string {
  switch (status.kind) {
    case "absent":
      return "无电池";
    case "present-measured":
      return `${status.percent}%`;
    case "present-unmeasured":
      return status.voltageMv == null
        ? "电量未标定"
        : `电量未标定 · ${status.voltageMv} mV`;
    case "unknown":
      return "电池状态未知";
    default:
      return "电池状态未知";
  }
}
