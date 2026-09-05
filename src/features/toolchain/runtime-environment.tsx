import {
  ArrowRight,
  Check,
  Copy,
  ExternalLink,
  Package,
  RefreshCw,
} from "lucide-react";
import { useState } from "react";
import {
  ActionButton,
  Card,
  InlineAlert,
  StatusTag,
} from "@/components/app-ui";
import type {
  AdapterStatusEntry,
  ToolchainStatus,
  ToolchainToolKind,
  ToolStatusEntry,
} from "@/lib/ailight";
import { api } from "@/lib/ailight";
import { runAsync } from "@/lib/utils";
import { runtimeFailure } from "./runtime-state";

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
      return { label: "需要安装接入组件", tone: "warning" };
    case "adapter_incompatible":
      return { label: "接入组件需要升级", tone: "warning" };
    case "node_missing":
      return { label: "需要安装 Node.js", tone: "danger" };
    case "node_incompatible":
      return { label: "需要升级 Node.js", tone: "danger" };
    case "npm_missing":
      return { label: "npm 不可用", tone: "danger" };
    case "invalid_override":
      return { label: "指定文件不可用", tone: "danger" };
    case "ambiguous":
      return { label: "需要选择运行环境", tone: "warning" };
    case "permission_denied":
      return { label: "无法运行所选文件", tone: "danger" };
    case "store_invalid":
      return { label: "环境配置需要重建", tone: "danger" };
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

/** 请求错误与后端解析错误均为失败，不能继续显示检查中。 */
export function RuntimeEnvironmentCard({
  checking,
  error,
  onRefresh,
  onReset,
  onSelect,
  status,
}: {
  checking: boolean;
  error: string | null;
  onRefresh: () => void;
  onReset: () => void;
  onSelect: (action: RecoveryAction) => void;
  status: ToolchainStatus | null;
}) {
  const [showDetails, setShowDetails] = useState(false);
  const [copyFeedback, setCopyFeedback] = useState("");
  const failure = runtimeFailure(status, checking, error);
  const state = status?.state;
  let copy = state ? toolchainStateCopy(state) : toolchainStateCopy("checking");
  if (failure) {
    copy = { label: "检测失败", tone: "danger" };
  }
  if (checking) {
    copy = { label: "检查中", tone: "neutral" };
  }
  const diagnostics = JSON.stringify({ error: failure, status }, null, 2);
  const copyDiagnostics = async () => {
    try {
      await navigator.clipboard.writeText(diagnostics);
      setCopyFeedback("诊断信息已复制");
    } catch {
      setCopyFeedback("复制失败，请展开详情，手动选择并复制诊断信息。");
      setShowDetails(true);
    }
  };
  return (
    <Card className="toolchain-card" data-testid="runtime-environment">
      <div className="toolchain-card__head">
        <div>
          <h2>运行环境</h2>
          <p className="toolchain-summary">
            {failure
              ? "无法完成运行环境检查，暂时无法确认是否可以接入。"
              : (status?.summary ?? "正在检查接入工具所需的运行环境…")}
          </p>
        </div>
        <div className="toolchain-card__actions">
          <StatusTag tone={copy.tone}>{copy.label}</StatusTag>
          <ActionButton
            busy={checking}
            onClick={onRefresh}
            tone={failure ? "primary" : "ghost"}
          >
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
      <AdapterIntroduction />
      {failure ? (
        <InlineAlert title="检测或恢复操作失败" tone="danger">
          <p>{failure}</p>
          <p>请重新检测；若仍失败，可复制诊断信息用于排查。</p>
          {status ? (
            <p>详情中保留上次检测结果，当前状态尚未重新确认。</p>
          ) : null}
        </InlineAlert>
      ) : null}
      {!failure && status ? (
        <RuntimeRecovery
          checking={checking}
          onReset={onReset}
          onSelect={onSelect}
          status={status}
        />
      ) : null}
      {failure || showDetails ? (
        <div className="toolchain-card__actions">
          <ActionButton
            onClick={() => runAsync(copyDiagnostics())}
            tone="ghost"
          >
            复制诊断信息
          </ActionButton>
          <span aria-live="polite">{copyFeedback}</span>
        </div>
      ) : null}
      {showDetails ? (
        <div className="toolchain-detail-panel">
          {status ? (
            <RuntimeDetails
              checking={checking}
              onReset={onReset}
              onSelect={onSelect}
              status={status}
            />
          ) : null}
          <details>
            <summary>原始诊断信息</summary>
            <pre style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>
              {diagnostics}
            </pre>
          </details>
        </div>
      ) : null}
    </Card>
  );
}

