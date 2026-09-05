import assert from "node:assert/strict";
import test from "node:test";
import { runtimeFailure } from "../src/features/toolchain/runtime-state.ts";
import type { ToolchainStatus } from "../src/lib/ailight.ts";

const status: ToolchainStatus = {
  state: "ready",
  mode: "auto",
  summary: "可用",
  node: null,
  npm: null,
  adapter: null,
  checkedAt: "",
  issues: [],
};
test("initial loading differs from a finished request without a result", () => {
  assert.equal(runtimeFailure(null, true, null), null);
  assert.ok(runtimeFailure(null, false, null));
});
test("request failure overrides a retained ready snapshot without changing it", () => {
  assert.equal(runtimeFailure(status, false, "请求失败"), "请求失败");
  assert.equal(status.state, "ready");
  assert.equal(runtimeFailure(status, false, null), null);
});
test("backend emergency checking state is a failure, not endless loading", () => {
  const emergency: ToolchainStatus = {
    ...status,
    state: "checking",
    issues: [
      {
        code: "INTERNAL",
        message: "解析任务失败",
        tool: null,
        recovery: "请重试",
      },
    ],
  };
  assert.equal(runtimeFailure(emergency, false, null), "解析任务失败");
  assert.ok(runtimeFailure({ ...status, state: "checking" }, false, null));
});
test("action errors stay visible even when revalidation finds a ready environment", () => {
  assert.equal(
    runtimeFailure(status, false, "所选路径不可用"),
    "所选路径不可用"
  );
  assert.equal(
    runtimeFailure({ ...status, state: "node_missing" }, false, null),
    null
  );
});
