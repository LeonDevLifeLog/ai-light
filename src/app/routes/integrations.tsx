import {
  CheckCircle2,
  CircleDotDashed,
  Info,
  PlugZap,
  ShieldCheck,
  Unplug,
} from "lucide-react";
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

type ToolId = "claude-code" | "codex" | "qoder" | "trae" | "workbuddy";

interface Integration {
  accent: string;
  description: string;
  id: ToolId;
  name: string;
  nextStep: {
    detail: string;
    manual: boolean;
    steps: string[];
    title: string;
  };
  source: string;
}

const integrations: Integration[] = [
  {
    accent: "amber",
    description: "同步工作、等待权限、本轮完成、失败和会话结束状态。",
    id: "claude-code",
    name: "Claude Code",
    nextStep: {
      detail:
        "通常无需额外设置。企业策略可能会限制用户级 Hook，可在 Claude Code 中输入 /hooks 检查是否已加载。",
      manual: false,
      steps: ["新建一个低风险任务，确认状态灯随任务变化"],
      title: "可以开始验证",
    },
    source: "C",
  },
  {
    accent: "green",
    description: "同步工作、等待权限、本轮完成和会话结束状态。",
    id: "codex",
    name: "Codex",
    nextStep: {
      detail: "Codex 默认禁用尚未信任的新 Hook，需要你在客户端中审核后放行。",
      manual: true,
      steps: [
        "打开 Codex 桌面客户端的「设置」",
        "进入「Hooks」，找到标记为“新钩子”的 AI-Light Hook",
        "确认命令来自 AI-Light 后，点击「信任」",
        "新建一个低风险任务，确认状态灯随任务变化",
      ],
      title: "还差一步：信任 Hook",
    },
    source: "X",
  },
  {
    accent: "blue",
    description: "同步工作、等待输入、本轮完成、失败和会话结束状态。",
    id: "qoder",
    name: "Qoder",
    nextStep: {
      detail: "无需在 Qoder 中开启其他设置或执行额外操作。",
      manual: false,
      steps: ["直接新建一个低风险任务，确认状态灯随任务变化"],
      title: "可以开始验证",
    },
    source: "Q",
  },
  {
    accent: "slate",
    description: "同步工作、等待交互、本轮完成和会话开始状态。",
    id: "trae",
    name: "TraeCode",
    nextStep: {
      detail:
        "TraeCode 只有在全局 Hook 开关开启后，才会执行 AI-Light 写入的配置。",
      manual: true,
      steps: [
        "打开 TraeCode 的「设置」",
        "进入「Hooks」，开启全局 Hook 开关",
        "无需开启“导入 CLAUDE 中的 Hooks 配置”",
        "新建一个低风险任务，确认状态灯随任务变化",
      ],
      title: "还差一步：开启全局 Hook",
    },
    source: "T",
  },
  {
    accent: "violet",
    description: "同步工作、等待提问、本轮完成和会话结束状态。",
    id: "workbuddy",
    name: "WorkBuddy",
    nextStep: {
      detail: "AI-Light 已完成 Hook 配置，无需修改网络地址或端口。",
      manual: false,
      steps: ["新建一个低风险任务，确认状态灯随任务变化"],
      title: "可以开始验证",
    },
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
              {connected ? "配置已写入" : "未配置"}
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
          <div
            className={`integration-next-step ${
              integration.nextStep.manual
                ? "integration-next-step--manual"
                : "integration-next-step--ready"
            }`}
          >
            <div className="integration-next-step__icon">
              {integration.nextStep.manual ? (
                <ShieldCheck aria-hidden="true" size={18} />
              ) : (
                <CheckCircle2 aria-hidden="true" size={18} />
              )}
            </div>
            <div className="integration-next-step__content">
              <strong>{integration.nextStep.title}</strong>
              <p>{integration.nextStep.detail}</p>
              <ol>
                {integration.nextStep.steps.map((step) => (
                  <li key={step}>{step}</li>
                ))}
              </ol>
              <span className="integration-next-step__footnote">
                <CircleDotDashed aria-hidden="true" size={14} />
                “配置已写入”仅表示 AI-Light
                已完成配置；真实任务产生状态变化才表示接入生效。
              </span>
            </div>
          </div>
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
      const message = integration.nextStep.manual
        ? integration.nextStep.title
        : "现在可以运行一个低风险任务进行验证";
      notify({
        tone: integration.nextStep.manual ? "info" : "success",
        title: `${integration.name} 配置已写入`,
        message,
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
      const message = integration.nextStep.manual
        ? integration.nextStep.title
        : "现在可以运行一个低风险任务进行验证";
      notify({
        tone: integration.nextStep.manual ? "info" : "success",
        title: `${integration.name} 配置已写入`,
        message,
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
