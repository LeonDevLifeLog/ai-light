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
    description:
      "配置 hooks 与 notify；Codex Desktop 可能重写 notify，请合并已有配置。",
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
    description: "事件语义与 Claude Code 同构，可复用本地 HTTP 接入方式。",
    source: "Q",
    accent: "slate",
    status: "reserved",
    config: (port) => `POST http://127.0.0.1:${port}/hook
Content-Type: application/json

{"source":"qoder","event":"state_change","state":"WORKING"}`,
  },
  {
    name: "Cursor",
    path: "桥接进程（后续版本）",
    description: "当前缺少原生 Hook，计划通过本机桥接进程接入。",
    source: "Cu",
    accent: "violet",
    status: "reserved",
    config: (port) => `curl -X POST http://127.0.0.1:${port}/hook \\
  -H 'Content-Type: application/json' \\
  -d '{"source":"my-tool","event":"state_change","state":"SUCCESS"}'`,
  },
];
const statusLabels: Record<Integration["status"], string> = {
  incompatible: "Desktop 不兼容",
  reserved: "预留",
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
    notify({ tone: "success", title: "配置已复制", message: integration.name });
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
          <span>本地 Hook 地址</span>
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
                  <ActionButton
                    busy={testing === integration.name}
                    disabled={integration.status !== "unconfigured"}
                    onClick={() => runAsync(testConnection(integration))}
                  >
                    测试连接
                  </ActionButton>
                  <ActionButton
                    disabled={integration.status === "reserved"}
                    onClick={() => runAsync(copy(integration))}
                  >
                    {copied === integration.name ? (
                      <Check size={16} />
                    ) : (
                      <Clipboard size={16} />
                    )}
                    {copied === integration.name ? "已复制" : "复制"}
                  </ActionButton>
                </div>
              </div>
              <p>{integration.description}</p>
              <details>
                <summary>查看配置示例</summary>
                <pre>
                  <code>{integration.config(port)}</code>
                </pre>
              </details>
            </div>
          </Card>
        ))}
      </div>
      <Card className="explain-card">
        <Info aria-hidden="true" size={19} />
        <div>
          <strong>这些配置在做什么？</strong>
          <p>
            工具在开始、完成、出错或等待你回复时调用本机端点。AI-Light
            仲裁状态，再由当前主题编译并下发灯效。
          </p>
        </div>
        <FlaskConical aria-hidden="true" className="explain-card__decoration" />
      </Card>
    </div>
  );
}

export const Component = IntegrationsPage;
