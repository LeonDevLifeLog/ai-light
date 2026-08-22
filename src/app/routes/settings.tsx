import {
  BookOpen,
  ChevronRight,
  ExternalLink,
  Monitor,
  Moon,
  Palette,
  Radio,
  Server,
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
import type { AppConfig, ThemeFile } from "@/lib/ailight";
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
  const { snapshot, config, patchConfig, notify, refresh } = useAppState();
  const [saving, setSaving] = useState<string | null>(null);
  const [openingDocs, setOpeningDocs] = useState(false);
  const [portInput, setPortInput] = useState("");
  const [preview, setPreview] = useState<{
    swatches: Array<{ state: string; color: string }>;
    hasSound: boolean;
  } | null>(null);
  const activeTheme = config?.activeTheme;

  useEffect(() => {
    if (config) {
      setPortInput(String(config.portPreference));
    }
  }, [config]);

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

  const updatePort = async () => {
    const port = Number(portInput);
    if (!Number.isInteger(port) || port < 1024 || port > 65_535) {
      notify({
        tone: "error",
        title: "端口格式不正确",
        message: "请输入 1024 到 65535 之间的整数",
      });
      return;
    }
    setSaving("portPreference");
    try {
      await patchConfig({ portPreference: port });
      await refresh();
      notify({
        tone: "success",
        title: "Hook 服务已切换端口",
        message: `当前监听 127.0.0.1:${port}`,
      });
    } catch (error) {
      setPortInput(String(config.portPreference));
      notify({
        tone: "error",
        title: "端口切换失败",
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
            description="多个工具同时运行时，决定优先显示哪个状态"
            icon={<Radio />}
            stacked
            title="多个工具同时运行时"
          >
            <fieldset aria-label="状态显示规则" className="mode-options">
              <button
                aria-pressed={config.arbitrationMode === "priority"}
                className={cn(
                  "mode-option",
                  config.arbitrationMode === "priority" && "mode-option--active"
                )}
                disabled={saving === "arbitrationMode"}
                onClick={() =>
                  runAsync(
                    update("arbitrationMode", { arbitrationMode: "priority" })
                  )
                }
                type="button"
              >
                <span aria-hidden="true" className="mode-option__indicator" />
                <span className="mode-option__body">
                  <span className="mode-option__title">
                    <span className="mode-option__name">重要状态优先</span>
                    <span className="mode-option__tag">推荐</span>
                  </span>
                  <span className="mode-option__desc">
                    重要状态优先：错误 &gt; 完成 &gt; 进行中 &gt; 等待 &gt; 空闲
                  </span>
                </span>
              </button>
              <button
                aria-pressed={config.arbitrationMode === "last_active"}
                className={cn(
                  "mode-option",
                  config.arbitrationMode === "last_active" &&
                    "mode-option--active"
                )}
                disabled={saving === "arbitrationMode"}
                onClick={() =>
                  runAsync(
                    update("arbitrationMode", {
                      arbitrationMode: "last_active",
                    })
                  )
                }
                type="button"
              >
                <span aria-hidden="true" className="mode-option__indicator" />
                <span className="mode-option__body">
                  <span className="mode-option__title">
                    <span className="mode-option__name">最近活动优先</span>
                  </span>
                  <span className="mode-option__desc">
                    最后上报状态的工具接管灯效
                  </span>
                </span>
              </button>
            </fieldset>
          </SettingRow>
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
              description={`当前监听端口：${snapshot?.service.port ?? config.portPreference}`}
              icon={<Server />}
              title="服务端口"
            >
              <div className="port-control">
                <input
                  aria-label="Hook 服务端口"
                  disabled={saving === "portPreference"}
                  max={65_535}
                  min={1024}
                  onChange={(event) => setPortInput(event.target.value)}
                  type="number"
                  value={portInput}
                />
                <button
                  className="action-button action-button--secondary"
                  disabled={
                    saving === "portPreference" ||
                    portInput === String(config.portPreference)
                  }
                  onClick={() => runAsync(updatePort())}
                  type="button"
                >
                  {saving === "portPreference" ? "正在切换…" : "保存并重启服务"}
                </button>
              </div>
            </SettingRow>
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
