import { type ReactNode, Suspense } from "react";
import { ErrorBoundary } from "react-error-boundary";
import { AppStateProvider } from "@/app/app-context";
import { TooltipProvider } from "@/components/ui/tooltip";
import AppErrorPage from "@/features/errors/app-error";

export default function AppProvider({ children }: { children: ReactNode }) {
  return (
    <Suspense fallback={<div className="route-loading">正在加载…</div>}>
      <ErrorBoundary FallbackComponent={AppErrorPage}>
        <TooltipProvider>
          <AppStateProvider>{children}</AppStateProvider>
        </TooltipProvider>
      </ErrorBoundary>
    </Suspense>
  );
}
