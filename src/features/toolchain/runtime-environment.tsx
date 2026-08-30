import { Check, Copy, RefreshCw } from "lucide-react";
import { useState } from "react";
import {
  ActionButton,
  Card,
  InlineAlert,
  StatusTag,
} from "@/components/app-ui";
import type {
  AdapterStatusEntry,
  ToolchainIssue,
  ToolchainStatus,
  ToolchainToolKind,
  ToolStatusEntry,
} from "@/lib/ailight";
import { cn, runAsync } from "@/lib/utils";

/** 解析状态 → 展示文案与色调（设计方案 §8.1） */
export function toolchainStateCopy(state: ToolchainStatus["state"]): {
  label: string;
  tone: "success" | "warning" | "danger" | "neutral";
} {
  switch (state) {
    case "ready":
      return { label: "可用", tone: "success" };
    case "checking":
      return { label: "检查中", tone: "neutral" };
    case "adapter_missing":
    case "adapter_incompatible":
      return { label: "Adapter 待安装", tone: "warning" };
    case "node_missing":
    case "npm_missing":
    case "node_incompatible":
      return { label: "运行环境未就绪", tone: "danger" };
    case "invalid_override":
      return { label: "手动路径不可用", tone: "danger" };
    case "ambiguous":
      return { label: "存在多组候选", tone: "warning" };
    case "permission_denied":
      return { label: "权限不足", tone: "danger" };
    case "store_invalid":
      return { label: "配置需要恢复", tone: "danger" };
    default:
      return { label: state, tone: "warning" };
  }
}

/** 阻塞恢复的第一个工具（设计方案 §8.2：Node/npm missing 原地给出选择路径） */
export function blockingTool(
  status: ToolchainStatus
): ToolchainToolKind | null {
  for (const kind of ["node", "npm", "adapter"] as const) {
    const entry = status[kind];
    if (entry && entry.state !== "ready" && entry.state !== "checking") {
      return kind;
    }
  }
  return null;
}

const toolLabels: Record<ToolchainToolKind, string> = {
  node: "Node.js",
  npm: "npm",
  adapter: "Adapter",
};

const sourceLabels: Record<string, string> = {
  override: "手动指定",
  previousSelected: "上次检测结果",
  processPath: "环境 PATH",
  osQuery: "系统查询",
  siblingOfNode: "Node 同安装族",
  npmGlobalPrefix: "npm 全局目录",
  windowsRegistry: "Windows 注册表",
  commonDirectory: "常见目录",
  versionManager: "版本管理器",
};

export function sourceLabel(source: string | null): string {
  if (!source) {
    return "—";
  }
  return sourceLabels[source] ?? source;
}

function canResetToolchain(status: ToolchainStatus): boolean {
  return status.mode === "manual" || status.state === "store_invalid";
}

function CopyButton({ value, label }: { value: string; label: string }) {
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(value);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      // 剪贴板不可用时静默：路径文本本身仍可见可手动复制
    }
  };
  return (
    <button
      aria-label={`复制 ${label} 路径`}
      className="toolchain-copy"
      onClick={() => runAsync(copy())}
      title="复制路径"
      type="button"
    >
      {copied ? (
        <Check aria-hidden="true" size={13} />
      ) : (
        <Copy aria-hidden="true" size={13} />
      )}
    </button>
  );
}

function ToolRow({
  entry,
  kind,
}: {
  entry: ToolStatusEntry | AdapterStatusEntry | null;
  kind: ToolchainToolKind;
}) {
  if (!entry) {
    return null;
  }
  return (
    <li className="toolchain-tool">
      <span className="toolchain-tool__name">{toolLabels[kind]}</span>
      <span className="toolchain-tool__meta">
        {entry.version ?? "未检测到"}
        {entry.overridden ? " · 手动指定" : ` · ${sourceLabel(entry.source)}`}
      </span>
      {entry.path ? (
        <span className="toolchain-tool__path">
          <code>{entry.path}</code>
          <CopyButton label={toolLabels[kind]} value={entry.path} />
        </span>
      ) : null}
    </li>
  );
}

