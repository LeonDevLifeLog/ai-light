import { Monitor, Palette, Radio, Server, ShieldCheck } from "lucide-react";
import { useState } from "react";
import { Link } from "react-router";
import { useAppState } from "@/app/app-context";
import { Card, PageHeader, StatusTag } from "@/components/app-ui";
import { type AppConfig, asAppError } from "@/lib/ailight";
import { runAsync } from "@/lib/utils";

function SettingRow({
  icon,
  title,
  description,
  children,
}: {
  icon: React.ReactNode;
  title: string;
  description: string;
  children: React.ReactNode;
}) {
  return (
    <div className="setting-row">
      <div className="setting-row__icon">{icon}</div>
      <div className="setting-row__copy">
        <strong>{title}</strong>
        <span>{description}</span>
      </div>
      <div className="setting-row__control">{children}</div>
    </div>
  );
}

export function SettingsPage() {
  const { snapshot, config, patchConfig, notify } = useAppState();
  const [saving, setSaving] = useState<string | null>(null);

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

  return (
    <div className="page-stack page-stack--narrow">
      <PageHeader description="管理服务、状态仲裁与界面偏好" title="设置" />
      <section aria-labelledby="service-settings">
        <h2 className="section-title" id="service-settings">
          服务
        </h2>
        <Card className="settings-card">
          <SettingRow
            description="本地 Hook 服务当前监听端口"
            icon={<Server />}
            title="服务端口"
          >
            <code className="setting-value">
              {snapshot?.service.port ?? config.portPreference}
            </code>
          </SettingRow>
          <SettingRow
            description="决定多个来源同时活跃时由谁控制灯效"
            icon={<Radio />}
            title="仲裁模式"
          >
            <select
              aria-label="仲裁模式"
              disabled={saving === "arbitrationMode"}
              onChange={(event) =>
                runAsync(
                  update("arbitrationMode", {
                    arbitrationMode: event.target
                      .value as AppConfig["arbitrationMode"],
                  })
                )
              }
              value={config.arbitrationMode}
            >
              <option value="priority">优先级抢占</option>
              <option value="last_active">最近活跃</option>
            </select>
          </SettingRow>
          <SettingRow
            description="第一版界面不开放密码修改；服务始终只监听本机回环地址"
            icon={<ShieldCheck />}
            title="接入保护"
          >
            <StatusTag
              tone={snapshot?.service.tokenEnabled ? "warning" : "success"}
            >
              {snapshot?.service.tokenEnabled ? "Token 已启用" : "仅限本机"}
            </StatusTag>
          </SettingRow>
        </Card>
      </section>
      <section aria-labelledby="appearance-settings">
        <h2 className="section-title" id="appearance-settings">
          显示
        </h2>
        <Card className="settings-card">
          <SettingRow
            description="状态总览中红、黄、绿灯的排列方向"
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
            description="当前生效的灯效与蜂鸣方案"
            icon={<Palette />}
            title="当前主题"
          >
            <Link className="setting-link" to="/themes">
              {snapshot?.activeTheme ?? config.activeTheme}
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
            description="登录系统后自动启动并驻留托盘"
            icon={<Monitor />}
            title="开机自启"
          >
            <button
              aria-checked={config.autostart}
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
      <p className="settings-note">
        设置会立即写入应用配置。接入密码当前以明文存储在本机配置目录中。
      </p>
    </div>
  );
}

export const Component = SettingsPage;
