import {
  BookOpen,
  ChevronRight,
  ExternalLink,
  Monitor,
  Moon,
  Palette,
  RefreshCw,
  ShieldCheck,
  Sun,
  SunMoon,
  Volume2,
} from "lucide-react";
import { useEffect, useState } from "react";
import { Link } from "react-router";
import { useAppState } from "@/app/app-context";
import {
  ActionButton,
  Card,
  PageHeader,
  StatusTag,
  themeDisplayName,
} from "@/components/app-ui";
import {
  ToolchainDetailsList,
  toolchainStateCopy,
} from "@/features/toolchain/runtime-environment";
import type { AppConfig, ThemeFile, ToolchainStatus } from "@/lib/ailight";
import { api, asAppError } from "@/lib/ailight";
import { cn, runAsync } from "@/lib/utils";

function SettingRow({
  icon,
  title,
  description,
  stacked,
  children,
}: {
  icon: React.ReactNode;
  title: string;
  description?: string;
  stacked?: boolean;
  children: React.ReactNode;
}) {
  return (
    <div className={cn("setting-row", stacked && "setting-row--stacked")}>
      <div className="setting-row__icon">{icon}</div>
      <div className="setting-row__copy">
        <strong>{title}</strong>
        {description ? <span>{description}</span> : null}
      </div>
      <div className="setting-row__control">{children}</div>
    </div>
  );
}

function themePreview(file: ThemeFile): {
  swatches: Array<{ state: string; color: string }>;
  hasSound: boolean;
} {
  const preferred = ["WORKING", "SUCCESS", "ERROR", "WAITING", "IDLE"];
  const keys = preferred.filter((key) => file.states[key]);
  const pick = keys.length >= 3 ? keys : Object.keys(file.states).slice(0, 3);
  const swatches: Array<{ state: string; color: string }> = [];
  let hasSound = false;
  for (const key of pick) {
    const scene = file.scenes[file.states[key]?.scene];
    const led = scene?.leds.find((track) => track != null);
    swatches.push({
      state: key,
      color: led?.high ?? led?.low ?? "#334155",
    });
    if (scene?.buzzer) {
      hasSound = true;
    }
  }
  while (swatches.length < 3) {
    swatches.push({ state: `pad-${swatches.length}`, color: "#334155" });
  }
  return { swatches, hasSound };
}

const themeModeOptions: Array<{
  value: AppConfig["themeMode"];
  icon: typeof Sun;
  label: string;
  description: string;
}> = [
  {
    value: "light",
    icon: Sun,
    label: "亮色",
    description: "明亮底色，适合白天环境",
  },
  {
    value: "dark",
    icon: Moon,
    label: "暗色",
    description: "OLED 深色，弱光下更护眼",
  },
  {
    value: "system",
    icon: SunMoon,
    label: "跟随系统",
    description: "自动匹配操作系统外观",
  },
];

