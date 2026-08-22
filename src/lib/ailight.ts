import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type BusinessStateName =
  | "IDLE"
  | "WORKING"
  | "WAITING"
  | "SUCCESS"
  | "ERROR"
  | (string & {});

export interface ServiceState {
  port: number;
  tokenEnabled: boolean;
  version: string;
}

export interface DeviceState {
  address: string | null;
  batteryPercent: number | null;
  chargeState: number | null;
  connected: boolean;
  fwVersion: string | null;
  hardwareVariant: number | null;
  name: string | null;
  powerFlags: number | null;
  powerSource: number | null;
  /** 断连后是否处于自动重连中（device-connection-changed 事件字段） */
  reconnecting: boolean;
}

export interface BusinessState {
  session: string | null;
  sinceTs: number;
  source: string | null;
  state: BusinessStateName;
  theme: string;
}

export interface AppSnapshot {
  activeTheme: string;
  business: BusinessState;
  device: DeviceState;
  service: ServiceState;
  themes: string[];
}

export interface ThemeMeta {
  builtin: boolean;
  name: string;
}

export interface ScannedDevice {
  address: string;
  name: string;
  recognized: boolean;
  rssi: number | null;
}

export interface RememberedDevice {
  address: string;
  name: string;
}

export interface AppConfig {
  activeTheme: string;
  arbitrationMode: "priority" | "last_active";
  autostart: boolean;
  badgeOrientation: "horizontal" | "vertical";
  portPreference: number;
  rememberedDevice: RememberedDevice | null;
  themeMode: "light" | "dark" | "system";
  token: string;
  version: number;
}

export interface AppError {
  code: string;
  message: string;
}

export interface LedTrack {
  brightness: number;
  curve: "CONSTANT" | "SQUARE" | "TRIANGLE" | "SAW_UP" | "SAW_DOWN";
  duty_percent?: number;
  end_level?: "OFF" | "LOW" | "HIGH";
  high: string;
  low?: string;
  period_ms?: number;
  phase_deg?: number;
  repeat?: number;
}

export interface BuzzerSegment {
  duration_ms: number;
  frequency_hz: number;
  volume: number;
}

export interface ThemeFile {
  scenes: Record<
    string,
    {
      leds: [LedTrack | null, LedTrack | null, LedTrack | null];
      buzzer?: {
        start_delay_ms?: number;
        repeat?: number;
        segments: BuzzerSegment[];
      } | null;
    }
  >;
  states: Record<
    string,
    { scene: string; transition_ms?: number; hold_ms?: number }
  >;
  theme: { name: string; version: number };
}

const mockSnapshot: AppSnapshot = {
  service: { version: "0.1.0", port: 25_679, tokenEnabled: false },
  device: {
    connected: false,
    address: null,
    name: null,
    fwVersion: null,
    hardwareVariant: null,
    batteryPercent: null,
    powerFlags: null,
    powerSource: null,
    chargeState: null,
    reconnecting: false,
  },
  business: {
    state: "IDLE",
    source: null,
    session: null,
    sinceTs: Date.now(),
    theme: "default",
  },
  themes: ["default", "minimal", "neon", "nature", "aurora", "focus"],
  activeTheme: "default",
};

const mockConfig: AppConfig = {
  version: 1,
  arbitrationMode: "priority",
  activeTheme: "default",
  portPreference: 25_679,
  rememberedDevice: null,
  token: "",
  autostart: false,
  badgeOrientation: "horizontal",
  themeMode: "dark",
};

