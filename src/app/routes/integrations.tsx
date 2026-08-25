import { CheckCircle2, Info, PlugZap, Unplug } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useAppState } from "@/app/app-context";
import { ActionButton, Card, PageHeader, StatusTag } from "@/components/app-ui";
import { api, asAppError, type IntegrationStatus } from "@/lib/ailight";
import { runAsync } from "@/lib/utils";

type ToolId = "claude-code" | "codex";

interface Integration {
  accent: string;
  description: string;
  id: ToolId;
  name: string;
  source: string;
}

const integrations: Integration[] = [
  {
    accent: "amber",
    description: "同步工作、等待权限、本轮完成、失败和会话结束状态。",
    id: "claude-code",
    name: "Claude Code",
    source: "C",
  },
  {
    accent: "green",
    description: "同步工作、等待权限、本轮完成和会话结束状态。",
    id: "codex",
    name: "Codex",
    source: "X",
  },
];

export function IntegrationsPage() {
  const { notify } = useAppState();
  const [statuses, setStatuses] = useState<
    Partial<Record<ToolId, IntegrationStatus>>
  >({});
  const [busy, setBusy] = useState<ToolId | null>(null);

  const refresh = useCallback(async () => {
    const entries = await Promise.all(
      integrations.map(async ({ id }) => {
        try {
          return [id, await api.getIntegrationStatus(id)] as const;
        } catch {
          return [id, { connected: false, managedCount: 0, path: "" }] as const;
        }
      })
    );
    setStatuses(Object.fromEntries(entries));
  }, []);

  useEffect(() => {
    runAsync(refresh());
  }, [refresh]);

  const connect = async (integration: Integration) => {
    setBusy(integration.id);
    try {
      await api.installIntegration(integration.id);
      await refresh();
      notify({
        tone: "success",
        title: `${integration.name} 已连接`,
        message: "下一次真实任务事件会自动同步到灯牌",
      });
    } catch (error) {
      notify({
        tone: "error",
        title: "连接失败",
        message: asAppError(error).message,
      });
    } finally {
      setBusy(null);
    }
  };

  const disconnect = async (integration: Integration) => {
    setBusy(integration.id);
    try {
      await api.uninstallIntegration(integration.id);
      await refresh();
      notify({ tone: "success", title: `${integration.name} 已断开` });
    } catch (error) {
      notify({
        tone: "error",
        title: "断开失败",
        message: asAppError(error).message,
      });
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="page-stack page-stack--narrow">
      <PageHeader
        description="连接一次，AI 工具的工作状态会自动同步到灯牌"
        title="接入外部工具"
      />
      <div className="integration-list">
        {integrations.map((integration) => {
          const status = statuses[integration.id];
          const connected = status?.connected ?? false;
          return (
            <Card className="integration-card" key={integration.id}>
              <div
                className={`integration-logo integration-logo--${integration.accent}`}
              >
                {integration.source}
              </div>
              <div className="integration-card__main">
                <div className="integration-card__heading">
                  <div>
                    <h2>{integration.name}</h2>
                    {status?.path ? <code>{status.path}</code> : null}
                  </div>
                  <div className="integration-actions">
                    <StatusTag tone={connected ? "success" : "warning"}>
                      {connected ? "已连接" : "未连接"}
                    </StatusTag>
                    {connected ? (
                      <ActionButton
                        busy={busy === integration.id}
                        onClick={() => runAsync(disconnect(integration))}
                      >
                        <Unplug size={16} /> 断开
                      </ActionButton>
                    ) : (
                      <ActionButton
                        busy={busy === integration.id}
                        onClick={() => runAsync(connect(integration))}
                        tone="primary"
                      >
                        <PlugZap size={16} /> 连接
                      </ActionButton>
                    )}
                  </div>
                </div>
                <p>{integration.description}</p>
                {connected ? (
                  <p className="integration-verified">
                    <CheckCircle2 aria-hidden="true" size={16} /> Hook 配置由
                    AI-Light Adapter 管理
                  </p>
                ) : null}
              </div>
            </Card>
          );
        })}
      </div>
      <Card className="explain-card">
        <Info aria-hidden="true" size={19} />
        <div>
          <strong>连接过程不会覆盖你的其他 Hook</strong>
          <p>
            AI-Light 会安装独立
            Adapter、备份现有配置，并且只管理自己添加的条目。
          </p>
        </div>
      </Card>
    </div>
  );
}

export const Component = IntegrationsPage;