interface RecoveryProps {
  checking: boolean;
  onReset: () => void;
  onSelect: (action: RecoveryAction) => void;
  status: ToolchainStatus;
}

function InstallationGuide({ state }: { state: ToolchainStatus["state"] }) {
  const [error, setError] = useState<string | null>(null);
  const labels: Partial<Record<ToolchainStatus["state"], string>> = {
    node_missing: "查看 Node.js 安装指引",
    node_incompatible: "查看 Node.js 升级指引",
    npm_missing: "查看 npm 修复步骤",
  };
  if (!labels[state]) {
    return null;
  }
  const openDownload = async () => {
    try {
      await api.openExternal("https://nodejs.org/zh-cn/download");
      setError(null);
    } catch {
      setError(
        "无法打开浏览器，请复制 https://nodejs.org/zh-cn/download 到浏览器访问。"
      );
    }
  };
  return (
    <details>
      <summary>{labels[state]}</summary>
      <ol>
        <li>
          从 Node.js 官方下载页面获取当前系统的安装包，选择 20
          或更高版本，并安装配套 npm。
          <ActionButton onClick={() => runAsync(openDownload())} tone="ghost">
            打开官方下载页面
          </ActionButton>
        </li>
        <li>
          如果使用版本管理器，请通过原管理器安装或切换 Node.js，确保配套 npm
          可用。
        </li>
        <li>安装或修复完成后，回到这里点击“重新检测”。</li>
      </ol>
      {error ? <p role="alert">{error}</p> : null}
    </details>
  );
}

function RuntimeRecovery({
  status,
  checking,
  onReset,
  onSelect,
}: RecoveryProps) {
  const state = status.state;
  const blocked = blockingTool(status);
  const storeInvalid = state === "store_invalid";
  const issue = recoveryIssue(status, blocked);
  const selectable = blocked !== null && blocked !== "adapter" && !storeInvalid;
  const resetAvailable = canResetToolchain(status);
  const resetLabel = storeInvalid ? "重建配置并检测" : "改用自动检测";
  const copy = toolchainStateCopy(state);
  if (["ready", "checking"].includes(state)) {
    return null;
  }
  if (["adapter_missing", "adapter_incompatible"].includes(state)) {
    return (
      <InlineAlert title={copy.label} tone="info">
        <p>
          请点击下方任一工具卡的“连接”，再确认
          {state === "adapter_missing" ? "安装" : "升级"}接入组件。AI-Light
          将使用兼容版本，完成后连接该工具。
        </p>
      </InlineAlert>
    );
  }
  return (
    <InlineAlert title={copy.label} tone="danger">
      <p>
        {storeInvalid
          ? "保存的环境配置无法读取，需要重建配置后重新检测。"
          : (issue?.message ?? "运行环境尚未通过检查，请查看详情并重新检测。")}
      </p>
      <p>完成修复后才能连接外部工具。</p>
      <InstallationGuide state={state} />
      {state === "permission_denied" ? (
        <p>
          请检查文件是否允许当前用户执行；若由版本管理器安装，优先通过原管理器修复，或选择其他可运行文件。
        </p>
      ) : null}
      {state === "ambiguous" ? (
        <p>请在下方选择要使用的运行环境文件，候选信息可在详情中查看。</p>
      ) : null}
      <div className="toolchain-recovery">
        {selectable && blocked ? (
          <ActionButton
            busy={checking}
            onClick={() => onSelect({ kind: blocked })}
          >
            {state === "invalid_override" ? "重新选择" : "已安装，选择"}{" "}
            {toolLabels[blocked]} 文件
          </ActionButton>
        ) : null}
        {resetAvailable ? (
          <ActionButton busy={checking} onClick={onReset}>
            {resetLabel}
          </ActionButton>
        ) : null}
      </div>
      {selectable ? (
        <p>
          请选择文件而非文件夹：Node.js 选择 node（Windows 为 node.exe）；npm
          选择 npm、npm.cmd 或 npm-cli.js。文件通常位于 Node.js
          安装目录或版本管理器目录。
        </p>
      ) : null}
      {resetAvailable ? (
        <p>
          {storeInvalid
            ? "重建前保留原配置；点击后将清除手动路径并重新发现运行环境。"
            : "改用自动检测会清除全部手动指定路径；重新检测则保留这些路径。"}
        </p>
      ) : null}
    </InlineAlert>
  );
}

