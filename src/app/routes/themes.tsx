import {
  Check,
  ChevronDown,
  FileJson,
  Import,
  Music2,
  Pencil,
  Plus,
  SlidersHorizontal,
  Sparkles,
  Trash2,
  Volume2,
  VolumeX,
  Wand2,
} from "lucide-react";
import {
  type ChangeEvent,
  type ReactNode,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import { useAppState } from "@/app/app-context";
import {
  ActionButton,
  Card,
  Dialog,
  EmptyState,
  InlineAlert,
  PageHeader,
  StatusTag,
  TrafficBadge,
} from "@/components/app-ui";
import {
  api,
  asAppError,
  type LedTrack,
  type ThemeFile,
  type ThemeMeta,
} from "@/lib/ailight";
import { cn, runAsync } from "@/lib/utils";

const standardStates = [
  "IDLE",
  "WORKING",
  "WAITING",
  "SUCCESS",
  "ERROR",
] as const;
const displayNames: Record<string, string> = {
  default: "默认",
  minimal: "极简",
  neon: "霓虹",
  nature: "自然",
  aurora: "极光",
  focus: "专注",
};

const descriptions: Record<string, string> = {
  default: "简洁耐看，全场景通用",
  minimal: "只保留必要信号，不打扰",
  neon: "明亮多彩，充满赛博氛围",
  nature: "柔和自然，适合长时间工作",
  aurora: "流动渐变，富有层次",
  focus: "克制专注，减少视觉干扰",
};
const THEME_NAME_PATTERN = /^[a-zA-Z0-9_-]{1,64}$/;

function cloneTheme(theme: ThemeFile): ThemeFile {
  return JSON.parse(JSON.stringify(theme)) as ThemeFile;
}

function defaultTrack(color: string): LedTrack {
  return { curve: "CONSTANT", high: color, brightness: 70 };
}

function normalizeTrack(track: LedTrack, patch: Partial<LedTrack>): LedTrack {
  const updated = { ...track, ...patch };
  if (updated.curve === "CONSTANT") {
    return {
      curve: "CONSTANT",
      high: updated.high,
      brightness: updated.brightness,
    };
  }
  const animated: LedTrack = {
    ...updated,
    low: updated.low ?? "#000000",
    period_ms: updated.period_ms ?? 1200,
    phase_deg: updated.phase_deg ?? 0,
  };
  if (animated.curve === "SQUARE") {
    animated.duty_percent ??= 50;
  } else {
    animated.duty_percent = undefined;
  }
  return animated;
}

function previewState(index: number) {
  const sequence = ["WORKING", "WAITING", "ERROR"] as const;
  return sequence[index % sequence.length];
}

type MotionPreset =
  | "steady"
  | "breathe"
  | "blink"
  | "flow"
  | "fade-in"
  | "fade-out";

const motionPresets: Array<{
  value: MotionPreset;
  label: string;
  hint: string;
  curve: LedTrack["curve"];
}> = [
  { value: "steady", label: "常亮", hint: "安静稳定", curve: "CONSTANT" },
  { value: "breathe", label: "呼吸", hint: "柔和起伏", curve: "TRIANGLE" },
  { value: "blink", label: "闪烁", hint: "清晰提醒", curve: "SQUARE" },
  { value: "flow", label: "流动", hint: "三灯依次移动", curve: "SAW_UP" },
  { value: "fade-in", label: "渐亮", hint: "从暗到亮", curve: "SAW_UP" },
  { value: "fade-out", label: "渐弱", hint: "从亮到暗", curve: "SAW_DOWN" },
];

const stateLabels: Record<
  string,
  { title: string; tagline: string; accent: string }
> = {
  IDLE: { title: "空闲", tagline: "等待任务", accent: "#94a3b8" },
  WORKING: { title: "工作中", tagline: "正在处理", accent: "#22c55e" },
  WAITING: { title: "等待中", tagline: "需要输入", accent: "#f59e0b" },
  SUCCESS: { title: "已完成", tagline: "已顺利完成", accent: "#4ade80" },
  ERROR: { title: "出错了", tagline: "遇到问题", accent: "#ef4444" },
};

const customAccent = "#a78bfa";

const HEX_COLOR_RE = /^#?([0-9a-f]{6})$/i;

function speedTierOf(periodMs: number): "slow" | "medium" | "fast" {
  if (periodMs >= 2200) {
    return "slow";
  }
  if (periodMs >= 800) {
    return "medium";
  }
  return "fast";
}

function renderLightEl(
  el: HTMLDivElement,
  track: LedTrack | null,
  now: number
) {
  if (!track) {
    el.style.backgroundColor = "transparent";
    el.style.boxShadow = "none";
    el.style.opacity = "0.1";
    return;
  }
  const rgb = trackRgbAt(track, now, 0) ?? [0, 0, 0];
  const color = `rgb(${rgb[0]},${rgb[1]},${rgb[2]})`;
  el.style.backgroundColor = color;
  el.style.boxShadow = `0 0 ${Math.max(8, (track.brightness ?? 0) * 0.45)}px ${color}`;
  el.style.opacity = "1";
}

function renderBuzzEl(
  el: HTMLDivElement,
  buzzer: ThemeFile["scenes"][string]["buzzer"],
  now: number
) {
  let active = false;
  if (buzzer?.segments.length) {
    const total = buzzer.segments.reduce(
      (sum, segment) => sum + segment.duration_ms,
      0
    );
    const loop =
      buzzer.repeat && buzzer.repeat > 0 ? total * buzzer.repeat : total;
    const at = now % (loop || total);
    let cursor = 0;
    for (const segment of buzzer.segments) {
      if (at >= cursor && at < cursor + segment.duration_ms) {
        active = segment.frequency_hz > 0;
        break;
      }
      cursor += segment.duration_ms;
    }
  }
  el.style.opacity = active ? "1" : "0.2";
  el.style.transform = active ? "scaleY(1)" : "scaleY(0.35)";
}

/** 按协议 §7.2 的曲线函数计算 0~1 波形值（不含 SINE，V0.4 未实现）。 */
function curveValue(
  curve: LedTrack["curve"],
  t: number,
  dutyPercent: number
): number {
  switch (curve) {
    case "CONSTANT":
      return 1;
    case "SQUARE":
      return t < dutyPercent / 100 ? 1 : 0;
    case "TRIANGLE":
      return t < 0.5 ? 2 * t : 2 - 2 * t;
    case "SAW_UP":
      return t;
    case "SAW_DOWN":
      return 1 - t;
    default:
      return 1;
  }
}

function hexToRgb(hex: string): [number, number, number] {
  const match = HEX_COLOR_RE.exec(hex);
  if (!match) {
    return [0, 0, 0];
  }
  const value = Number.parseInt(match[1], 16);
  const r = Math.floor(value / 65_536);
  const g = Math.floor((value % 65_536) / 256);
  const b = value % 256;
  return [r, g, b];
}

function rgbToHex(rgb: [number, number, number]): string {
  const [r, g, b] = rgb.map((channel) =>
    Math.max(0, Math.min(255, Math.round(channel)))
  );
  const value = r * 65_536 + g * 256 + b;
  return `#${value.toString(16).padStart(6, "0")}`;
}

function darkenHex(hex: string, factor: number): string {
  const [r, g, b] = hexToRgb(hex);
  return rgbToHex([r * factor, g * factor, b * factor]);
}

/** 计算单条灯轨在某一时刻应显示的实际 RGB（协议 §7.1 的模拟实现）。 */
function trackRgbAt(
  track: LedTrack | null,
  elapsedMs: number,
  epochMs = 0
): [number, number, number] | null {
  if (!track) {
    return null;
  }
  const high = hexToRgb(track.high);
  const low = hexToRgb(track.low ?? "#000000");
  const brightness = (track.brightness ?? 0) / 100;
  if (track.curve === "CONSTANT" || !track.period_ms) {
    return high.map((channel) => Math.round(channel * brightness)) as [
      number,
      number,
      number,
    ];
  }
  const period = track.period_ms;
  const phaseMs = (period * (track.phase_deg ?? 0)) / 360;
  const sceneTime = elapsedMs - epochMs;
  const position = ((sceneTime + phaseMs) % period) / period;
  const value = curveValue(track.curve, position, track.duty_percent ?? 50);
  return [0, 1, 2].map((index) =>
    Math.round(low[index] + (high[index] - low[index]) * value * brightness)
  ) as [number, number, number];
}

/** 在软件端即时模拟三灯 + 蜂鸣动画，让"看得见"先行于"连设备"。 */
function LivePreview({ scene }: { scene: ThemeFile["scenes"][string] | null }) {
  const lightRefs = useRef<Array<HTMLDivElement | null>>([]);
  const buzzRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    const reduceMotion = window.matchMedia?.(
      "(prefers-reduced-motion: reduce)"
    ).matches;
    const renderFrame = (now: number) => {
      scene?.leds.forEach((track, index) => {
        const el = lightRefs.current[index];
        if (el) {
          renderLightEl(el, track, now);
        }
      });
      const buzzEl = buzzRef.current;
      if (buzzEl) {
        renderBuzzEl(buzzEl, scene?.buzzer, now);
      }
    };

    const tick = (now: number) => {
      renderFrame(now);
      raf = requestAnimationFrame(tick);
    };

    let raf = 0;
    if (reduceMotion) {
      renderFrame(0);
    } else {
      raf = requestAnimationFrame(tick);
    }
    return () => cancelAnimationFrame(raf);
  }, [scene]);

  return (
    <div className="te-preview">
      <div
        aria-label="三灯与蜂鸣预览"
        className="te-preview__device"
        role="img"
      >
        {["顶灯", "中灯", "底灯"].map((label, index) => {
          const track = scene?.leds[index] ?? null;
          return (
            <div className="te-preview__light-wrap" key={label}>
              <span className="te-preview__label">{label}</span>
              <div
                className="te-preview__light"
                ref={(node) => {
                  lightRefs.current[index] = node;
                }}
                style={{
                  backgroundColor: track ? track.high : "transparent",
                  opacity: track
                    ? Math.max(0.08, (track.brightness ?? 70) / 100)
                    : 0.1,
                }}
              />
            </div>
          );
        })}
      </div>
      <div className="te-preview__buzz">
        <div className="te-preview__buzz-bars" ref={buzzRef}>
          <i />
          <i />
          <i />
        </div>
        <span>{scene?.buzzer?.segments.length ? "提示音已开启" : "无声"}</span>
      </div>
    </div>
  );
}

