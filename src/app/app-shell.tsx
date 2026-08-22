import {
  BellRing,
  Bluetooth,
  Cable,
  LayoutDashboard,
  Palette,
  RefreshCw,
  Settings,
} from "lucide-react";
import { useEffect } from "react";
import { NavLink, Outlet, useNavigate } from "react-router";
import { useAppState } from "@/app/app-context";
import { ActionButton, InlineAlert, Skeleton } from "@/components/app-ui";
import { subscribe } from "@/lib/ailight";
import { cn, runAsync } from "@/lib/utils";

const navigation = [
  { to: "/", label: "状态", icon: LayoutDashboard, end: true },
  { to: "/devices", label: "设备", icon: Bluetooth },
  { to: "/integrations", label: "接入", icon: Cable },
  { to: "/themes", label: "主题", icon: Palette },
  { to: "/preview", label: "试听", icon: BellRing },
  { to: "/settings", label: "设置", icon: Settings },
];

export function AppShell() {
  const { snapshot, loading, fatalError, refresh, toasts, dismissToast } =
    useAppState();
  const navigate = useNavigate();

  // 托盘「打开配置」→ 跳转 /devices（Rust 侧 emit open-config）
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    subscribe("open-config", () => navigate("/devices")).then((fn) => {
      unlisten = fn;
    });
    return () => {
      unlisten?.();
    };
  }, [navigate]);

  return (
    <div className="app-frame">
      <a className="skip-link" href="#main-content">
        跳到主要内容
      </a>
      <aside className="sidebar">
        <div className="brand">
          <span aria-hidden="true" className="brand__mark">
            A
          </span>
          <span>AI-Light</span>
        </div>
        <nav aria-label="主导航" className="sidebar__nav">
          {navigation.map(({ to, label, icon: Icon, end }) => (
            <NavLink
              className={({ isActive }) =>
                cn("nav-link", isActive && "is-active")
              }
              end={end}
              key={to}
              to={to}
            >
              <Icon aria-hidden="true" size={18} />
              <span>{label}</span>
            </NavLink>
          ))}
        </nav>
        <div className="sidebar__footer">
          {loading ? (
            <Skeleton className="skeleton--line" />
          ) : (
            <div className="connection-label">
              <span
                className={cn(
                  "connection-dot",
                  snapshot?.device.connected && "is-connected"
                )}
              />
              {snapshot?.device.connected ? "设备已连接" : "设备未连接"}
            </div>
          )}
          <details className="sidebar__advanced">
            <summary>高级信息</summary>
            <span className="sidebar__meta">
              版本 {snapshot?.service.version ?? "—"} · 服务端口{" "}
              {snapshot?.service.port ?? "—"}
            </span>
          </details>
        </div>
      </aside>
      <main className="main-content" id="main-content" tabIndex={-1}>
        {fatalError ? (
          <div className="fatal-state">
            <InlineAlert title="无法加载应用状态">
              {fatalError.message}
            </InlineAlert>
            <ActionButton onClick={() => runAsync(refresh())}>
              <RefreshCw aria-hidden="true" size={16} /> 重试
            </ActionButton>
          </div>
        ) : (
          <Outlet />
        )}
      </main>
      <div
        aria-live="polite"
        aria-relevant="additions"
        className="toast-region"
      >
        {toasts.map((toast) => (
          <button
            className={cn("toast", `toast--${toast.tone}`)}
            key={toast.id}
            onClick={() => dismissToast(toast.id)}
            type="button"
          >
            <strong>{toast.title}</strong>
            {toast.message ? <span>{toast.message}</span> : null}
          </button>
        ))}
      </div>
    </div>
  );
}
