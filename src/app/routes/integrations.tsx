import { CheckCircle2, Info, PlugZap, Unplug } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useAppState } from "@/app/app-context";
import {
  ActionButton,
  Card,
  InlineAlert,
  PageHeader,
  StatusTag,
} from "@/components/app-ui";
import {
  RuntimeEnvironmentCard,
  toolchainStateCopy,
} from "@/features/toolchain/runtime-environment";
import { runtimeFailure } from "@/features/toolchain/runtime-state";
import {
  api,
  asAppError,
  type IntegrationStatus,
  type ToolchainStatus,
  type ToolchainToolKind,
} from "@/lib/ailight";
import { runAsync } from "@/lib/utils";

type ToolId = "claude-code" | "codex" | "qoder" | "workbuddy";

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
  {
    accent: "blue",
    description: "同步工作、等待输入、本轮完成、失败和会话结束状态。",
    id: "qoder",
    name: "Qoder",
    source: "Q",
  },
  {
    accent: "violet",
    description: "同步工作、等待提问、本轮完成和会话结束状态。",
    id: "workbuddy",
    name: "WorkBuddy",
    source: "W",
  },
];

function connectButtonLabel(
  busy: boolean,
  confirmPending: boolean,
  upgradePending: boolean
): string {
  if (busy) {
    return "正在检查运行环境";
  }
  if (!confirmPending) {
    return "连接";
  }
  return upgradePending ? "确认并升级" : "确认并安装";
}

function IntegrationRow({
  busy,
  confirmPending,
  disabled,
  integration,
  onConnect,
  onDisconnect,
  status,
  upgradePending,
}: {
  busy: boolean;
  confirmPending: boolean;
  disabled: boolean;
  integration: Integration;
  onConnect: (integration: Integration) => void;
  onDisconnect: (integration: Integration) => void;
  status?: IntegrationStatus;
  upgradePending: boolean;
}) {
  const connected = status?.connected ?? false;
  return (
    <Card className="integration-card">
      <div
        className={`integration-logo integration-logo--${integration.accent}`}
      >
        {integration.source}
      </div>
      <div className="integration-card__main">
        <div className="integration-card__heading">
          <div>
            <h2>{integration.name}</h2>
            {(status?.paths ?? (status?.path ? [status.path] : [])).map(
              (path) => (
                <code key={path}>{path}</code>
              )
            )}
          </div>
          <div className="integration-actions">
            <StatusTag tone={connected ? "success" : "warning"}>
              {connected ? "已连接" : "未连接"}
            </StatusTag>
            {connected ? (
              <ActionButton
                busy={busy}
                onClick={() => onDisconnect(integration)}
              >
                <Unplug size={16} /> 断开
              </ActionButton>
            ) : (
              <ActionButton
                busy={busy}
                disabled={disabled}
                onClick={() => onConnect(integration)}
                tone="primary"
              >
                <PlugZap size={16} />
                {connectButtonLabel(busy, confirmPending, upgradePending)}
              </ActionButton>
            )}
          </div>
        </div>
        <p>{integration.description}</p>
        {confirmPending ? (
          <InlineAlert
            title={`将通过 npm ${upgradePending ? "升级" : "安装"} @ai-light/adapter`}
            tone="info"
          >
            <p>
              即将使用 {status?.toolchainSummary ?? "已检测的 Node.js 与 npm"}{" "}
              全局{upgradePending ? "升级" : "安装"} AI-Light Adapter，随后注入
              Hook。安装过程不提升权限、不修改系统 PATH。
            </p>
          </InlineAlert>
        ) : null}
        {connected ? (
          <p className="integration-verified">
            <CheckCircle2 aria-hidden="true" size={16} /> Hook 配置由 AI-Light
            Adapter 管理
          </p>
        ) : null}
      </div>
    </Card>
  );
}