function MotionGlyph({
  curve,
  active,
}: {
  curve: LedTrack["curve"];
  active: boolean;
}) {
  const color = active ? "var(--accent)" : "var(--text-tertiary)";
  const paths: Record<LedTrack["curve"], string> = {
    CONSTANT: "M1 8 H47",
    SQUARE: "M1 14 H11 L11 2 H39 L39 14 H47",
    TRIANGLE: "M1 14 L24 2 L47 14",
    SAW_UP: "M1 14 L47 2",
    SAW_DOWN: "M1 2 L47 14",
  };
  return (
    <svg
      aria-hidden="true"
      className="te-motion-card__glyph"
      viewBox="0 0 48 16"
    >
      <path
        d={paths[curve]}
        fill="none"
        stroke={color}
        strokeLinecap="round"
        strokeLinejoin="round"
        strokeWidth={2}
      />
    </svg>
  );
}

const SOUND_PRESETS: Record<
  "silent" | "gentle" | "confirm" | "alert",
  ThemeFile["scenes"][string]["buzzer"]
> = {
  silent: null,
  gentle: {
    repeat: 1,
    segments: [{ frequency_hz: 1200, duration_ms: 100, volume: 30 }],
  },
  confirm: {
    repeat: 1,
    segments: [
      { frequency_hz: 880, duration_ms: 100, volume: 45 },
      { frequency_hz: 1320, duration_ms: 140, volume: 45 },
    ],
  },
  alert: {
    repeat: 3,
    segments: [
      { frequency_hz: 2000, duration_ms: 140, volume: 70 },
      { frequency_hz: 0, duration_ms: 100, volume: 0 },
    ],
  },
};

const soundOptions: Array<{
  value: keyof typeof SOUND_PRESETS;
  label: string;
  hint: string;
}> = [
  { value: "silent", label: "无声", hint: "关闭蜂鸣" },
  { value: "gentle", label: "轻提示", hint: "短促一声" },
  { value: "confirm", label: "确认音", hint: "两音上行" },
  { value: "alert", label: "警报音", hint: "急促三响" },
];