const mockTheme: ThemeFile = {
  theme: { name: "default", version: 1 },
  scenes: {
    off: { leds: [null, null, null], buzzer: null },
    working: {
      leds: [
        null,
        null,
        {
          curve: "TRIANGLE",
          low: "#052E16",
          high: "#22C55E",
          brightness: 70,
          period_ms: 2000,
          phase_deg: 0,
        },
      ],
      buzzer: null,
    },
    waiting: { leds: [null, defaultMockTrack("#F59E0B"), null], buzzer: null },
    success: { leds: [null, null, defaultMockTrack("#22C55E")], buzzer: null },
    error: { leds: [defaultMockTrack("#EF4444"), null, null], buzzer: null },
  },
  states: {
    IDLE: { scene: "off" },
    WORKING: { scene: "working" },
    WAITING: { scene: "waiting" },
    SUCCESS: { scene: "success", hold_ms: 5000 },
    ERROR: { scene: "error" },
  },
};

function defaultMockTrack(high: string): LedTrack {
  return { curve: "CONSTANT", high, brightness: 75 };
}

export const isTauri = () => "__TAURI_INTERNALS__" in window;

const normalize = <T>(value: unknown): T => {
  if (Array.isArray(value)) {
    return value.map((item) => normalize(item)) as T;
  }
  if (value && typeof value === "object") {
    const result: Record<string, unknown> = {};
    for (const [key, item] of Object.entries(value)) {
      result[
        key.replace(/_([a-z])/g, (_, letter: string) => letter.toUpperCase())
      ] = normalize(item);
    }
    return result as T;
  }
  return value as T;
};

export const asAppError = (error: unknown): AppError => {
  const normalized = normalize<Partial<AppError>>(error);
  if (normalized && typeof normalized === "object" && normalized.message) {
    return { code: normalized.code ?? "INTERNAL", message: normalized.message };
  }
  return {
    code: "INTERNAL",
    message: error instanceof Error ? error.message : String(error),
  };
};

async function call<T>(
  command: string,
  args?: Record<string, unknown>
): Promise<T> {
  return normalize<T>(await invoke(command, args));
}

export const api = {
  getAppState: async () =>
    isTauri() ? call<AppSnapshot>("get_app_state") : mockSnapshot,
  getThemes: async () =>
    isTauri()
      ? call<ThemeMeta[]>("get_themes")
      : mockSnapshot.themes.map((name) => ({ name, builtin: true })),
  getTheme: async (name: string) =>
    isTauri()
      ? call<string>("get_theme", { name })
      : JSON.stringify({ ...mockTheme, theme: { ...mockTheme.theme, name } }),
  setActiveTheme: async (name: string) =>
    isTauri() ? call<void>("set_active_theme", { name }) : undefined,
  importTheme: async (content: string) =>
    isTauri()
      ? call<string>("import_theme", { content })
      : (JSON.parse(content) as ThemeFile).theme.name,
  scanDevices: async () =>
    isTauri() ? call<ScannedDevice[]>("scan_devices") : [],
  connectDevice: (address: string) => call<void>("connect_device", { address }),
  disconnectDevice: async () =>
    isTauri() ? call<{ ok: boolean }>("disconnect_device") : { ok: true },
  forgetDevice: async () =>
    isTauri() ? call<{ ok: boolean }>("forget_device") : { ok: true },
  triggerState: async (state: string) =>
    isTauri() ? call<boolean>("trigger_state", { state, meta: null }) : true,
  previewScene: (state: string, theme?: string) =>
    call<void>("preview_scene", { state, theme, content: null }),
  previewThemeDraft: async (state: string, content: string) =>
    isTauri()
      ? call<void>("preview_scene", { state, theme: null, content })
      : undefined,
  resetOutputs: async () =>
    isTauri() ? call<void>("reset_outputs") : undefined,
  getConfig: async () =>
    isTauri() ? call<AppConfig>("get_config") : mockConfig,
  updateConfig: async (patch: Partial<AppConfig>) =>
    isTauri()
      ? call<AppConfig>("update_config", { patch })
      : { ...mockConfig, ...patch },
};

export function subscribe<T>(
  event: string,
  callback: (payload: T) => void
): Promise<UnlistenFn> {
  if (!isTauri()) {
    return Promise.resolve(() => undefined);
  }
  return listen<T>(event, ({ payload }) => callback(normalize<T>(payload)));
}