export function IntegrationsPage() {
  const { notify } = useAppState();
  const [statuses, setStatuses] = useState<
    Partial<Record<ToolId, IntegrationStatus>>
  >({});
  const [busy, setBusy] = useState<ToolId | null>(null);
  const [toolchain, setToolchain] = useState<ToolchainStatus | null>(null);
  const [toolchainError, setToolchainError] = useState<string | null>(null);
  const [toolchainChecking, setToolchainChecking] = useState(true);
  // Adapter 缺失时：连接按钮变为「确认安装」前的内联确认态（设计方案 §8.2.4）
  const [confirmingTool, setConfirmingTool] = useState<ToolId | null>(null);

  const refreshToolchain = useCallback(
    async (force: boolean) => {
      setToolchainChecking(true);
      try {
        const status = await api.getToolchainStatus(force);
        setToolchain(status);
        setToolchainError(null);
      } catch (error) {
        // 刷新失败不得吞掉错误伪装成"未连接"（设计方案 §12.2）
        setToolchainError(asAppError(error).message);
        notify({
          tone: "error",
          title: "无法获取运行环境状态",
          message: asAppError(error).message,
        });
      } finally {
        setToolchainChecking(false);
      }
    },
    [notify]
  );

  const refresh = useCallback(async () => {
    const entries = await Promise.all(
      integrations.map(async ({ id }) => {
        try {
          return [id, await api.getIntegrationStatus(id)] as const;
        } catch (error) {
          const appError = asAppError(error);
          const status: IntegrationStatus = {
            connected: false,
            managedCount: 0,
            path: "",
          };
          if (appError.code === "ADAPTER_NOT_FOUND") {
            status.reason = "adapter_missing";
            status.toolchainSummary = appError.message;
          }
          return [id, status] as const;
        }
      })
    );
    setStatuses(Object.fromEntries(entries));
  }, []);

  useEffect(() => {
    runAsync(refreshToolchain(false));
    runAsync(refresh());
  }, [refresh, refreshToolchain]);

  const connect = async (integration: Integration) => {
    setBusy(integration.id);
    setConfirmingTool(null);
    try {
      // 连接前先检查运行环境（设计方案 §8.2.2）
      const status = await api.getToolchainStatus(true);
      setToolchain(status);
      setToolchainError(null);
      if (
        status.state === "adapter_missing" ||
        status.state === "adapter_incompatible"
      ) {
        // Adapter 缺失或不兼容：确认后由 install_integration 安装明确兼容版本。
        setConfirmingTool(integration.id);
        return;
      }
      if (status.state !== "ready") {
        // Node/npm 未就绪：停止连接，恢复卡已内联展示（§8.2.5）
        notify({
          tone: "error",
          title: "运行环境未就绪",
          message: toolchainStateCopy(status.state).label,
        });
        return;
      }
      await api.installIntegration(integration.id);
      await Promise.all([refresh(), refreshToolchain(false)]);
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
      runAsync(refreshToolchain(true));
    } finally {
      setBusy(null);
    }
  };

  const confirmInstall = async (integration: Integration) => {
    setBusy(integration.id);
    try {
      await api.installIntegration(integration.id);
      await Promise.all([refresh(), refreshToolchain(true)]);
      notify({
        tone: "success",
        title: `${integration.name} 已连接`,
        message: "Adapter 已安装并注入 Hook",
      });
    } catch (error) {
      notify({
        tone: "error",
        title: "安装失败",
        message: asAppError(error).message,
      });
      runAsync(refreshToolchain(true));
    } finally {
      setBusy(null);
      setConfirmingTool(null);
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

  const selectExecutable = async (kind: ToolchainToolKind) => {
    setToolchainChecking(true);
    try {
      // 后端打开原生文件选择器并立即验证（设计方案 §7 / §8.2.7）
      const status = await api.selectExecutable(kind);
      if (JSON.stringify(status) !== JSON.stringify(toolchain)) {
        setToolchain(status);
        setToolchainError(null);
      }
      // 取消选择返回当前状态：不发送可能误报的成功 Toast，结果由卡片持续展示。
    } catch (error) {
      setToolchainError(`所选路径不可用：${asAppError(error).message}`);
      notify({
        tone: "error",
        title: "所选路径不可用",
        message: asAppError(error).message,
      });
      try {
        setToolchain(await api.getToolchainStatus(true));
      } catch {
        // 保持原状态
      }
    } finally {
      setToolchainChecking(false);
    }
  };

  const resetOverrides = async () => {
    setToolchainChecking(true);
    try {
      setToolchain(await api.resetToolchainOverrides());
      setToolchainError(null);
    } catch (error) {
      setToolchainError(`改用自动检测失败：${asAppError(error).message}`);
      notify({
        tone: "error",
        title: "改用自动检测失败",
        message: asAppError(error).message,
      });
    } finally {
      setToolchainChecking(false);
    }
  };

  let environmentAnnouncement = "正在检查运行环境";
  if (toolchain) {
    environmentAnnouncement = `运行环境${toolchainStateCopy(toolchain.state).label}：${toolchain.summary}`;
  }
  const failure = runtimeFailure(toolchain, toolchainChecking, toolchainError);
  if (failure) {
    environmentAnnouncement = `运行环境操作失败：${failure}`;
  }
  if (toolchainChecking) {
    environmentAnnouncement = "正在检查运行环境";
  }
  const environmentReady = toolchain?.state === "ready";

  return (
    <div className="page-stack page-stack--narrow">
      <PageHeader
        description="连接一次，AI 工具的工作状态会自动同步到灯牌"
        title="接入外部工具"
      />
      <div aria-live="polite" className="visually-hidden">
        {environmentAnnouncement}
      </div>
      <RuntimeEnvironmentCard
        checking={toolchainChecking}
        error={toolchainError}
        onRefresh={() => runAsync(refreshToolchain(true))}
        onReset={() => runAsync(resetOverrides())}
        onSelect={({ kind }) => runAsync(selectExecutable(kind))}
        status={toolchain}
      />
      <div className="integration-list">
        {integrations.map((integration) => (
          <IntegrationRow
            busy={busy === integration.id || toolchainChecking}
            confirmPending={confirmingTool === integration.id}
            disabled={toolchainChecking && !environmentReady}
            integration={integration}
            key={integration.id}
            onConnect={(item) =>
              runAsync(
                confirmingTool === integration.id
                  ? confirmInstall(item)
                  : connect(item)
              )
            }
            onDisconnect={(item) => runAsync(disconnect(item))}
            status={statuses[integration.id]}
            upgradePending={
              confirmingTool === integration.id &&
              toolchain?.state === "adapter_incompatible"
            }
          />
        ))}
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