// biome-ignore lint/complexity/noExcessiveCognitiveComplexity: the editor maps a flat, end-user vocabulary onto a single SCENE.
function ThemeEditor({
  availableThemes,
  open,
  source,
  onClose,
  onSaved,
}: {
  availableThemes: ThemeMeta[];
  open: boolean;
  source: ThemeFile | null;
  onClose: () => void;
  onSaved: () => Promise<void>;
}) {
  const { notify, snapshot } = useAppState();
  const [draft, setDraft] = useState<ThemeFile | null>(null);
  const [selectedState, setSelectedState] = useState("WORKING");
  const [selectedLed, setSelectedLed] = useState(0);
  const [advanced, setAdvanced] = useState(false);
  const [saving, setSaving] = useState(false);
  const [previewing, setPreviewing] = useState(false);
  const [newState, setNewState] = useState("");
  const [mixOpen, setMixOpen] = useState(false);
  const [mixTheme, setMixTheme] = useState("default");
  const [mixState, setMixState] = useState("WORKING");
  const [confirmingClose, setConfirmingClose] = useState(false);

  useEffect(() => {
    if (source && open) {
      const next = cloneTheme(source);
      next.theme.name =
        source.theme.name === "default"
          ? "my-theme"
          : `${source.theme.name}-copy`;
      const saved = localStorage.getItem(
        `ailight-theme-draft:${source.theme.name}`
      );
      if (saved) {
        try {
          setDraft(JSON.parse(saved) as ThemeFile);
        } catch {
          setDraft(next);
        }
      } else {
        setDraft(next);
      }
      setSelectedState("WORKING");
      setAdvanced(false);
      setNewState("");
      setMixTheme("default");
      setMixState("WORKING");
      setConfirmingClose(false);
    }
  }, [source, open]);

  useEffect(() => {
    if (open && source && draft) {
      localStorage.setItem(
        `ailight-theme-draft:${source.theme.name}`,
        JSON.stringify(draft)
      );
    }
  }, [draft, open, source]);

  if (!draft) {
    return null;
  }
  const mapping = draft.states[selectedState];
  const scene = mapping ? draft.scenes[mapping.scene] : null;
  const track = scene?.leds[selectedLed] ?? null;
  const trackCurve = track?.curve ?? "CONSTANT";
  const sceneMotionCurve: LedTrack["curve"] =
    scene?.leds.find((led) => led !== null)?.curve ?? "CONSTANT";
  const sceneReferences = mapping
    ? Object.entries(draft.states)
        .filter(([, value]) => value.scene === mapping.scene)
        .map(([state]) => state)
    : [];
  const buzzerSegment = scene?.buzzer?.segments[0] ?? {
    duration_ms: 150,
    frequency_hz: 1800,
    volume: 60,
  };

  const period = track?.period_ms ?? 1200;
  const speedTier = speedTierOf(period);
  const activeMotion =
    motionPresets.find((preset) => preset.curve === sceneMotionCurve)?.value ??
    "steady";
  const currentBuzzer = JSON.stringify(scene?.buzzer ?? null);
  const activeSound = soundOptions.find(
    (option) => JSON.stringify(SOUND_PRESETS[option.value]) === currentBuzzer
  )?.value;
  const relationPhases = {
    sync: [0, 0, 0],
    "top-down": [0, 120, 240],
    "bottom-up": [240, 120, 0],
    staggered: [0, 180, 90],
  } as const;
  const currentPhases = scene
    ? scene.leds.map((led) =>
        led && led.curve !== "CONSTANT" ? (led.phase_deg ?? 0) : 0
      )
    : [0, 0, 0];
  const activeRelation = (
    Object.keys(relationPhases) as Array<keyof typeof relationPhases>
  ).find(
    (relation) =>
      JSON.stringify(relationPhases[relation]) === JSON.stringify(currentPhases)
  );

  const updateTrack = (patch: Partial<LedTrack>) => {
    if (!scene) {
      return;
    }
    const next = cloneTheme(draft);
    const nextScene = next.scenes[mapping.scene];
    const current = nextScene.leds[selectedLed] ?? defaultTrack("#22C55E");
    nextScene.leds[selectedLed] = normalizeTrack(current, patch);
    setDraft(next);
  };

  const updateLedColor = (index: number, patch: Partial<LedTrack>) => {
    if (!scene) {
      return;
    }
    const next = cloneTheme(draft);
    const nextScene = next.scenes[mapping.scene];
    const current = nextScene.leds[index] ?? defaultTrack("#22C55E");
    nextScene.leds[index] = normalizeTrack(current, patch);
    setDraft(next);
  };

  const toggleLed = (index: number) => {
    if (!scene) {
      return;
    }
    const next = cloneTheme(draft);
    const nextScene = next.scenes[mapping.scene];
    nextScene.leds[index] = nextScene.leds[index]
      ? null
      : defaultTrack(["#EF4444", "#F59E0B", "#22C55E"][index]);
    setDraft(next);
  };

  const updateMapping = (patch: Partial<ThemeFile["states"][string]>) => {
    const next = cloneTheme(draft);
    next.states[selectedState] = { ...next.states[selectedState], ...patch };
    setDraft(next);
  };

  const applyMotion = (preset: (typeof motionPresets)[number]) => {
    if (!scene) {
      return;
    }
    const next = cloneTheme(draft);
    const nextScene = next.scenes[mapping.scene];
    nextScene.leds = nextScene.leds.map((item, index) => {
      const current =
        item ?? defaultTrack(["#EF4444", "#F59E0B", "#22C55E"][index]);
      const patch: Partial<LedTrack> = { curve: preset.curve };
      if (preset.curve !== "CONSTANT" && !current.low) {
        patch.low = darkenHex(current.high, 0.4);
      }
      const normalized = normalizeTrack(current, patch);
      if (normalized.curve !== "CONSTANT") {
        normalized.phase_deg = preset.value === "flow" ? index * 120 : 0;
      }
      return normalized;
    }) as ThemeFile["scenes"][string]["leds"];
    setDraft(next);
  };

  const applySpeed = (periodMs: number) => {
    if (!scene) {
      return;
    }
    const next = cloneTheme(draft);
    next.scenes[mapping.scene].leds = next.scenes[mapping.scene].leds.map(
      (item) =>
        item && item.curve !== "CONSTANT"
          ? { ...item, period_ms: periodMs }
          : item
    ) as ThemeFile["scenes"][string]["leds"];
    setDraft(next);
  };

  const applySound = (preset: keyof typeof SOUND_PRESETS) => {
    if (!scene) {
      return;
    }
    const next = cloneTheme(draft);
    next.scenes[mapping.scene].buzzer = SOUND_PRESETS[preset]
      ? (JSON.parse(JSON.stringify(SOUND_PRESETS[preset])) as NonNullable<
          ThemeFile["scenes"][string]["buzzer"]
        >)
      : null;
    setDraft(next);
  };

  const addCustomState = () => {
    const state = newState.trim().toUpperCase();
    if (!THEME_NAME_PATTERN.test(state) || draft.states[state]) {
      notify({
        tone: "error",
        title: "无法添加状态",
        message: draft.states[state]
          ? "这个状态已经存在"
          : "请使用字母、数字、下划线或连字符",
      });
      return;
    }
    const next = cloneTheme(draft);
    const sceneName = `custom-${state.toLowerCase()}`;
    next.scenes[sceneName] = scene
      ? (JSON.parse(JSON.stringify(scene)) as typeof scene)
      : { leds: [null, null, null], buzzer: null };
    next.states[state] = { scene: sceneName };
    setDraft(next);
    setSelectedState(state);
    setNewState("");
  };

  const deleteCustomState = () => {
    if (
      standardStates.includes(selectedState as (typeof standardStates)[number])
    ) {
      return;
    }
    const next = cloneTheme(draft);
    const sceneName = next.states[selectedState]?.scene;
    delete next.states[selectedState];
    if (
      sceneName &&
      !Object.values(next.states).some((value) => value.scene === sceneName)
    ) {
      delete next.scenes[sceneName];
    }
    setDraft(next);
    setSelectedState("WORKING");
  };

  const makeSceneIndependent = () => {
    if (!(scene && mapping)) {
      return;
    }
    const next = cloneTheme(draft);
    const baseName = `${mapping.scene}-${selectedState.toLowerCase()}`;
    let sceneName = baseName;
    let suffix = 2;
    while (next.scenes[sceneName]) {
      sceneName = `${baseName}-${suffix}`;
      suffix += 1;
    }
    next.scenes[sceneName] = JSON.parse(JSON.stringify(scene)) as typeof scene;
    next.states[selectedState] = { ...mapping, scene: sceneName };
    setDraft(next);
  };

  const applyRelation = (
    relation: "sync" | "top-down" | "bottom-up" | "staggered"
  ) => {
    if (!scene) {
      return;
    }
    const next = cloneTheme(draft);
    const nextScene = next.scenes[mapping.scene];
    const phases = relationPhases[relation];
    nextScene.leds = nextScene.leds.map((item, index) =>
      item && item.curve !== "CONSTANT"
        ? { ...item, phase_deg: phases[index] }
        : item
    ) as ThemeFile["scenes"][string]["leds"];
    setDraft(next);
  };

  const updateBuzzer = (
    patch: Partial<ThemeFile["scenes"][string]["buzzer"]>
  ) => {
    if (!scene?.buzzer) {
      return;
    }
    const next = cloneTheme(draft);
    next.scenes[mapping.scene].buzzer = { ...scene.buzzer, ...patch };
    setDraft(next);
  };

  const save = async () => {
    if (!THEME_NAME_PATTERN.test(draft.theme.name)) {
      notify({
        tone: "error",
        title: "主题名格式不正确",
        message: "仅支持字母、数字、下划线和连字符",
      });
      return;
    }
    setSaving(true);
    try {
      const name = await api.importTheme(JSON.stringify(draft, null, 2));
      notify({ tone: "success", title: "主题已保存", message: name });
      if (source) {
        localStorage.removeItem(`ailight-theme-draft:${source.theme.name}`);
      }
      await onSaved();
      onClose();
    } catch (error) {
      notify({
        tone: "error",
        title: "主题保存失败",
        message: asAppError(error).message,
      });
    } finally {
      setSaving(false);
    }
  };

  const requestClose = () => {
    if (!source) {
      onClose();
      return;
    }
    const initial = cloneTheme(source);
    initial.theme.name =
      source.theme.name === "default"
        ? "my-theme"
        : `${source.theme.name}-copy`;
    const changed = JSON.stringify(initial) !== JSON.stringify(draft);
    if (!changed) {
      onClose();
      return;
    }
    setConfirmingClose(true);
  };

  const previewDraft = async () => {
    setPreviewing(true);
    try {
      await api.previewThemeDraft(selectedState, JSON.stringify(draft));
    } catch (error) {
      notify({
        tone: "error",
        title: "无法试听当前效果",
        message: asAppError(error).message,
      });
    } finally {
      setPreviewing(false);
    }
  };

  const mixFromTheme = async () => {
    try {
      const sourceTheme = JSON.parse(await api.getTheme(mixTheme)) as ThemeFile;
      const sourceMapping = sourceTheme.states[mixState];
      const sourceScene = sourceMapping
        ? sourceTheme.scenes[sourceMapping.scene]
        : null;
      if (!(sourceMapping && sourceScene)) {
        throw new Error(`${mixTheme} 没有 ${mixState} 效果`);
      }
      const next = cloneTheme(draft);
      const baseName = `${mixTheme}-${mixState.toLowerCase()}`;
      let sceneName = baseName;
      let suffix = 2;
      while (next.scenes[sceneName]) {
        sceneName = `${baseName}-${suffix}`;
        suffix += 1;
      }
      next.scenes[sceneName] = JSON.parse(
        JSON.stringify(sourceScene)
      ) as typeof sourceScene;
      next.states[selectedState] = {
        ...sourceMapping,
        scene: sceneName,
      };
      setDraft(next);
      notify({
        tone: "success",
        title: "已借用效果",
        message: `${displayNames[mixTheme] ?? mixTheme} · ${stateLabels[mixState]?.title ?? mixState} → ${stateLabels[selectedState]?.title ?? selectedState}`,
      });
    } catch (error) {
      notify({
        tone: "error",
        title: "无法借用效果",
        message: asAppError(error).message,
      });
    }
  };

  const selectedIsStandard = standardStates.includes(
    selectedState as (typeof standardStates)[number]
  );

  if (confirmingClose) {
    return (
      <Dialog
        footer={
          <>
            <ActionButton onClick={() => setConfirmingClose(false)}>
              继续编辑
            </ActionButton>
            <ActionButton onClick={onClose} tone="danger">
              放弃修改
            </ActionButton>
          </>
        }
        onClose={() => setConfirmingClose(false)}
        open={open}
        title="放弃尚未保存的修改？"
      >
        <p className="te-discard-copy">
          当前主题的修改尚未保存，关闭后将丢失。
        </p>
      </Dialog>
    );
  }

  return (
    <Dialog
      description="为每个状态挑一套灯光与声音，所见即所得。"
      footer={
        <>
          <ActionButton onClick={requestClose}>取消</ActionButton>
          <ActionButton
            busy={saving}
            onClick={() => runAsync(save())}
            tone="primary"
          >
            保存主题
          </ActionButton>
        </>
      }
      onClose={requestClose}
      open={open}
      size="large"
      title="主题创作器"
    >
      <div className="te-topbar">
        <div className="field field--grow">
          <label htmlFor="theme-name">主题标识</label>
          <input
            id="theme-name"
            onChange={(event) =>
              setDraft({
                ...draft,
                theme: { ...draft.theme, name: event.target.value },
              })
            }
            value={draft.theme.name}
          />
          <small>用于保存和导入；支持字母、数字、下划线和连字符</small>
        </div>
      </div>

      <section className="te-section">
        <div className="te-section__head">
          <h3>为哪个状态设计？</h3>
          <p>标准状态固定保留，你也可以添加自己的状态。</p>
        </div>
        <div aria-label="业务状态" className="te-state-chips" role="tablist">
          {Object.keys(draft.states).map((state) => {
            const meta = stateLabels[state] ?? {
              title: state,
              tagline: "自定义状态",
              accent: customAccent,
            };
            return (
              <button
                aria-selected={selectedState === state}
                className={cn(
                  "te-state-chip",
                  selectedState === state && "is-active"
                )}
                key={state}
                onClick={() => setSelectedState(state)}
                role="tab"
                type="button"
              >
                <span
                  aria-hidden="true"
                  className="te-state-chip__dot"
                  style={{ background: meta.accent }}
                />
                <span className="te-state-chip__title">{meta.title}</span>
                {standardStates.includes(
                  state as (typeof standardStates)[number]
                ) ? null : (
                  <span className="te-state-chip__code">{state}</span>
                )}
              </button>
            );
          })}
        </div>
        <div className="te-state-actions">
          <div className="te-state-new">
            <input
              aria-label="新增自定义状态"
              onChange={(event) => setNewState(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  addCustomState();
                }
              }}
              placeholder="状态标识，例如 REVIEW（等待审核）"
              value={newState}
            />
            <ActionButton disabled={!newState.trim()} onClick={addCustomState}>
              <Plus size={16} /> 添加
            </ActionButton>
          </div>
          {selectedIsStandard ? null : (
            <ActionButton onClick={deleteCustomState} tone="danger">
              <Trash2 size={16} /> 删除这个状态
            </ActionButton>
          )}
        </div>
      </section>

      {scene ? (
        <div className="te-layout">
          <div className="te-controls">
            <div className="te-borrow">
              <button
                aria-expanded={mixOpen}
                className="te-borrow__trigger"
                onClick={() => setMixOpen((value) => !value)}
                type="button"
              >
                <Wand2 aria-hidden="true" size={16} />
                <span className="te-borrow__heading">
                  <strong>借用主题效果</strong>
                  <small>复制其他主题的灯光与声音到当前状态</small>
                </span>
                <ChevronDown
                  aria-hidden="true"
                  className={cn("te-chevron", mixOpen && "is-open")}
                  size={16}
                />
              </button>
              {mixOpen ? (
                <div className="te-borrow__panel">
                  <div className="te-borrow__fields">
                    <div className="field">
                      <label htmlFor="mix-theme">来源主题</label>
                      <select
                        id="mix-theme"
                        onChange={(event) => setMixTheme(event.target.value)}
                        value={mixTheme}
                      >
                        {availableThemes.map((theme) => (
                          <option key={theme.name} value={theme.name}>
                            {displayNames[theme.name] ?? theme.name}
                          </option>
                        ))}
                      </select>
                    </div>
                    <div className="field">
                      <label htmlFor="mix-state">来源状态</label>
                      <select
                        id="mix-state"
                        onChange={(event) => setMixState(event.target.value)}
                        value={mixState}
                      >
                        {["IDLE", "WORKING", "WAITING", "SUCCESS", "ERROR"].map(
                          (state) => (
                            <option key={state} value={state}>
                              {stateLabels[state]?.title ?? state}
                            </option>
                          )
                        )}
                      </select>
                    </div>
                  </div>
                  <div className="te-borrow__action">
                    <span>
                      将覆盖：当前主题 ·{" "}
                      {stateLabels[selectedState]?.title ?? selectedState}
                    </span>
                    <ActionButton onClick={() => runAsync(mixFromTheme())}>
                      借用此效果
                    </ActionButton>
                  </div>
                </div>
              ) : null}
            </div>

            <fieldset className="te-group">
              <legend>灯光怎么动？</legend>
              <div className="te-motion-grid">
                {motionPresets.map((preset) => {
                  const active = activeMotion === preset.value;
                  return (
                    <button
                      aria-pressed={active}
                      className={cn("te-motion-card", active && "is-active")}
                      key={preset.value}
                      onClick={() => applyMotion(preset)}
                      type="button"
                    >
                      <MotionGlyph active={active} curve={preset.curve} />
                      <strong>{preset.label}</strong>
                      <span>{preset.hint}</span>
                    </button>
                  );
                })}
              </div>
            </fieldset>

            {sceneMotionCurve === "CONSTANT" ? null : (
              <fieldset className="te-group">
                <legend>变化速度</legend>
                <div className="te-seg te-seg--3">
                  {[
                    {
                      value: "slow",
                      label: "舒缓",
                      hint: "约 2.8 秒",
                      ms: 2800,
                    },
                    {
                      value: "medium",
                      label: "适中",
                      hint: "约 1.4 秒",
                      ms: 1400,
                    },
                    {
                      value: "fast",
                      label: "活跃",
                      hint: "约 0.6 秒",
                      ms: 600,
                    },
                  ].map((option) => (
                    <button
                      aria-pressed={speedTier === option.value}
                      className={cn(
                        "te-seg__item",
                        speedTier === option.value && "is-active"
                      )}
                      key={option.value}
                      onClick={() => applySpeed(option.ms)}
                      type="button"
                    >
                      <strong>{option.label}</strong>
                      <span>{option.hint}</span>
                    </button>
                  ))}
                </div>
              </fieldset>
            )}

            <fieldset className="te-group">
              <legend>三颗灯的颜色与亮度</legend>
              <div className="te-led-rows">
                {[0, 1, 2].map((index) => {
                  const item = scene.leds[index] ?? null;
                  const label = ["顶", "中", "底"][index];
                  return (
                    <div className="te-led-row" key={index}>
                      <div className="te-led-row__head">
                        <span className="te-led-row__name">{label}灯</span>
                        <div className="te-led-row__meta">
                          {item ? (
                            <span className="te-led-row__motion">
                              {item.curve === "CONSTANT"
                                ? "常亮"
                                : (motionPresets.find(
                                    (preset) => preset.curve === item.curve
                                  )?.label ?? item.curve)}
                            </span>
                          ) : (
                            <span className="te-led-row__motion te-led-row__motion--off">
                              熄灭
                            </span>
                          )}
                          <button
                            className="te-led-row__toggle"
                            onClick={() => toggleLed(index)}
                            type="button"
                          >
                            {item ? "熄灭此灯" : "点亮此灯"}
                          </button>
                        </div>
                      </div>
                      <div className="te-led-row__body">
                        {item ? (
                          <>
                            <label
                              className="te-swatch"
                              style={{ background: item.high }}
                            >
                              <input
                                aria-label={`${label}灯颜色`}
                                onChange={(event) =>
                                  updateLedColor(index, {
                                    high: event.target.value,
                                  })
                                }
                                type="color"
                                value={item.high}
                              />
                            </label>
                            <label className="te-range">
                              <span>亮度</span>
                              <input
                                max="100"
                                min="0"
                                onChange={(event) =>
                                  updateLedColor(index, {
                                    brightness: Number(event.target.value),
                                  })
                                }
                                type="range"
                                value={item.brightness}
                              />
                              <output>{item.brightness}%</output>
                            </label>
                          </>
                        ) : (
                          <p className="te-led-row__off-hint">
                            点亮此灯后可设置颜色和亮度。
                          </p>
                        )}
                      </div>
                    </div>
                  );
                })}
              </div>
            </fieldset>

            {sceneMotionCurve === "CONSTANT" ? null : (
              <fieldset className="te-group">
                <legend>三灯怎么依次出现？</legend>
                <div className="te-seg te-seg--4">
                  {[
                    { value: "sync", label: "一起" },
                    { value: "top-down", label: "上→下" },
                    { value: "bottom-up", label: "下→上" },
                    { value: "staggered", label: "交错" },
                  ].map((option) => (
                    <button
                      aria-pressed={activeRelation === option.value}
                      className={cn(
                        "te-seg__item",
                        activeRelation === option.value && "is-active"
                      )}
                      key={option.value}
                      onClick={() =>
                        applyRelation(
                          option.value as
                            | "sync"
                            | "top-down"
                            | "bottom-up"
                            | "staggered"
                        )
                      }
                      type="button"
                    >
                      {option.label}
                    </button>
                  ))}
                </div>
              </fieldset>
            )}

            <fieldset className="te-group">
              <legend>
                <Music2 aria-hidden="true" size={14} /> 提示声音
              </legend>
              <div className="te-sound-grid">
                {soundOptions.map((option) => {
                  const active = activeSound === option.value;
                  const Icon = option.value === "silent" ? VolumeX : Volume2;
                  return (
                    <button
                      aria-pressed={active}
                      className={cn("te-sound-card", active && "is-active")}
                      key={option.value}
                      onClick={() => applySound(option.value)}
                      type="button"
                    >
                      <Icon aria-hidden="true" size={18} />
                      <strong>{option.label}</strong>
                      <span>{option.hint}</span>
                    </button>
                  );
                })}
              </div>
            </fieldset>

            <button
              aria-expanded={advanced}
              className="te-advanced-toggle"
              onClick={() => setAdvanced((value) => !value)}
              type="button"
            >
              <SlidersHorizontal aria-hidden="true" size={16} />
              <span>
                <strong>逐灯精确调整</strong>
                <small>单独调整每颗灯的运动、周期和切换参数</small>
              </span>
              <ChevronDown
                aria-hidden="true"
                className={cn("te-chevron", advanced && "is-open")}
                size={16}
              />
            </button>

            {advanced ? (
              <section className="te-advanced">
                <div className="te-seg te-seg--3">
                  {[0, 1, 2].map((index) => (
                    <button
                      aria-pressed={selectedLed === index}
                      className={cn(
                        "te-seg__item",
                        selectedLed === index && "is-active"
                      )}
                      key={index}
                      onClick={() => setSelectedLed(index)}
                      type="button"
                    >
                      {["顶", "中", "底"][index]}灯
                    </button>
                  ))}
                </div>
                <div className="field">
                  <label htmlFor="curve">为此灯选择运动方式</label>
                  <select
                    id="curve"
                    onChange={(event) =>
                      updateTrack({
                        curve: event.target.value as LedTrack["curve"],
                      })
                    }
                    value={trackCurve}
                  >
                    {[
                      "CONSTANT",
                      "SQUARE",
                      "TRIANGLE",
                      "SAW_UP",
                      "SAW_DOWN",
                    ].map((curve) => (
                      <option key={curve} value={curve}>
                        {
                          {
                            CONSTANT: "常亮",
                            SQUARE: "闪烁",
                            TRIANGLE: "呼吸",
                            SAW_UP: "渐亮",
                            SAW_DOWN: "渐弱",
                          }[curve]
                        }
                      </option>
                    ))}
                  </select>
                </div>
                <div className="te-grid2">
                  <div className="field">
                    <label htmlFor="low-color">低点颜色</label>
                    <input
                      id="low-color"
                      onChange={(event) =>
                        updateTrack({ low: event.target.value })
                      }
                      type="color"
                      value={track?.low ?? "#000000"}
                    />
                  </div>
                  <label className="te-range">
                    <span>周期</span>
                    <input
                      max="5000"
                      min="100"
                      onChange={(event) =>
                        updateTrack({ period_ms: Number(event.target.value) })
                      }
                      step="100"
                      type="range"
                      value={track?.period_ms ?? 1200}
                    />
                    <output>{track?.period_ms ?? 1200} ms</output>
                  </label>
                  <label className="te-range">
                    <span>出场时间</span>
                    <input
                      max="360"
                      min="0"
                      onChange={(event) =>
                        updateTrack({ phase_deg: Number(event.target.value) })
                      }
                      type="range"
                      value={track?.phase_deg ?? 0}
                    />
                    <output>{track?.phase_deg ?? 0}°</output>
                  </label>
                  {trackCurve === "SQUARE" ? (
                    <label className="te-range">
                      <span>占空比</span>
                      <input
                        max="99"
                        min="1"
                        onChange={(event) =>
                          updateTrack({
                            duty_percent: Number(event.target.value),
                          })
                        }
                        type="range"
                        value={track?.duty_percent ?? 50}
                      />
                      <output>{track?.duty_percent ?? 50}%</output>
                    </label>
                  ) : null}
                </div>
                <div className="te-grid2">
                  <div className="field">
                    <label htmlFor="repeat">播放次数（0 = 持续）</label>
                    <input
                      id="repeat"
                      min="0"
                      onChange={(event) =>
                        updateTrack({ repeat: Number(event.target.value) })
                      }
                      type="number"
                      value={track?.repeat ?? 0}
                    />
                  </div>
                  {trackCurve === "CONSTANT" ? null : (
                    <div className="field">
                      <label htmlFor="end-level">结束后的灯位</label>
                      <select
                        id="end-level"
                        onChange={(event) =>
                          updateTrack({
                            end_level: event.target
                              .value as LedTrack["end_level"],
                          })
                        }
                        value={track?.end_level ?? "OFF"}
                      >
                        <option value="OFF">熄灭</option>
                        <option value="LOW">停在暗色</option>
                        <option value="HIGH">停在亮色</option>
                      </select>
                    </div>
                  )}
                </div>

                <div className="te-section__head">
                  <h3>状态切换</h3>
                </div>
                <div className="te-grid2">
                  <div className="field">
                    <label htmlFor="transition-ms">过渡时长</label>
                    <input
                      id="transition-ms"
                      max="2500"
                      min="0"
                      onChange={(event) =>
                        updateMapping({
                          transition_ms: Number(event.target.value),
                        })
                      }
                      type="number"
                      value={mapping.transition_ms ?? 0}
                    />
                    <small>切换到这个状态时，灯光柔和过渡的时长。</small>
                  </div>
                  <div className="field">
                    <label htmlFor="hold-ms">终态驻留</label>
                    <input
                      disabled={
                        selectedState !== "SUCCESS" && selectedState !== "ERROR"
                      }
                      id="hold-ms"
                      min="0"
                      onChange={(event) =>
                        updateMapping({
                          hold_ms: Number(event.target.value),
                        })
                      }
                      type="number"
                      value={mapping.hold_ms ?? 0}
                    />
                    <small>亮起后停留多久再回落空闲；0 表示不自动回落。</small>
                  </div>
                </div>

                <div className="te-section__head">
                  <h3>提示音细节</h3>
                </div>
                {scene.buzzer ? (
                  <fieldset className="te-group te-buzz">
                    <div className="te-grid2">
                      <div className="field">
                        <label htmlFor="buzzer-frequency">频率 (Hz)</label>
                        <input
                          id="buzzer-frequency"
                          min="0"
                          onChange={(event) =>
                            updateBuzzer({
                              segments: [
                                {
                                  ...buzzerSegment,
                                  frequency_hz: Number(event.target.value),
                                },
                              ],
                            })
                          }
                          type="number"
                          value={buzzerSegment.frequency_hz}
                        />
                      </div>
                      <div className="field">
                        <label htmlFor="buzzer-duration">时长 (ms)</label>
                        <input
                          id="buzzer-duration"
                          min="1"
                          onChange={(event) =>
                            updateBuzzer({
                              segments: [
                                {
                                  ...buzzerSegment,
                                  duration_ms: Number(event.target.value),
                                },
                              ],
                            })
                          }
                          type="number"
                          value={buzzerSegment.duration_ms}
                        />
                      </div>
                      <label className="te-range">
                        <span>音量</span>
                        <input
                          max="100"
                          min="0"
                          onChange={(event) =>
                            updateBuzzer({
                              segments: [
                                {
                                  ...buzzerSegment,
                                  volume: Number(event.target.value),
                                },
                              ],
                            })
                          }
                          type="range"
                          value={buzzerSegment.volume}
                        />
                        <output>{buzzerSegment.volume}%</output>
                      </label>
                      <div className="field">
                        <label htmlFor="buzzer-repeat">重复次数</label>
                        <input
                          id="buzzer-repeat"
                          min="0"
                          onChange={(event) =>
                            updateBuzzer({ repeat: Number(event.target.value) })
                          }
                          type="number"
                          value={scene.buzzer.repeat ?? 0}
                        />
                      </div>
                    </div>
                  </fieldset>
                ) : (
                  <p className="te-hint">
                    当前无声；选择一种声音后可在此微调。
                  </p>
                )}

                {sceneReferences.length > 1 ? (
                  <div className="te-shared">
                    <span>
                      多个状态共用这套效果：{sceneReferences.join("、")}
                    </span>
                    <ActionButton onClick={makeSceneIndependent}>
                      让本状态独立
                    </ActionButton>
                  </div>
                ) : null}

                <details className="te-json">
                  <summary>
                    查看效果库（共 {Object.keys(draft.scenes).length} 个）
                  </summary>
                  <dl>
                    {Object.keys(draft.scenes).map((sceneName) => {
                      const references = Object.entries(draft.states)
                        .filter(([, value]) => value.scene === sceneName)
                        .map(([state]) => state);
                      return (
                        <div key={sceneName}>
                          <dt>{sceneName}</dt>
                          <dd>
                            {references.length
                              ? references.join("、")
                              : "尚未使用"}
                          </dd>
                        </div>
                      );
                    })}
                  </dl>
                </details>
                <details className="te-json">
                  <summary>查看生成的主题 JSON</summary>
                  <pre>
                    <code>{JSON.stringify(draft, null, 2)}</code>
                  </pre>
                </details>
              </section>
            ) : null}
          </div>

          <aside className="te-stage">
            <div className="te-stage__head">
              <span>当前预览</span>
              <strong>
                {stateLabels[selectedState]?.title ?? selectedState}
              </strong>
            </div>
            <LivePreview scene={scene} />
            {sceneReferences.length > 1 ? (
              <div className="te-shared te-shared--stage">
                <span>{sceneReferences.length} 个状态共用</span>
                <ActionButton onClick={makeSceneIndependent}>独立</ActionButton>
              </div>
            ) : null}
            <ActionButton
              busy={previewing}
              className="te-stage__listen"
              disabled={!snapshot?.device.connected}
              onClick={() => runAsync(previewDraft())}
              tone="primary"
            >
              {snapshot?.device.connected ? "在设备上试听" : "连接设备后可试听"}
            </ActionButton>
            <small className="te-stage__note">
              软件预览会模拟真实动效；连上灯牌即可体验实际灯光与声音。
            </small>
          </aside>
        </div>
      ) : (
        <InlineAlert title="这个状态还没有灯效" tone="info">
          在本主题里为这个状态选择一套动效即可开始。
        </InlineAlert>
      )}
    </Dialog>
  );
}

