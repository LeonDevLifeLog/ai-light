import {
  Check,
  FileJson,
  Import,
  Pencil,
  Plus,
  Sparkles,
  Trash2,
} from "lucide-react";
import {
  type ChangeEvent,
  type ReactNode,
  useCallback,
  useEffect,
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
import { runAsync } from "@/lib/utils";

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

// biome-ignore lint/complexity/noExcessiveCognitiveComplexity: the creator keeps state, presets, preview, and progressive controls in one flow.
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
  const [selectedState, setSelectedState] = useState<string>("WORKING");
  const [selectedLed, setSelectedLed] = useState(0);
  const [mode, setMode] = useState<"quick" | "workbench">("quick");
  const [saving, setSaving] = useState(false);
  const [previewing, setPreviewing] = useState(false);
  const [newState, setNewState] = useState("");
  const [mixTheme, setMixTheme] = useState("default");
  const [mixState, setMixState] = useState("WORKING");

  useEffect(() => {
    if (source && open) {
      const next = cloneTheme(source);
      next.theme.name = `${source.theme.name}-custom`;
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
      setMode("quick");
      setNewState("");
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
      const normalized = normalizeTrack(current, { curve: preset.curve });
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

  const applySound = (preset: "silent" | "gentle" | "confirm" | "alert") => {
    if (!scene) {
      return;
    }
    const sounds: Record<typeof preset, ThemeFile["scenes"][string]["buzzer"]> =
      {
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
    const next = cloneTheme(draft);
    next.scenes[mapping.scene].buzzer = sounds[preset];
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
    const phases = {
      sync: [0, 0, 0],
      "top-down": [0, 120, 240],
      "bottom-up": [240, 120, 0],
      staggered: [0, 180, 90],
    }[relation];
    nextScene.leds = nextScene.leds.map((item, index) =>
      item && item.curve !== "CONSTANT"
        ? { ...item, phase_deg: phases[index] }
        : item
    ) as ThemeFile["scenes"][string]["leds"];
    setDraft(next);
  };

  const setBuzzerEnabled = (enabled: boolean) => {
    if (!scene) {
      return;
    }
    const next = cloneTheme(draft);
    next.scenes[mapping.scene].buzzer = enabled
      ? {
          start_delay_ms: 0,
          repeat: 1,
          segments: [{ frequency_hz: 1800, duration_ms: 150, volume: 60 }],
        }
      : null;
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
    initial.theme.name = `${source.theme.name}-custom`;
    const changed = JSON.stringify(initial) !== JSON.stringify(draft);
    // biome-ignore lint/suspicious/noAlert: native confirmation prevents accidental loss before the dedicated draft dialog lands.
    if (!changed || window.confirm("放弃尚未保存的主题修改？")) {
      onClose();
    }
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
        title: "效果已混搭",
        message: `${mixTheme} · ${mixState} → ${selectedState}`,
      });
    } catch (error) {
      notify({
        tone: "error",
        title: "无法混搭效果",
        message: asAppError(error).message,
      });
    }
  };

  return (
    <Dialog
      description="从喜欢的主题开始，先选感觉，再按需要调整每颗灯。"
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
      <div className="editor-toolbar">
        <div className="field field--grow">
          <label htmlFor="theme-name">主题名称</label>
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
        </div>
        <fieldset aria-label="编辑模式" className="segmented-control">
          <button
            aria-pressed={mode === "quick"}
            onClick={() => setMode("quick")}
            type="button"
          >
            快速创作
          </button>
          <button
            aria-pressed={mode === "workbench"}
            onClick={() => setMode("workbench")}
            type="button"
          >
            轨道工作台
          </button>
        </fieldset>
      </div>
      <div aria-label="业务状态" className="editor-state-tabs" role="tablist">
        {Object.keys(draft.states).map((state) => (
          <button
            aria-selected={selectedState === state}
            key={state}
            onClick={() => setSelectedState(state)}
            role="tab"
            type="button"
          >
            <TrafficBadge compact state={state} />
            <span>{state}</span>
          </button>
        ))}
      </div>
      <div className="custom-state-row">
        <div className="field field--grow">
          <label htmlFor="new-custom-state">增加个性化状态</label>
          <input
            id="new-custom-state"
            onChange={(event) => setNewState(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                addCustomState();
              }
            }}
            placeholder="例如 REVIEW、DEPLOY"
            value={newState}
          />
        </div>
        <ActionButton disabled={!newState.trim()} onClick={addCustomState}>
          <Plus size={16} /> 添加状态
        </ActionButton>
        {standardStates.includes(
          selectedState as (typeof standardStates)[number]
        ) ? null : (
          <ActionButton onClick={deleteCustomState}>
            <Trash2 size={16} /> 删除当前状态
          </ActionButton>
        )}
      </div>
      <details className="theme-mixer">
        <summary>从其他主题借用一个效果</summary>
        <div>
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
            <label htmlFor="mix-state">借用状态</label>
            <select
              id="mix-state"
              onChange={(event) => setMixState(event.target.value)}
              value={mixState}
            >
              {standardStates.map((state) => (
                <option key={state}>{state}</option>
              ))}
            </select>
          </div>
          <ActionButton onClick={() => runAsync(mixFromTheme())}>
            应用到 {selectedState}
          </ActionButton>
        </div>
      </details>
      {scene ? (
        <div className="editor-layout">
          <div className="editor-controls">
            {sceneReferences.length > 1 ? (
              <div className="shared-scene-notice">
                <InlineAlert title="这个效果被多个状态共用" tone="info">
                  修改会同时影响 {sceneReferences.join("、")}。
                </InlineAlert>
                <ActionButton onClick={makeSceneIndependent}>
                  复制为当前状态的独立效果
                </ActionButton>
              </div>
            ) : null}
            {mode === "quick" ? (
              <>
                <fieldset>
                  <legend>想让灯光怎么动？</legend>
                  <div className="motion-preset-grid">
                    {motionPresets.map((preset) => (
                      <button
                        key={preset.value}
                        onClick={() => applyMotion(preset)}
                        type="button"
                      >
                        <strong>{preset.label}</strong>
                        <span>{preset.hint}</span>
                      </button>
                    ))}
                  </div>
                </fieldset>
                <fieldset>
                  <legend>变化速度</legend>
                  <div className="segmented-control segmented-control--wide">
                    <button onClick={() => applySpeed(2800)} type="button">
                      舒缓
                    </button>
                    <button onClick={() => applySpeed(1400)} type="button">
                      适中
                    </button>
                    <button onClick={() => applySpeed(600)} type="button">
                      活跃
                    </button>
                  </div>
                </fieldset>
              </>
            ) : (
              <p className="editor-help">
                分别选择顶灯、中灯和底灯，调整各自的运动、颜色和出场时间。
              </p>
            )}
            <fieldset>
              <legend>
                {mode === "quick" ? "选择要换颜色的灯" : "正在调整"}
              </legend>
              <div className="segmented-control segmented-control--wide">
                {[0, 1, 2].map((index) => (
                  <button
                    aria-pressed={selectedLed === index}
                    key={index}
                    onClick={() => setSelectedLed(index)}
                    type="button"
                  >
                    {["顶灯", "中灯", "底灯"][index]}
                  </button>
                ))}
              </div>
            </fieldset>
            {mode === "workbench" ? (
              <div className="field">
                <label htmlFor="curve">运动方式</label>
                <select
                  id="curve"
                  onChange={(event) =>
                    updateTrack({
                      curve: event.target.value as LedTrack["curve"],
                    })
                  }
                  value={trackCurve}
                >
                  {["CONSTANT", "SQUARE", "TRIANGLE", "SAW_UP", "SAW_DOWN"].map(
                    (curve) => (
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
                    )
                  )}
                </select>
              </div>
            ) : null}
            <div className="color-grid">
              <div className="field">
                <label htmlFor="high-color">高点颜色</label>
                <input
                  id="high-color"
                  onChange={(event) =>
                    updateTrack({ high: event.target.value })
                  }
                  type="color"
                  value={track?.high ?? "#22c55e"}
                />
              </div>
              {mode === "workbench" && trackCurve !== "CONSTANT" ? (
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
              ) : null}
            </div>
            <label className="range-field">
              亮度 <output>{track?.brightness ?? 70}%</output>
              <input
                max="100"
                min="0"
                onChange={(event) =>
                  updateTrack({ brightness: Number(event.target.value) })
                }
                type="range"
                value={track?.brightness ?? 70}
              />
            </label>
            {mode === "workbench" && trackCurve !== "CONSTANT" ? (
              <label className="range-field">
                周期 <output>{track?.period_ms ?? 1200} ms</output>
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
              </label>
            ) : null}
            {mode === "workbench" && trackCurve !== "CONSTANT" ? (
              <>
                <label className="range-field">
                  出场时间 <output>{track?.phase_deg ?? 0}°</output>
                  <input
                    max="360"
                    min="0"
                    onChange={(event) =>
                      updateTrack({ phase_deg: Number(event.target.value) })
                    }
                    type="range"
                    value={track?.phase_deg ?? 0}
                  />
                </label>
                <div className="field">
                  <label htmlFor="repeat">重复次数（0 = 持续）</label>
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
              </>
            ) : null}
            <fieldset>
              <legend>灯光怎么依次出现？</legend>
              <div className="segmented-control segmented-control--wide">
                <button onClick={() => applyRelation("sync")} type="button">
                  一起
                </button>
                <button
                  disabled={trackCurve === "CONSTANT"}
                  onClick={() => applyRelation("top-down")}
                  type="button"
                >
                  从上往下
                </button>
                <button
                  disabled={trackCurve === "CONSTANT"}
                  onClick={() => applyRelation("bottom-up")}
                  type="button"
                >
                  从下往上
                </button>
                <button
                  disabled={trackCurve === "CONSTANT"}
                  onClick={() => applyRelation("staggered")}
                  type="button"
                >
                  交错
                </button>
              </div>
            </fieldset>
            {mode === "workbench" ? (
              <>
                <div className="editor-number-grid">
                  <div className="field">
                    <label htmlFor="transition-ms">过渡时长 (ms)</label>
                    <input
                      id="transition-ms"
                      min="0"
                      onChange={(event) =>
                        updateMapping({
                          transition_ms: Number(event.target.value),
                        })
                      }
                      type="number"
                      value={mapping.transition_ms ?? 0}
                    />
                  </div>
                  <div className="field">
                    <label htmlFor="hold-ms">终态驻留 (ms)</label>
                    <input
                      disabled={
                        selectedState !== "SUCCESS" && selectedState !== "ERROR"
                      }
                      id="hold-ms"
                      min="0"
                      onChange={(event) =>
                        updateMapping({ hold_ms: Number(event.target.value) })
                      }
                      type="number"
                      value={mapping.hold_ms ?? 0}
                    />
                  </div>
                </div>
                {trackCurve !== "CONSTANT" && (track?.repeat ?? 0) > 0 ? (
                  <div className="field">
                    <label htmlFor="end-level">重复结束后的灯位</label>
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
                      <option value="LOW">低点</option>
                      <option value="HIGH">高点</option>
                    </select>
                  </div>
                ) : null}
                <fieldset className="buzzer-editor">
                  <legend>蜂鸣轨道</legend>
                  <label className="checkbox-row">
                    <input
                      checked={Boolean(scene.buzzer)}
                      onChange={(event) =>
                        setBuzzerEnabled(event.target.checked)
                      }
                      type="checkbox"
                    />
                    启用蜂鸣
                  </label>
                  {scene.buzzer ? (
                    <div className="editor-number-grid">
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
                      <label className="range-field">
                        音量 <output>{buzzerSegment.volume}%</output>
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
                      </label>
                    </div>
                  ) : null}
                </fieldset>
              </>
            ) : (
              <fieldset>
                <legend>提示声音</legend>
                <div className="segmented-control segmented-control--wide">
                  <button onClick={() => applySound("silent")} type="button">
                    无声
                  </button>
                  <button onClick={() => applySound("gentle")} type="button">
                    轻提示
                  </button>
                  <button onClick={() => applySound("confirm")} type="button">
                    确认音
                  </button>
                  <button onClick={() => applySound("alert")} type="button">
                    警报音
                  </button>
                </div>
              </fieldset>
            )}
          </div>
          <div className="editor-preview">
            <span>当前颜色与灯位</span>
            <div
              aria-label="三灯颜色预览"
              className="light-preview-stack"
              role="img"
            >
              {["顶灯", "中灯", "底灯"].map((label, index) => {
                const item = scene.leds[index];
                return (
                  <i
                    aria-hidden="true"
                    key={label}
                    style={{
                      backgroundColor: item?.high ?? "#000000",
                      boxShadow: item
                        ? `0 0 ${Math.max(8, item.brightness / 2)}px ${item.high}`
                        : "none",
                      opacity: item
                        ? Math.max(0.08, item.brightness / 100)
                        : 0.08,
                    }}
                  />
                );
              })}
            </div>
            <code>{mapping.scene}</code>
            <small>
              软件预览用于确认颜色与灯位，真实动态和声音以设备为准。
            </small>
            <ActionButton
              busy={previewing}
              disabled={!snapshot?.device.connected}
              onClick={() => runAsync(previewDraft())}
              tone="primary"
            >
              {snapshot?.device.connected ? "在设备上试听" : "连接设备后可试听"}
            </ActionButton>
          </div>
        </div>
      ) : (
        <InlineAlert title="当前状态没有映射">
          请在 JSON 进阶编辑中添加对应 SCENE。
        </InlineAlert>
      )}
      <details className="scene-library">
        <summary>查看效果库（{Object.keys(draft.scenes).length} 个）</summary>
        <dl>
          {Object.keys(draft.scenes).map((sceneName) => {
            const references = Object.entries(draft.states)
              .filter(([, value]) => value.scene === sceneName)
              .map(([state]) => state);
            return (
              <div key={sceneName}>
                <dt>{sceneName}</dt>
                <dd>
                  {references.length ? references.join("、") : "尚未使用"}
                </dd>
              </div>
            );
          })}
        </dl>
      </details>
      {mode === "workbench" ? (
        <details className="json-preview">
          <summary>查看生成的主题 JSON</summary>
          <pre>
            <code>{JSON.stringify(draft, null, 2)}</code>
          </pre>
        </details>
      ) : null}
    </Dialog>
  );
}

function ThemeCardItem({
  active,
  applying,
  index,
  onApply,
  onInspect,
  theme,
}: {
  active: boolean;
  applying: boolean;
  index: number;
  onApply: (name: string) => Promise<void>;
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
      <ActionButton
        busy={applying}
        disabled={active}
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
    </Card>
  );
}

export function ThemesPage() {
  const { snapshot, notify, refresh } = useAppState();
  const [themes, setThemes] = useState<ThemeMeta[]>([]);
  const [selectedTheme, setSelectedTheme] = useState<ThemeFile | null>(null);
  const [loading, setLoading] = useState(true);
  const [applying, setApplying] = useState<string | null>(null);
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
              index={index}
              key={theme.name}
              onApply={apply}
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
                {Object.keys(selectedTheme.scenes).length} 个 SCENE ·{" "}
                {Object.keys(selectedTheme.states).length} 个状态映射
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
