import {
  Check,
  Clipboard,
  FlaskConical,
  Info,
  TerminalSquare,
} from "lucide-react";
import { useState } from "react";
import { useAppState } from "@/app/app-context";
import { ActionButton, Card, PageHeader, StatusTag } from "@/components/app-ui";
import { api, asAppError } from "@/lib/ailight";
import { runAsync } from "@/lib/utils";

interface Integration {
  accent: string;
  config: (port: number) => string;
  description: string;
  name: string;
  path: string;
  source: string;
  status: "unconfigured" | "incompatible" | "reserved";
}

const integrations: Integration[] = [
  {
    name: "Claude Code",
    path: "~/.claude/settings.json",
    description: "通过 HTTP hook 上报开始、完成、错误和等待状态。",
    source: "C",
    accent: "amber",
    status: "unconfigured",
    config: (port) => `{
  "hooks": {
    "UserPromptSubmit": [{ "hooks": [{ "type": "http", "url": "http://127.0.0.1:${port}/hook" }] }],
    "Stop": [{ "hooks": [{ "type": "http", "url": "http://127.0.0.1:${port}/hook" }] }]
  }
}`,
  },
  {
    name: "Codex",
    path: "~/.codex/hooks.json + config.toml",
    description: "Codex CLI 可以通过通知脚本同步状态；Codex Desktop 暂不支持。",
    source: "X",
    accent: "green",
    status: "incompatible",
    config: (port) => `curl -s -X POST http://127.0.0.1:${port}/hook \\
  -H 'Content-Type: application/json' \\
  -d '{"source":"codex","state":"WORKING"}'`,
  },
  {
    name: "Qoder",
    path: "工具 Hook 设置",
    description: "此工具的自动接入正在准备中。",
    source: "Q",
    accent: "slate",
    status: "reserved",
    config: (port) => `POST http://127.0.0.1:${port}/hook
Content-Type: application/json

{"source":"qoder","event":"state_change","state":"WORKING"}`,
  },
  {
    name: "Cursor",
    path: "暂不支持自动接入",
    description: "Cursor 当前无法自动同步状态。",
    source: "Cu",
    accent: "violet",
    status: "reserved",
    config: (port) => `curl -X POST http://127.0.0.1:${port}/hook \\
  -H 'Content-Type: application/json' \\
  -d '{"source":"my-tool","event":"state_change","state":"SUCCESS"}'`,
  },
];
const statusLabels: Record<Integration["status"], string> = {
  incompatible: "仅支持 CLI",
  reserved: "暂不支持",
  unconfigured: "未配置",
};

export function IntegrationsPage() {
  const { snapshot, notify } = useAppState();
  const [copied, setCopied] = useState<string | null>(null);
  const [testing, setTesting] = useState<string | null>(null);
  const port = snapshot?.service.port ?? 47_800;

  const copy = async (integration: Integration) => {
    await navigator.clipboard.writeText(integration.config(port));
    setCopied(integration.name);
    notify({
      tone: "success",
      title: `${integration.name} 配置代码已复制`,
      message: `请粘贴到 ${integration.path}`,
    });
    window.setTimeout(() => setCopied(null), 2000);
  };

  const testConnection = async (integration: Integration) => {
    setTesting(integration.name);
    try {
      await api.triggerState("WORKING");
      notify({
        tone: "success",
        title: "测试状态已触发",
        message: "Dashboard 应立即显示工作中",
      });
    } catch (error) {
      notify({
        tone: "error",
        title: "测试连接失败",
        message: asAppError(error).message,
      });
    } finally {
      setTesting(null);
    }
  };

  return (
    <div className="page-stack page-stack--narrow">
      <PageHeader
        description="把 AI 编程工具的状态自动同步到灯牌，配置一次，长期生效"
        title="接入外部工具"
      />
      <Card className="endpoint-banner">
        <TerminalSquare aria-hidden="true" size={20} />
        <div>
          <span>本机接收地址</span>
          <code>http://127.0.0.1:{port}/hook</code>
        </div>
        <StatusTag
          tone={snapshot?.service.tokenEnabled ? "warning" : "success"}
        >
          {snapshot?.service.tokenEnabled ? "需要 Token" : "仅本机访问"}
        </StatusTag>
      </Card>
      <div className="integration-list">
        {integrations.map((integration) => (
          <Card className="integration-card" key={integration.name}>
            <div
              className={`integration-logo integration-logo--${integration.accent}`}
            >
              {integration.source}
            </div>
            <div className="integration-card__main">
              <div className="integration-card__heading">
                <div>
                  <h2>{integration.name}</h2>
                  <code>{integration.path}</code>
                </div>
                <div className="integration-actions">
                  <StatusTag
                    tone={
                      integration.status === "unconfigured"
                        ? "warning"
                        : "neutral"
                    }
                  >
                    {statusLabels[integration.status]}
                  </StatusTag>
                  {integration.status === "reserved" ? null : (
                    <>
                      <ActionButton
                        busy={testing === integration.name}
                        disabled={integration.status !== "unconfigured"}
                        onClick={() => runAsync(testConnection(integration))}
                      >
                        测试连接
                      </ActionButton>
                      <ActionButton onClick={() => runAsync(copy(integration))}>
                        {copied === integration.name ? (
                          <Check size={16} />
                        ) : (
                          <Clipboard size={16} />
                        )}
                        {copied === integration.name ? "已复制" : "复制配置"}
                      </ActionButton>
                    </>
                  )}
                </div>
              </div>
              <p>{integration.description}</p>
              {integration.status === "reserved" ? null : (
                <details>
                  <summary>查看配置步骤</summary>
                  <pre>
                    <code>{integration.config(port)}</code>
                  </pre>
                </details>
              )}
            </div>
          </Card>
        ))}
      </div>
      <Card className="explain-card">
        <Info aria-hidden="true" size={19} />
        <div>
          <strong>这些配置在做什么？</strong>
          <p>
            工具会在开始、完成、出错或等待你回复时通知
            AI-Light，应用再按当前主题让灯牌显示对应效果。
          </p>
        </div>
        <FlaskConical aria-hidden="true" className="explain-card__decoration" />
      </Card>
    </div>
  );
}

export const Component = IntegrationsPage;
