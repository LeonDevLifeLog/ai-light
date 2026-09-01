import {
  AlertCircle,
  Battery,
  CheckCircle2,
  ChevronRight,
  CircleOff,
  LoaderCircle,
  Radio,
  WifiOff,
  X,
} from "lucide-react";
import {
  type ButtonHTMLAttributes,
  type HTMLAttributes,
  type ReactNode,
  useEffect,
  useRef,
} from "react";
import type { BusinessStateName, DeviceState } from "@/lib/ailight";
import { batteryStatus, batteryStatusLabel } from "@/lib/battery-status";
import { cn } from "@/lib/utils";

export function Card({ className, ...props }: HTMLAttributes<HTMLDivElement>) {
  return <div className={cn("card", className)} {...props} />;
}

interface PageHeaderProps {
  actions?: ReactNode;
  description?: ReactNode;
  title: string;
}

export function PageHeader({ title, description, actions }: PageHeaderProps) {
  return (
    <header className="page-header">
      <div>
        <h1>{title}</h1>
        {description ? <p>{description}</p> : null}
      </div>
      {actions ? <div className="page-actions">{actions}</div> : null}
    </header>
  );
}

interface ActionButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  busy?: boolean;
  tone?: "primary" | "secondary" | "ghost" | "danger";
}

export function ActionButton({
  tone = "secondary",
  busy = false,
  className,
  children,
  disabled,
  ...props
}: ActionButtonProps) {
  return (
    <button
      className={cn("action-button", `action-button--${tone}`, className)}
      disabled={disabled || busy}
      type="button"
      {...props}
    >
      {busy ? (
        <LoaderCircle aria-hidden="true" className="spin" size={16} />
      ) : null}
      {children}
    </button>
  );
}

export function StatusTag({
  tone = "neutral",
  children,
}: {
  tone?: "success" | "warning" | "danger" | "neutral";
  children: ReactNode;
}) {
  return (
    <span className={cn("status-tag", `status-tag--${tone}`)}>{children}</span>
  );
}

const labels: Record<string, { title: string; subtitle: string }> = {
  IDLE: { title: "空闲", subtitle: "一切就绪，等待任务" },
  WORKING: { title: "工作中", subtitle: "AI 正在处理任务" },
  WAITING: { title: "等待中", subtitle: "需要你的输入或授权" },
  SUCCESS: { title: "已完成", subtitle: "任务已顺利完成" },
  ERROR: { title: "出错了", subtitle: "任务遇到问题，请检查详情" },
};

export function stateCopy(state: BusinessStateName) {
  return labels[state] ?? { title: state, subtitle: "自定义业务状态" };
}

const themeLabels: Record<string, string> = {
  default: "默认主题",
  minimal: "极简",
  neon: "霓虹",
  nature: "自然",
  aurora: "极光",
  focus: "专注",
};

export function themeDisplayName(theme: string) {
  return themeLabels[theme] ?? theme;
}

export function TrafficBadge({
  state,
  orientation = "horizontal",
  compact = false,
}: {
  state: BusinessStateName;
  orientation?: "horizontal" | "vertical";
  compact?: boolean;
}) {
  const normalized = state in labels ? state : "IDLE";
  return (
    <div
      aria-label={`当前灯组状态：${stateCopy(state).title}`}
      className={cn(
        "traffic-badge",
        `traffic-badge--${orientation}`,
        compact && "is-compact"
      )}
      role="img"
    >
      <span
        className={cn(
          "light-dot light-dot--red",
          normalized === "ERROR" && "is-on is-blink"
        )}
      />
      <span
        className={cn(
          "light-dot light-dot--yellow",
          normalized === "WAITING" && "is-on"
        )}
      />
      <span
        className={cn(
          "light-dot light-dot--green",
          (normalized === "WORKING" || normalized === "SUCCESS") && "is-on",
          normalized === "WORKING" && "is-breathing"
        )}
      />
    </div>
  );
}

