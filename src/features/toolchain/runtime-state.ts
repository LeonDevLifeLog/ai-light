import type { ToolchainStatus } from "../../lib/ailight.ts";

export function runtimeFailure(
  status: ToolchainStatus | null,
  checking: boolean,
  error: string | null
): string | null {
  if (error) {
    return error;
  }
  const internal = status?.issues.find((issue) => issue.code === "INTERNAL");
  if (internal) {
    return internal.message;
  }
  if (!checking && (!status || status.state === "checking")) {
    return "检测未能完成，请重新检测。";
  }
  return null;
}