export function SettingsPage() {
  const { snapshot, config, patchConfig, notify } = useAppState();
  const [saving, setSaving] = useState<string | null>(null);
  const [openingDocs, setOpeningDocs] = useState(false);
  const [preview, setPreview] = useState<{
    swatches: Array<{ state: string; color: string }>;
    hasSound: boolean;
  } | null>(null);
  const [toolchain, setToolchain] = useState<ToolchainStatus | null>(null);
  const [toolchainBusy, setToolchainBusy] = useState(false);
  const activeTheme = config?.activeTheme;

  useEffect(() => {
    if (!activeTheme) {
      return;
    }
    let cancelled = false;
    runAsync(
      api
        .getTheme(activeTheme)
        .then((content) => JSON.parse(content) as ThemeFile)
        .then((file) => {
          if (!cancelled) {
            setPreview(themePreview(file));
          }
        })
        .catch(() => {
          if (!cancelled) {
            setPreview(null);
          }
        })
    );
    return () => {
      cancelled = true;
    };
  }, [activeTheme]);

  // 外部运行环境：惰性探测，不阻塞设置页（设计方案 §6.1）
  useEffect(() => {
    let cancelled = false;
    runAsync(
      api
        .getToolchainStatus(false)
        .then((status) => {
          if (!cancelled) {
            setToolchain(status);
          }
        })
        .catch(() => {
          if (!cancelled) {
            setToolchain(null);
          }
        })
    );
    return () => {
      cancelled = true;
    };
  }, []);

  if (!config) {
    return null;
  }

  const update = async (field: string, patch: Partial<AppConfig>) => {
    setSaving(field);
    try {
      await patchConfig(patch);
      notify({ tone: "success", title: "设置已保存" });
    } catch (error) {
      notify({
        tone: "error",
        title: "保存失败，已恢复原值",
        message: asAppError(error).message,
      });
    } finally {
      setSaving(null);
    }
  };

  const openApiDocs = async () => {
    if (!snapshot) {
      return;
    }
    setOpeningDocs(true);
    try {
      await api.openExternal(`http://127.0.0.1:${snapshot.service.port}/docs/`);
    } catch (error) {
      notify({
        tone: "error",
        title: "无法打开 API 文档",
        message: asAppError(error).message,
      });
    } finally {
      setOpeningDocs(false);
    }
  };

  const refreshToolchain = async (force: boolean) => {
    setToolchainBusy(true);
    try {
      setToolchain(await api.getToolchainStatus(force));
    } catch (error) {
      notify({
        tone: "error",
        title: "无法获取运行环境状态",
        message: asAppError(error).message,
      });
    } finally {
      setToolchainBusy(false);
    }
  };

  const resetToolchain = async () => {
    setToolchainBusy(true);
    try {
      setToolchain(await api.resetToolchainOverrides());
      notify({ tone: "success", title: "已恢复自动检测" });
    } catch (error) {
      notify({
        tone: "error",
        title: "恢复自动检测失败",
        message: asAppError(error).message,
      });
    } finally {
      setToolchainBusy(false);
    }
  };

  return (
    <div className="page-stack page-stack--narrow">
      <PageHeader
        description="管理智能体状态上报、灯效规则与外观偏好"
        title="设置"
      />
      <section aria-labelledby="service-settings">
        <h2 className="section-title" id="service-settings">
          服务
        </h2>
        <Card className="settings-card">
          <SettingRow
            description="工具只能从这台电脑连接；当前无需额外密钥"
            icon={<ShieldCheck />}
            title="连接安全"
          >
            <StatusTag
              tone={snapshot?.service.tokenEnabled ? "warning" : "success"}
            >
              {snapshot?.service.tokenEnabled ? "已启用身份验证" : "仅限本机"}
            </StatusTag>
          </SettingRow>
          <details className="settings-advanced">
            <summary>高级服务信息</summary>
            <SettingRow
              description="在浏览器中查看 Hook API，可调试接口并生成自定义调用"
              icon={<BookOpen />}
              title="接口文档"
            >
              <ActionButton
                busy={openingDocs}
                disabled={!snapshot}
                onClick={() => runAsync(openApiDocs())}
              >
                {openingDocs ? "正在打开…" : "打开 API 文档"}
                {openingDocs ? null : (
                  <ExternalLink aria-hidden="true" size={15} />
                )}
              </ActionButton>
            </SettingRow>
          </details>
        </Card>
      </section>
      <section aria-labelledby="appearance-settings">
        <h2 className="section-title" id="appearance-settings">
          显示
        </h2>
        <Card className="settings-card">
          <SettingRow
            description="界面亮暗主题：亮色 / 暗色 / 跟随系统"
            icon={<SunMoon />}
            stacked
            title="外观模式"
          >
            <fieldset
              aria-label="外观模式"
              className="mode-options mode-options--tri"
            >
              {themeModeOptions.map(
                ({ value, icon: Icon, label, description }) => (
                  <button
                    aria-pressed={config.themeMode === value}
                    className={cn(
                      "mode-option",
                      "mode-option--appearance",
                      config.themeMode === value && "mode-option--active"
                    )}
                    disabled={saving === "themeMode"}
                    key={value}
                    onClick={() =>
                      runAsync(update("themeMode", { themeMode: value }))
                    }
                    type="button"
                  >
                    <span aria-hidden="true" className="mode-option__icon">
                      <Icon size={17} />
                    </span>
                    <span
                      aria-hidden="true"
                      className="mode-option__indicator"
                    />
                    <span className="mode-option__body">
                      <span className="mode-option__name">{label}</span>
                      <span className="mode-option__desc">{description}</span>
                    </span>
                  </button>
                )
              )}
            </fieldset>
          </SettingRow>
          <SettingRow
            description="状态总览中红绿灯的排列方向"
            icon={<Monitor />}
            title="灯组朝向"
          >
            <fieldset aria-label="灯组朝向" className="segmented-control">
              <button
                aria-pressed={config.badgeOrientation === "horizontal"}
                onClick={() =>
                  runAsync(
                    update("orientation", { badgeOrientation: "horizontal" })
                  )
                }
                type="button"
              >
                横排
              </button>
              <button
                aria-pressed={config.badgeOrientation === "vertical"}
                onClick={() =>
                  runAsync(
                    update("orientation", { badgeOrientation: "vertical" })
                  )
                }
                type="button"
              >
                纵向
              </button>
            </fieldset>
          </SettingRow>
          <SettingRow
            description="当前灯效与提示音方案，点击更换"
            icon={<Palette />}
            title="当前主题"
          >
            <Link className="setting-link setting-theme" to="/themes">
              {preview ? (
                <span aria-hidden="true" className="setting-theme__dots">
                  {preview.swatches.map((swatch) => (
                    <span
                      key={swatch.state}
                      style={{ background: swatch.color }}
                    />
                  ))}
                </span>
              ) : null}
              <span className="setting-theme__name">
                {themeDisplayName(snapshot?.activeTheme ?? config.activeTheme)}
              </span>
              {preview?.hasSound ? (
                <span className="setting-theme__sound">
                  <Volume2 size={12} />
                  提示音
                </span>
              ) : null}
              <ChevronRight size={14} />
            </Link>
          </SettingRow>
        </Card>
      </section>
      <section aria-labelledby="system-settings">
        <h2 className="section-title" id="system-settings">
          系统
        </h2>
        <Card className="settings-card">
          <SettingRow
            description="接入工具依赖的 Node.js / npm / Adapter 运行环境"
            icon={<Monitor />}
            title="外部运行环境"
          >
            <StatusTag
              tone={
                toolchain ? toolchainStateCopy(toolchain.state).tone : "neutral"
              }
            >
              {toolchain ? toolchainStateCopy(toolchain.state).label : "检查中"}
            </StatusTag>
          </SettingRow>
          <details className="settings-advanced">
            <summary>运行环境详情与操作</summary>
            {toolchain ? (
              <div className="settings-toolchain">
                <p className="toolchain-summary">{toolchain.summary}</p>
                <ToolchainDetailsList status={toolchain} />
                <p className="toolchain-mode">
                  检测模式：
                  {toolchain.mode === "manual" ? "手动（存在覆盖项）" : "自动"}·
                  检测时间 {toolchain.checkedAt}
                </p>
                <div className="toolchain-recovery">
                  <ActionButton
                    busy={toolchainBusy}
                    onClick={() => runAsync(refreshToolchain(true))}
                  >
                    <RefreshCw aria-hidden="true" size={14} /> 重新检测
                  </ActionButton>
                  {toolchain.mode === "manual" ||
                  toolchain.state === "store_invalid" ? (
                    <ActionButton
                      busy={toolchainBusy}
                      onClick={() => runAsync(resetToolchain())}
                      tone="ghost"
                    >
                      恢复自动检测
                    </ActionButton>
                  ) : null}
                </div>
              </div>
            ) : (
              <p className="toolchain-summary">
                暂无检测结果；打开「接入外部工具」页会自动检测。
              </p>
            )}
          </details>
          <SettingRow
            description="登录系统后自动启动 AI-Light"
            icon={<Monitor />}
            title="开机自启"
          >
            <button
              aria-checked={config.autostart}
              aria-label="开机自启"
              className="switch"
              disabled={saving === "autostart"}
              onClick={() =>
                runAsync(update("autostart", { autostart: !config.autostart }))
              }
              role="switch"
              type="button"
            >
              <span />
            </button>
          </SettingRow>
        </Card>
      </section>
      <p className="settings-note">所有设置即时生效并自动保存</p>
    </div>
  );
}

export const Component = SettingsPage;