export function EmptyState({
  icon = <CircleOff />,
  title,
  description,
  action,
}: {
  icon?: ReactNode;
  title: string;
  description: string;
  action?: ReactNode;
}) {
  return (
    <div className="empty-state">
      <div className="empty-state__icon">{icon}</div>
      <h2>{title}</h2>
      <p>{description}</p>
      {action}
    </div>
  );
}

export function DeviceSummary({
  connected,
  name,
  device,
  reconnecting,
}: {
  connected: boolean;
  name?: string | null;
  device: DeviceState;
  reconnecting?: boolean;
}) {
  let title = "尚未连接设备";
  let subtitle = "连接灯牌后即可同步状态灯效";
  if (connected) {
    title = name || "AgentCore-Light";
    subtitle = "蓝牙连接正常";
  } else if (reconnecting) {
    title = "正在重连…";
    subtitle = "设备断开，自动重连中";
  }
  return (
    <div className="device-summary">
      <div className={cn("device-orb", connected && "is-connected")}>
        {connected ? (
          <Radio aria-hidden="true" />
        ) : (
          <WifiOff aria-hidden="true" />
        )}
      </div>
      <div className="device-summary__copy">
        <strong>{title}</strong>
        <span>{subtitle}</span>
      </div>
      {connected ? (
        <span className="battery-value">
          <Battery aria-hidden="true" size={16} />
          {batteryStatusLabel(batteryStatus(device))}
        </span>
      ) : null}
      <ChevronRight aria-hidden="true" className="muted-icon" size={18} />
    </div>
  );
}

export function InlineAlert({
  tone = "danger",
  title,
  children,
}: {
  tone?: "danger" | "info" | "success";
  title: string;
  children?: ReactNode;
}) {
  const Icon = tone === "success" ? CheckCircle2 : AlertCircle;
  return (
    <div
      className={cn("inline-alert", `inline-alert--${tone}`)}
      role={tone === "danger" ? "alert" : "status"}
    >
      <Icon aria-hidden="true" size={18} />
      <div>
        <strong>{title}</strong>
        {children ? <p>{children}</p> : null}
      </div>
    </div>
  );
}

export function Dialog({
  open,
  title,
  description,
  children,
  footer,
  onClose,
  size = "medium",
}: {
  open: boolean;
  title: string;
  description?: string;
  children: ReactNode;
  footer?: ReactNode;
  onClose: () => void;
  size?: "medium" | "large";
}) {
  const dialogRef = useRef<HTMLElement>(null);

  useEffect(() => {
    if (!open) {
      return;
    }
    const previouslyFocused = document.activeElement as HTMLElement | null;
    const dialog = dialogRef.current;
    const focusable = dialog?.querySelectorAll<HTMLElement>(
      'button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [href], [tabindex]:not([tabindex="-1"])'
    );
    focusable?.[0]?.focus();
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        onClose();
        return;
      }
      if (event.key !== "Tab" || !focusable || focusable.length === 0) {
        return;
      }
      const first = focusable[0];
      const last = focusable.item(focusable.length - 1);
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
      previouslyFocused?.focus();
    };
  }, [open, onClose]);

  if (!open) {
    return null;
  }
  return (
    <div className="dialog-scrim">
      <section
        aria-describedby={description ? "dialog-description" : undefined}
        aria-modal="true"
        className={cn("dialog", `dialog--${size}`)}
        ref={dialogRef}
        role="dialog"
      >
        <header className="dialog__header">
          <div>
            <h2>{title}</h2>
            {description ? <p id="dialog-description">{description}</p> : null}
          </div>
          <button
            aria-label="关闭对话框"
            className="icon-button"
            onClick={onClose}
            type="button"
          >
            <X aria-hidden="true" size={18} />
          </button>
        </header>
        <div className="dialog__body">{children}</div>
        {footer ? <footer className="dialog__footer">{footer}</footer> : null}
      </section>
    </div>
  );
}

export function Skeleton({ className }: { className?: string }) {
  return <div aria-hidden="true" className={cn("skeleton", className)} />;
}