function ThemeCardItem({
  active,
  applying,
  deleting,
  index,
  onApply,
  onDelete,
  onInspect,
  theme,
}: {
  active: boolean;
  applying: boolean;
  deleting: boolean;
  index: number;
  onApply: (name: string) => Promise<void>;
  onDelete: (theme: ThemeMeta) => void;
  onInspect: (name: string) => Promise<void>;
  theme: ThemeMeta;
}) {
  return (
    <Card
      className={`theme-card theme-card--${index % 6} ${active ? "is-active" : ""}`}
      onClick={() => runAsync(onInspect(theme.name))}
    >
      <div className="theme-card__preview">
        <Sparkles aria-hidden="true" />
        <TrafficBadge compact state={previewState(index)} />
      </div>
      <div className="theme-card__heading">
        <div>
          <h2>{displayNames[theme.name] ?? theme.name}</h2>
          <p>{descriptions[theme.name] ?? "用户自定义灯效主题"}</p>
        </div>
        <StatusTag tone={theme.builtin ? "neutral" : "warning"}>
          {theme.builtin ? "内置" : "用户"}
        </StatusTag>
      </div>
      <div className="theme-card__actions">
        <ActionButton
          busy={applying}
          disabled={active || deleting}
          onClick={(event) => {
            event.stopPropagation();
            runAsync(onApply(theme.name));
          }}
          tone={active ? "primary" : "secondary"}
        >
          {active ? (
            <>
              <Check size={16} /> 正在使用
            </>
          ) : (
            "使用此主题"
          )}
        </ActionButton>
        {theme.builtin ? null : (
          <ActionButton
            aria-label={`删除主题 ${theme.name}`}
            busy={deleting}
            disabled={applying}
            onClick={(event) => {
              event.stopPropagation();
              onDelete(theme);
            }}
            tone="danger"
          >
            <Trash2 aria-hidden="true" size={16} /> 删除
          </ActionButton>
        )}
      </div>
    </Card>
  );
}

