import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
} from "react";
import {
  type AppConfig,
  type AppError,
  type AppSnapshot,
  api,
  asAppError,
  subscribe,
} from "@/lib/ailight";
import { runAsync } from "@/lib/utils";

interface ToastItem {
  id: number;
  message?: string;
  title: string;
  tone: "success" | "error" | "info";
}

interface AppContextValue {
  config: AppConfig | null;
  dismissToast: (id: number) => void;
  fatalError: AppError | null;
  fault: { source: number; code: number; context: number } | null;
  loading: boolean;
  notify: (toast: Omit<ToastItem, "id">) => void;
  patchConfig: (patch: Partial<AppConfig>) => Promise<AppConfig>;
  refresh: () => Promise<void>;
  snapshot: AppSnapshot | null;
  toasts: ToastItem[];
}

const AppContext = createContext<AppContextValue | null>(null);

export function AppStateProvider({ children }: { children: ReactNode }) {
  const [snapshot, setSnapshot] = useState<AppSnapshot | null>(null);
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [loading, setLoading] = useState(true);
  const [fatalError, setFatalError] = useState<AppError | null>(null);
  const [fault, setFault] = useState<AppContextValue["fault"]>(null);
  const [toasts, setToasts] = useState<ToastItem[]>([]);

  const dismissToast = useCallback((id: number) => {
    setToasts((items) => items.filter((item) => item.id !== id));
  }, []);

  const notify = useCallback(
    (toast: Omit<ToastItem, "id">) => {
      const id = Date.now() + Math.random();
      setToasts((items) => [...items.slice(-3), { ...toast, id }]);
      window.setTimeout(() => dismissToast(id), 4500);
    },
    [dismissToast]
  );

  const refresh = useCallback(async () => {
    try {
      const [nextSnapshot, nextConfig] = await Promise.all([
        api.getAppState(),
        api.getConfig(),
      ]);
      setSnapshot(nextSnapshot);
      setConfig(nextConfig);
      setFatalError(null);
    } catch (error) {
      setFatalError(asAppError(error));
    } finally {
      setLoading(false);
    }
  }, []);

  const patchConfig = useCallback(async (patch: Partial<AppConfig>) => {
    const next = await api.updateConfig(patch);
    setConfig(next);
    return next;
  }, []);

  useEffect(() => {
    runAsync(refresh());
    const unlisteners: Array<() => void> = [];
    const register = async () => {
      unlisteners.push(
        await subscribe<AppSnapshot["business"]>(
          "business-state-changed",
          (business) => {
            setSnapshot((current) =>
              current
                ? { ...current, business: { ...current.business, ...business } }
                : current
            );
          }
        ),
        await subscribe<Partial<AppSnapshot["device"]>>(
          "device-connection-changed",
          (device) => {
            setSnapshot((current) =>
              current
                ? { ...current, device: { ...current.device, ...device } }
                : current
            );
            notify({
              tone: device.connected ? "success" : "info",
              title: device.connected ? "设备已连接" : "设备已断开",
            });
          }
        ),
        await subscribe<Partial<AppSnapshot["device"]>>(
          "device-power-changed",
          (device) => {
            setSnapshot((current) =>
              current
                ? { ...current, device: { ...current.device, ...device } }
                : current
            );
          }
        ),
        await subscribe<AppContextValue["fault"]>(
          "device-fault",
          (nextFault) => {
            setFault(nextFault);
            notify({
              tone: "error",
              title: "设备报告故障",
              message: `故障码 ${nextFault?.code}`,
            });
          }
        ),
        await subscribe<{ name: string }>("theme-changed", ({ name }) => {
          setSnapshot((current) =>
            current
              ? {
                  ...current,
                  activeTheme: name,
                  business: { ...current.business, theme: name },
                }
              : current
          );
        })
      );
    };
    runAsync(register());
    return () => {
      for (const unlisten of unlisteners) {
        unlisten();
      }
    };
  }, [notify, refresh]);

  const value = useMemo(
    () => ({
      snapshot,
      config,
      loading,
      fatalError,
      fault,
      toasts,
      refresh,
      patchConfig,
      notify,
      dismissToast,
    }),
    [
      snapshot,
      config,
      loading,
      fatalError,
      fault,
      toasts,
      refresh,
      patchConfig,
      notify,
      dismissToast,
    ]
  );

  return <AppContext.Provider value={value}>{children}</AppContext.Provider>;
}

export function useAppState() {
  const value = useContext(AppContext);
  if (!value) {
    throw new Error("useAppState must be used within AppStateProvider");
  }
  return value;
}
