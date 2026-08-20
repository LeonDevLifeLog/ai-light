import { Check, FileJson, Import, Pencil, Plus, Sparkles } from "lucide-react";
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

// biome-ignore lint/complexity/noExcessiveCognitiveComplexity: the editor mirrors the protocol's conditional field matrix in one form.
function ThemeEditor({
  open,
  source,
  onClose,
  onSaved,
}: {
  open: boolean;
  source: ThemeFile | null;
  onClose: () => void;
  onSaved: () => Promise<void>;
}) {
  const { notify } = useAppState();
  const [draft, setDraft] = useState<ThemeFile | null>(null);
  const [selectedState, setSelectedState] = useState<string>("WORKING");
  const [selectedLed, setSelectedLed] = useState(0);
  const [mode, setMode] = useState<"simple" | "advanced">("simple");
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (source && open) {
      const next = cloneTheme(source);
      next.theme.name = `${source.theme.name}-custom`;
      setDraft(next);
      setSelectedState("WORKING");
    }
  }, [source, open]);

  if (!draft) {
    return null;
  }
  const mapping = draft.states[selectedState];
  const scene = mapping ? draft.scenes[mapping.scene] : null;
  const track = scene?.leds[selectedLed] ?? null;
  const trackCurve = track?.curve ?? "CONSTANT";
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

  const applyRelation = (relation: "sync" | "chase") => {
    if (!scene) {
      return;
    }
    const next = cloneTheme(draft);
    const nextScene = next.scenes[mapping.scene];
    const base = track ?? defaultTrack("#22C55E");
    nextScene.leds = [0, 1, 2].map((index) => ({
      ...base,
      phase_deg:
        relation === "chase" && base.curve !== "CONSTANT"
          ? index * 120
          : base.phase_deg,
    })) as ThemeFile["scenes"][string]["leds"];
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

  return (
    <Dialog
      description="内置主题会另存为用户主题，不会覆盖原文件。"
      footer={
        <>
          <ActionButton onClick={onClose}>取消</ActionButton>
          <ActionButton
            busy={saving}
            onClick={() => runAsync(save())}
            tone="primary"
          >
            保存主题
          </ActionButton>
        </>
      }
      onClose={onClose}
      open={open}
      size="large"
      title="主题编辑器"
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
            aria-pressed={mode === "simple"}
            onClick={() => setMode("simple")}
            type="button"
          >
            简单
          </button>
          <button
            aria-pressed={mode === "advanced"}
            onClick={() => setMode("advanced")}
            type="button"
          >
            进阶
          </button>
        </fieldset>
      </div>
      <div aria-label="业务状态" className="editor-state-tabs" role="tablist">
        {standardStates.map((state) => (
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
      {scene ? (
        <div className="editor-layout">
          <div className="editor-controls">
            <fieldset>
              <legend>选择灯位</legend>
              <div className="segmented-control segmented-control--wide">
                {[0, 1, 2].map((index) => (
                  <button
                    aria-pressed={selectedLed === index}
                    key={index}
                    onClick={() => setSelectedLed(index)}
                    type="button"
                  >
                    {["红灯", "黄灯", "绿灯"][index]}
                  </button>
                ))}
              </div>
            </fieldset>
            <div className="field">
              <label htmlFor="curve">波形</label>
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
                    <option key={curve}>{curve}</option>
                  )
                )}
              </select>
            </div>
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
              {trackCurve === "CONSTANT" ? null : (
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
              )}
            </div>
            <label className="range-field">
              亮度 <output>{track?.brightness ?? 70}%</output>
              <input
                max="100"
                min="1"
                onChange={(event) =>
                  updateTrack({ brightness: Number(event.target.value) })
                }
                type="range"
                value={track?.brightness ?? 70}
              />
            </label>
            {trackCurve === "CONSTANT" ? null : (
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
            )}
            {mode === "advanced" && trackCurve !== "CONSTANT" ? (
              <>
                <label className="range-field">
                  相位 <output>{track?.phase_deg ?? 0}°</output>
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
              <legend>三灯关系</legend>
              <div className="segmented-control segmented-control--wide">
                <button onClick={() => applyRelation("sync")} type="button">
                  同步
                </button>
                <button
                  disabled={trackCurve === "CONSTANT"}
                  onClick={() => applyRelation("chase")}
                  type="button"
                >
                  依次追逐
                </button>
              </div>
            </fieldset>
            {mode === "advanced" ? (
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
                          min="1"
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
            ) : null}
          </div>
          <div className="editor-preview">
            <span>实时预览</span>
            <TrafficBadge orientation="vertical" state={selectedState} />
            <code>{mapping.scene}</code>
            <small>颜色和曲线以物理设备实际渲染为准</small>
          </div>
        </div>
      ) : (
        <InlineAlert title="当前状态没有映射">
          请在 JSON 进阶编辑中添加对应 SCENE。
        </InlineAlert>
      )}
      {mode === "advanced" ? (
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
              onClick={() =>
                runAsync(openEditor(snapshot?.activeTheme ?? "default"))
              }
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
              onClick={() => {
                setEditorSource(selectedTheme);
                setEditorOpen(true);
              }}
            >
              <Pencil size={16} /> 编辑当前主题
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
        onClose={() => setEditorOpen(false)}
        onSaved={loadThemes}
        open={editorOpen}
        source={editorSource}
      />
    </div>
  );
}

export const Component = ThemesPage;