/** 每个工具的路径、版本、来源与 override 状态（设计方案 §8.1 / §8.3） */
export function ToolchainDetailsList({ status }: { status: ToolchainStatus }) {
  return (
    <ul aria-label="运行环境详情" className="toolchain-details">
      <ToolRow entry={status.node} kind="node" />
      <ToolRow entry={status.npm} kind="npm" />
      <ToolRow entry={status.adapter} kind="adapter" />
    </ul>
  );
}

interface RecoveryAction {
  kind: ToolchainToolKind;
}

/** 运行环境卡：摘要 + 问题时的恢复卡（设计方案 §8.1 / §8.2） */
export function RuntimeEnvironmentCard({
  checking,
  onRefresh,
  onReset,
  onSelect,
  status,
}: {
  checking: boolean;
  onRefresh: () => void;
  onReset: () => void;
  onSelect: (action: RecoveryAction) => void;
  status: ToolchainStatus | null;
}) {
  const [showDetails, setShowDetails] = useState(false);
  if (!status) {
    return (
      <Card className="toolchain-card" data-testid="runtime-environment">
        <div className="toolchain-card__head">
          <div>
            <h2>运行环境</h2>
            <p className="toolchain-summary">
              正在检查本机 Node.js / npm / Adapter…
            </p>
          </div>
          <StatusTag tone="neutral">检查中</StatusTag>
        </div>
      </Card>
    );
  }
  const { label, tone } = toolchainStateCopy(status.state);
  const blocked = blockingTool(status);
  const storeIssue = status.issues.find(
    (issue) => issue.code === "TOOLCHAIN_STORE"
  );
  const blockingIssue: ToolchainIssue | undefined =
    status.issues.find(
      (issue) =>
        blocked != null &&
        (issue.tool === blocked || issue.code === "TOOLCHAIN_OVERRIDE_INVALID")
    ) ?? storeIssue;
  const resetAvailable = canResetToolchain(status);
  const needsRecovery =
    status.state !== "ready" &&
    status.state !== "adapter_missing" &&
    status.state !== "checking";
  return (
    <Card className="toolchain-card" data-testid="runtime-environment">
      <div className="toolchain-card__head">
        <div>
          <h2>运行环境</h2>
          <p className="toolchain-summary">{status.summary}</p>
        </div>
        <div className="toolchain-card__actions">
          <StatusTag tone={tone}>{checking ? "检查中" : label}</StatusTag>
          <ActionButton busy={checking} onClick={onRefresh} tone="ghost">
            <RefreshCw aria-hidden="true" size={14} /> 重新检测
          </ActionButton>
          <button
            aria-expanded={showDetails}
            className="toolchain-details-toggle"
            onClick={() => setShowDetails((value) => !value)}
            type="button"
          >
            {showDetails ? "收起详情" : "查看详情"}
          </button>
        </div>
      </div>
      {needsRecovery && blockingIssue ? (
        <InlineAlert title={blockingIssue.message} tone="danger">
          <p>
            {blockingIssue.recovery ??
              "AI-Light 已搜索系统 PATH、Node.js 安装信息与常见版本管理器目录。"}
          </p>
          <div className="toolchain-recovery">
            {blocked && status.state !== "store_invalid" ? (
              <ActionButton
                busy={checking}
                onClick={() => onSelect({ kind: blocked })}
                tone="primary"
              >
                选择 {toolLabels[blocked]} 路径
              </ActionButton>
            ) : null}
            {resetAvailable ? (
              <ActionButton busy={checking} onClick={onReset}>
                恢复自动检测
              </ActionButton>
            ) : null}
          </div>
        </InlineAlert>
      ) : null}
      {status.issues.some((issue) => issue.code === "ADAPTER_INCOMPATIBLE") ? (
        <InlineAlert title="Adapter 版本与 AI-Light 不兼容" tone="danger">
          <p>连接时将提示升级到兼容版本。</p>
        </InlineAlert>
      ) : null}
      {showDetails ? (
        <div className={cn("toolchain-detail-panel")}>
          <ToolchainDetailsList status={status} />
          <p className="toolchain-mode">
            检测模式：{status.mode === "manual" ? "手动（存在覆盖项）" : "自动"}
            · 检测时间 {status.checkedAt}
          </p>
          {resetAvailable ? (
            <ActionButton busy={checking} onClick={onReset} tone="ghost">
              恢复自动检测
            </ActionButton>
          ) : null}
        </div>
      ) : null}
    </Card>
  );
}