export function ThemesPage() {
  const { snapshot, notify, refresh } = useAppState();
  const [themes, setThemes] = useState<ThemeMeta[]>([]);
  const [selectedTheme, setSelectedTheme] = useState<ThemeFile | null>(null);
  const [loading, setLoading] = useState(true);
  const [applying, setApplying] = useState<string | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<ThemeMeta | null>(null);
  const [deleting, setDeleting] = useState<string | null>(null);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const [importOpen, setImportOpen] = useState(false);
  const [editorOpen, setEditorOpen] = useState(false);
  const [editorSource, setEditorSource] = useState<ThemeFile | null>(null);
  const [importContent, setImportContent] = useState("");
  const [importError, setImportError] = useState<string | null>(null);

  const loadThemes = useCallback(async () => {
    setLoading(true);
    try {
      setThemes(await api.getThemes());
    } catch (error) {
      notify({
        tone: "error",
        title: "主题加载失败",
        message: asAppError(error).message,
      });
    } finally {
      setLoading(false);
    }
  }, [notify]);

  useEffect(() => {
    runAsync(loadThemes());
  }, [loadThemes]);

  const inspect = async (name: string) => {
    try {
      setSelectedTheme(JSON.parse(await api.getTheme(name)) as ThemeFile);
    } catch (error) {
      notify({
        tone: "error",
        title: "无法读取主题",
        message: asAppError(error).message,
      });
    }
  };

  const openEditor = async (name: string) => {
    try {
      setEditorSource(JSON.parse(await api.getTheme(name)) as ThemeFile);
      setEditorOpen(true);
    } catch (error) {
      notify({
        tone: "error",
        title: "无法打开主题编辑器",
        message: asAppError(error).message,
      });
    }
  };

  const apply = async (name: string) => {
    setApplying(name);
    try {
      await api.setActiveTheme(name);
      await refresh();
      notify({ tone: "success", title: "主题已切换", message: name });
    } catch (error) {
      notify({
        tone: "error",
        title: "主题切换失败",
        message: asAppError(error).message,
      });
    } finally {
      setApplying(null);
    }
  };

  const importFile = async (event: ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    if (file) {
      setImportContent(await file.text());
    }
  };

  const doImport = async () => {
    setImportError(null);
    try {
      const name = await api.importTheme(importContent);
      await loadThemes();
      notify({ tone: "success", title: "主题导入成功", message: name });
      setImportOpen(false);
      setImportContent("");
    } catch (error) {
      setImportError(asAppError(error).message);
    }
  };

  const requestDelete = (theme: ThemeMeta) => {
    setDeleteError(null);
    setDeleteTarget(theme);
  };

  const closeDelete = () => {
    if (!deleting) {
      setDeleteTarget(null);
      setDeleteError(null);
    }
  };

  const confirmDelete = async () => {
    if (!deleteTarget) {
      return;
    }
    const name = deleteTarget.name;
    setDeleting(name);
    setDeleteError(null);
    try {
      await api.deleteTheme(name);
      if (selectedTheme?.theme.name === name) {
        setSelectedTheme(null);
      }
      await Promise.all([loadThemes(), refresh()]);
      setDeleteTarget(null);
      notify({ tone: "success", title: "主题已删除", message: name });
    } catch (error) {
      setDeleteError(asAppError(error).message);
    } finally {
      setDeleting(null);
    }
  };

  let themeContent: ReactNode;
  if (loading) {
    themeContent = (
      <div className="theme-grid">
        <Card />
        <Card />
        <Card />
      </div>
    );
  } else if (themes.length === 0) {
    themeContent = (
      <Card>
        <EmptyState
          description="导入一个 .ailight-theme.json 文件开始使用。"
          icon={<FileJson />}
          title="还没有可用主题"
        />
      </Card>
    );
  } else {
    themeContent = (
      <div className="theme-layout">
        <div className="theme-grid">
          {themes.map((theme, index) => (
            <ThemeCardItem
              active={snapshot?.activeTheme === theme.name}
              applying={applying === theme.name}
              deleting={deleting === theme.name}
              index={index}
              key={theme.name}
              onApply={apply}
              onDelete={requestDelete}
              onInspect={inspect}
              theme={theme}
            />
          ))}
        </div>
        {selectedTheme ? (
          <aside className="theme-detail">
            <div>
              <span className="section-kicker">主题详情</span>
              <h2>{selectedTheme.theme.name}</h2>
              <p>
                {Object.keys(selectedTheme.scenes).length} 组灯光与声音效果 ·{" "}
                {Object.keys(selectedTheme.states).length} 个状态
              </p>
            </div>
            <dl>
              {Object.entries(selectedTheme.states).map(([state, mapping]) => (
                <div key={state}>
                  <dt>{state}</dt>
                  <dd>{mapping.scene}</dd>
                </div>
              ))}
            </dl>
            <ActionButton
              onClick={() => runAsync(openEditor(selectedTheme.theme.name))}
            >
              <Pencil size={16} /> 以此主题创建
            </ActionButton>
          </aside>
        ) : null}
      </div>
    );
  }

  return (
    <div className="page-stack">
      <PageHeader
        actions={
          <>
            <ActionButton
              onClick={() =>
                runAsync(openEditor(snapshot?.activeTheme ?? "default"))
              }
            >
              <Pencil size={16} /> 以当前主题创建
            </ActionButton>
            <ActionButton onClick={() => setImportOpen(true)} tone="primary">
              <Import size={16} /> 导入主题
            </ActionButton>
          </>
        }
        description="浏览、试听并定制状态灯效"
        title="主题"
      />
      {themeContent}
      <Dialog
        description={
          deleteTarget && snapshot?.activeTheme === deleteTarget.name
            ? "删除后将自动切换到默认主题，并立即更新当前灯效。"
            : "此操作会永久删除本机保存的主题文件。"
        }
        footer={
          <>
            <ActionButton disabled={Boolean(deleting)} onClick={closeDelete}>
              取消
            </ActionButton>
            <ActionButton
              busy={Boolean(deleting)}
              onClick={() => runAsync(confirmDelete())}
              tone="danger"
            >
              删除主题
            </ActionButton>
          </>
        }
        onClose={closeDelete}
        open={Boolean(deleteTarget)}
        title={`删除主题“${deleteTarget?.name ?? ""}”？`}
      >
        {deleteError ? (
          <InlineAlert title="主题删除失败">{deleteError}</InlineAlert>
        ) : (
          <p>内置主题不受影响；删除后无法恢复。</p>
        )}
      </Dialog>
      <Dialog
        description="选择主题文件，或粘贴完整 JSON 内容。"
        footer={
          <>
            <ActionButton onClick={() => setImportOpen(false)}>
              取消
            </ActionButton>
            <ActionButton
              disabled={!importContent.trim()}
              onClick={() => runAsync(doImport())}
              tone="primary"
            >
              导入
            </ActionButton>
          </>
        }
        onClose={() => setImportOpen(false)}
        open={importOpen}
        title="导入主题"
      >
        <div className="import-panel">
          <label className="file-picker">
            <Plus size={18} /> 选择 .ailight-theme.json 文件
            <input
              accept=".json,.ailight-theme.json"
              onChange={(event) => runAsync(importFile(event))}
              type="file"
            />
          </label>
          <span>或</span>
          <div className="field">
            <label htmlFor="theme-json">主题 JSON</label>
            <textarea
              id="theme-json"
              onChange={(event) => setImportContent(event.target.value)}
              placeholder="粘贴 JSON…"
              rows={12}
              value={importContent}
            />
          </div>
          {importError ? (
            <InlineAlert title="主题无法导入">{importError}</InlineAlert>
          ) : null}
        </div>
      </Dialog>
      <ThemeEditor
        availableThemes={themes}
        onClose={() => setEditorOpen(false)}
        onSaved={loadThemes}
        open={editorOpen}
        source={editorSource}
      />
    </div>
  );
}

export const Component = ThemesPage;