function RuntimeDetails({
  status,
  checking,
  onReset,
  onSelect,
}: RecoveryProps) {
  const storeInvalid = status.state === "store_invalid";
  const resetAvailable = canResetToolchain(status);
  const resetLabel = storeInvalid ? "重建配置并检测" : "改用自动检测";
  return (
    <>
      <p>{status.summary}</p>
      <ToolchainDetailsList status={status} />
      <p className="toolchain-mode">
        检测模式：
        {status.mode === "manual" ? "手动（存在覆盖项）" : "自动"} · 检测时间{" "}
        {status.checkedAt}
      </p>
      <ul>
        {status.issues.map((item) => (
          <li key={`${item.code}-${item.message}`}>
            <code>{item.code}</code>：{item.message}
          </li>
        ))}
      </ul>
      {storeInvalid ? null : (
        <>
          <p>高级操作：手动指定文件后立即验证。</p>
          <div className="toolchain-recovery">
            {(["node", "npm", "adapter"] as const).map((kind) => (
              <ActionButton
                busy={checking}
                key={kind}
                onClick={() => onSelect({ kind })}
                tone="ghost"
              >
                选择 {toolLabels[kind]} 文件
              </ActionButton>
            ))}
          </div>
        </>
      )}
      {resetAvailable ? (
        <>
          <p>此操作清除全部手动路径并重新检测。</p>
          <ActionButton busy={checking} onClick={onReset} tone="ghost">
            {resetLabel}
          </ActionButton>
        </>
      ) : null}
    </>
  );
}

function recoveryIssue(
  status: ToolchainStatus,
  blocked: ToolchainToolKind | null
) {
  if (status.state === "store_invalid") {
    return status.issues.find((item) => item.code === "TOOLCHAIN_STORE");
  }
  return (
    status.issues.find((item) => item.code === "TOOLCHAIN_OVERRIDE_INVALID") ??
    status.issues.find((item) => item.tool === blocked)
  );
}

function AdapterIntroduction() {
  const [linkError, setLinkError] = useState<string | null>(null);
  const packageUrl = "https://www.npmjs.com/package/@ai-light/adapter";
  const openPackage = async () => {
    try {
      await api.openExternal(packageUrl);
      setLinkError(null);
    } catch {
      setLinkError(`无法打开浏览器，请复制此地址访问：${packageUrl}`);
    }
  };
  return (
    <section aria-label="接入原理" className="adapter-intro">
      <div className="adapter-intro__heading">
        <h3>
          <Package aria-hidden="true" size={16} />
          接入是如何工作的？
        </h3>
        <a
          aria-label="在浏览器查看 @ai-light/adapter npm 程序包"
          className="adapter-intro__package"
          href={packageUrl}
          onClick={(event) => {
            event.preventDefault();
            runAsync(openPackage());
          }}
          rel="noreferrer"
          target="_blank"
        >
          <code>@ai-light/adapter</code>
          <ExternalLink aria-hidden="true" size={13} />
        </a>
      </div>
      <p>
        AI-Light 通过这个 npm 程序包配置客户端的
        Hook（事件回调），将任务状态上报给本机 AI-Light，再由 AI-Light
        驱动状态灯。
      </p>
      <ol aria-label="任务状态传递流程" className="adapter-intro__flow">
        {["AI 客户端", "接入组件", "本机 AI-Light", "状态灯"].map(
          (step, index) => (
            <li key={step}>
              {index > 0 ? <ArrowRight aria-hidden="true" size={13} /> : null}
              <span>{step}</span>
            </li>
          )
        )}
      </ol>
      <p className="adapter-intro__requirement">
        运行需要 <strong>Node.js 20+ 和 npm</strong>
        ；首次连接且组件未就绪时，会提示你确认安装或升级兼容版本。
      </p>
      {linkError ? (
        <p className="adapter-intro__error" role="alert">
          {linkError}
        </p>
      ) : null}
    </section>
  );
}
