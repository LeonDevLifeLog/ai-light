import { type ReactNode, useEffect, useLayoutEffect, useState } from "react";
import { useAppState } from "@/app/app-context";

export type ThemeMode = "light" | "dark" | "system";
export type ResolvedTheme = "light" | "dark";

/** 首帧引导缓存 key（index.html 内联脚本与 Provider 共用；config.json 仍是唯一事实源） */
export const THEME_MODE_KEY = "ailight.themeMode";

const DARK_QUERY = "(prefers-color-scheme: dark)";

function isThemeMode(value: string | null): value is ThemeMode {
  return value === "light" || value === "dark" || value === "system";
}

export function systemTheme(): ResolvedTheme {
  return window.matchMedia(DARK_QUERY).matches ? "dark" : "light";
}

export function resolveTheme(mode: ThemeMode): ResolvedTheme {
  return mode === "system" ? systemTheme() : mode;
}

/** config 加载前的启动值：上次运行的引导缓存，否则按系统外观（默认暗色基线兜底） */
function bootMode(): ThemeMode {
  try {
    const stored = localStorage.getItem(THEME_MODE_KEY);
    if (isThemeMode(stored)) {
      return stored;
    }
  } catch {
    // localStorage 不可用时回退默认
  }
  return "dark";
}

export function ThemeProvider({ children }: { children: ReactNode }) {
  const { config } = useAppState();
  const [mode, setMode] = useState<ThemeMode>(() =>
    config && isThemeMode(config.themeMode) ? config.themeMode : bootMode()
  );

  // config 到达后以 Rust 侧为事实源；config-changed 事件会驱动后续切换
  useEffect(() => {
    if (config && isThemeMode(config.themeMode)) {
      setMode(config.themeMode);
    }
  }, [config]);

  // 首帧无闪烁应用（useLayoutEffect 保证在绘制前执行）
  useLayoutEffect(() => {
    document.documentElement.dataset.theme = resolveTheme(mode);
  }, [mode]);

  // "跟随系统"：实时响应 OS 外观变化
  useEffect(() => {
    if (mode !== "system") {
      return;
    }
    const mql = window.matchMedia(DARK_QUERY);
    const applySystem = () => {
      document.documentElement.dataset.theme = mql.matches ? "dark" : "light";
    };
    applySystem();
    mql.addEventListener("change", applySystem);
    return () => mql.removeEventListener("change", applySystem);
  }, [mode]);

  // 引导缓存：供下次启动首帧读取（config.json 仍是唯一事实源）
  useEffect(() => {
    try {
      localStorage.setItem(THEME_MODE_KEY, mode);
    } catch {
      // 忽略：无持久化能力时仅影响首帧闪烁
    }
  }, [mode]);

  return children;
}
