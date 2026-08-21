import { ArrowRight, Palette, Radio, Timer } from "lucide-react";
import { Link } from "react-router";
import { useAppState } from "@/app/app-context";
import {
  Card,
  DeviceSummary,
  Skeleton,
  StatusTag,
  stateCopy,
  TrafficBadge,
} from "@/components/app-ui";

export function DashboardPage() {
  const { snapshot, config, loading } = useAppState();
  const business = snapshot?.business;
  const copy = stateCopy(business?.state ?? "IDLE");
  const since = business?.sinceTs
    ? new Date(business.sinceTs).toLocaleTimeString([], {
        hour: "2-digit",
        minute: "2-digit",
      })
    : "—";

  if (loading || !snapshot) {
    return (
      <div className="page-stack">
        <Skeleton className="skeleton--hero" />
        <div className="dashboard-grid">
          <Skeleton className="skeleton--card" />
          <Skeleton className="skeleton--card" />
        </div>
      </div>
    );
  }

  return (
    <div className="page-stack">
      <Card className="status-hero">
        <div className="status-hero__eyebrow">当前状态</div>
        <StatusTag tone="success">
          <span className="live-pip" /> 实时
        </StatusTag>
        <TrafficBadge
          orientation={config?.badgeOrientation ?? "horizontal"}
          state={business?.state ?? "IDLE"}
        />
        <h1>{copy.title}</h1>
        <p>{copy.subtitle}</p>
        <div className="status-meta">
          <span>
            <Radio aria-hidden="true" size={14} /> 来源：
            {business?.source ?? "暂无"}
          </span>
          <span>
            <Timer aria-hidden="true" size={14} /> 自 {since}
          </span>
        </div>
      </Card>

      <div className="dashboard-grid">
        <Link
          aria-label="查看设备详情"
          className="card card-link"
          to="/devices"
        >
          <div className="section-kicker">设备</div>
          <DeviceSummary
            battery={snapshot.device.batteryPercent}
            connected={snapshot.device.connected}
            name={snapshot.device.name}
          />
        </Link>
        <Link
          aria-label="查看和切换主题"
          className="card card-link"
          to="/themes"
        >
          <div className="section-kicker">主题</div>
          <div className="theme-summary">
            <div className="theme-swatch">
              <Palette aria-hidden="true" size={22} />
            </div>
            <div>
              <strong>{snapshot.activeTheme}</strong>
              <span>当前灯效主题</span>
            </div>
            <ArrowRight aria-hidden="true" className="muted-icon" size={18} />
          </div>
        </Link>
      </div>
    </div>
  );
}

export const Component = DashboardPage;
