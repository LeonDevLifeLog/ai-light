import { BellRing, Power, RotateCcw, Send } from "lucide-react";
import { type FormEvent, useMemo, useState } from "react";
import { useAppState } from "@/app/app-context";
import {
  ActionButton,
  Card,
  InlineAlert,
  PageHeader,
  stateCopy,
  TrafficBadge,
  themeDisplayName,
} from "@/components/app-ui";
import { api, asAppError, type BusinessStateName } from "@/lib/ailight";
import { runAsync } from "@/lib/utils";

const states: BusinessStateName[] = [
  "IDLE",
  "WORKING",
  "WAITING",
  "SUCCESS",
  "ERROR",
];
const STATE_NAME_PATTERN = /^[A-Z0-9_-]{1,64}$/;

export function PreviewPage() {
  const { snapshot, notify } = useAppState();
  const [busy, setBusy] = useState<string | null>(null);
  const [customState, setCustomState] = useState("");
  const [recent, setRecent] = useState<string[]>([]);
  const connected = snapshot?.device.connected ?? false;

  const invokeState = async (state: string, preview = false) => {
    setBusy(state);
    try {
      if (preview) {
        await api.previewScene(state, snapshot?.activeTheme);
      } else {
        await api.triggerState(state);
      }
      const stateTitle = stateCopy(state).title;
      let message = `软件状态已切换为“${stateTitle}”，连接灯牌后即可显示效果`;
      if (preview) {
        message = `正在展示“${stateTitle}”效果`;
      } else if (connected) {
        message = `灯牌将展示“${stateTitle}”效果`;
      }
      notify({
        tone: "success",
        title: preview ? "灯效已发送到灯牌" : "状态已切换",
        message,
      });
    } catch (error) {
      notify({
        tone: "error",
        title: "操作失败",
        message: asAppError(error).message,
      });
    } finally {
      setBusy(null);
    }
  };

  const submitCustom = async (event: FormEvent) => {
    event.preventDefault();
    const value = customState.trim().toUpperCase();
    if (!STATE_NAME_PATTERN.test(value)) {
      notify({
        tone: "error",
        title: "状态名格式不正确",
        message: "仅支持字母、数字、下划线和连字符",
      });
      return;
    }
    await invokeState(value);
    setRecent((items) =>
      [value, ...items.filter((item) => item !== value)].slice(0, 5)
    );
    setCustomState("");
  };

  const activeState = useMemo(
    () => snapshot?.business.state ?? "IDLE",
    [snapshot]
  );

  return (
    <div className="page-stack page-stack--narrow">
      <PageHeader
        description={
          <>
            模拟业务状态，或在灯牌上测试{" "}
            <strong className="accent-text">
              {snapshot?.activeTheme
                ? themeDisplayName(snapshot.activeTheme)
                : "当前主题"}
            </strong>
          </>
        }
        title="状态与灯效测试"
      />
      {connected ? null : (
        <InlineAlert title="当前没有连接设备" tone="info">
          你仍可模拟业务状态；连接灯牌后才能看到实际灯光和听到提示音。
        </InlineAlert>
      )}
      <Card>
        <div className="section-kicker">标准状态</div>
        <div className="state-button-grid">
          {states.map((state) => (
            <button
              aria-pressed={activeState === state}
              className="state-button"
              disabled={busy !== null}
              key={state}
              onClick={() => runAsync(invokeState(state))}
              type="button"
            >
              <TrafficBadge compact state={state} />
              <strong>{stateCopy(state).title}</strong>
            </button>
          ))}
        </div>
      </Card>
      <Card>
        <div className="section-kicker">自定义状态</div>
        <form
          className="custom-state-form"
          onSubmit={(event) => runAsync(submitCustom(event))}
        >
          <div className="field field--grow">
            <label htmlFor="custom-state">状态名称</label>
            <div className="custom-state-controls">
              <input
                aria-describedby="custom-state-help"
                id="custom-state"
                maxLength={64}
                onChange={(event) => setCustomState(event.target.value)}
                placeholder="例如 REVIEW（等待审核）"
                value={customState}
              />
              <ActionButton
                disabled={!customState.trim()}
                tone="primary"
                type="submit"
              >
                <Send aria-hidden="true" size={16} /> 触发
              </ActionButton>
            </div>
            <small id="custom-state-help">
              当前主题没有对应效果时，将使用“空闲”效果。
            </small>
          </div>
        </form>
        {recent.length > 0 ? (
          <div className="recent-states">
            <span>最近使用</span>
            {recent.map((state) => (
              <button
                key={state}
                onClick={() => runAsync(invokeState(state))}
                type="button"
              >
                {state}
              </button>
            ))}
          </div>
        ) : null}
      </Card>
      <Card className="preview-actions-card">
        <div>
          <BellRing aria-hidden="true" />
          <span>
            <strong>在灯牌上试听当前效果</strong>
            <small>重新播放灯光和声音，不改变软件状态</small>
          </span>
        </div>
        <ActionButton
          disabled={!connected}
          onClick={() => runAsync(invokeState(activeState, true))}
        >
          <Power aria-hidden="true" size={16} /> 试听当前灯效
        </ActionButton>
      </Card>
      <div className="danger-zone">
        <div>
          <strong>恢复为空闲</strong>
          <span>停止灯效和蜂鸣，并将业务状态恢复为空闲。</span>
        </div>
        <ActionButton
          busy={busy === "RESET"}
          onClick={async () => {
            setBusy("RESET");
            try {
              await api.resetOutputs();
              notify({ tone: "success", title: "输出已重置" });
            } catch (error) {
              notify({
                tone: "error",
                title: "重置失败",
                message: asAppError(error).message,
              });
            } finally {
              setBusy(null);
            }
          }}
          tone="danger"
        >
          <RotateCcw aria-hidden="true" size={16} /> 恢复为空闲
        </ActionButton>
      </div>
    </div>
  );
}

export const Component = PreviewPage;
