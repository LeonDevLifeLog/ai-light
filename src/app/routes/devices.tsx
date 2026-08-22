import { Bluetooth, Radio, RefreshCw, Signal, WifiOff } from "lucide-react";
import { type ReactNode, useCallback, useEffect, useState } from "react";
import { useAppState } from "@/app/app-context";
import {
  ActionButton,
  Card,
  Dialog,
  EmptyState,
  InlineAlert,
  PageHeader,
  StatusTag,
} from "@/components/app-ui";
import { api, asAppError, type ScannedDevice } from "@/lib/ailight";
import { runAsync } from "@/lib/utils";

function signalLabel(rssi: number | null) {
  if (rssi == null) {
    return "信号未知";
  }
  if (rssi >= -60) {
    return "信号很强";
  }
  if (rssi >= -75) {
    return "信号良好";
  }
  return "信号较弱";
}

export function DevicesPage() {
  const { snapshot, fault, notify, refresh } = useAppState();
  const [devices, setDevices] = useState<ScannedDevice[]>([]);
  const [scanning, setScanning] = useState(false);
  const [scanError, setScanError] = useState<string | null>(null);
  const [connecting, setConnecting] = useState<string | null>(null);
  const [hasScanned, setHasScanned] = useState(false);
  const [deviceAction, setDeviceAction] = useState<
    "disconnect" | "forget" | null
  >(null);
  const [confirmForget, setConfirmForget] = useState(false);

  const scan = useCallback(async () => {
    setScanning(true);
    setScanError(null);
    try {
      const [found] = await Promise.all([
        api.scanDevices(),
        new Promise((resolve) => window.setTimeout(resolve, 400)),
      ]);
      setDevices(found);
    } catch (error) {
      setScanError(asAppError(error).message);
    } finally {
      setScanning(false);
      setHasScanned(true);
    }
  }, []);

  useEffect(() => {
    runAsync(scan());
  }, [scan]);

  const connect = async (device: ScannedDevice) => {
    setConnecting(device.address);
    try {
      await api.connectDevice(device.address);
      await refresh();
      notify({
        tone: "success",
        title: "连接请求已发送",
        message: device.name,
      });
    } catch (error) {
      notify({
        tone: "error",
        title: "连接失败",
        message: asAppError(error).message,
      });
    } finally {
      setConnecting(null);
    }
  };

  const disconnect = async () => {
    setDeviceAction("disconnect");
    try {
      await api.disconnectDevice();
      await refresh();
      notify({ tone: "success", title: "设备已断开" });
    } catch (error) {
      notify({
        tone: "error",
        title: "断开失败",
        message: asAppError(error).message,
      });
    } finally {
      setDeviceAction(null);
    }
  };

  const forget = async () => {
    setDeviceAction("forget");
    try {
      await api.forgetDevice();
      await refresh();
      setConfirmForget(false);
      notify({ tone: "success", title: "已忘记设备" });
    } catch (error) {
      notify({
        tone: "error",
        title: "无法忘记设备",
        message: asAppError(error).message,
      });
    } finally {
      setDeviceAction(null);
    }
  };

  let connectionSection: ReactNode = null;
  if (snapshot?.device.connected) {
    connectionSection = (
      <section aria-labelledby="connected-title">
        <h2 className="section-title" id="connected-title">
          已连接
        </h2>
        <Card className="connected-device">
          <div className="device-orb is-connected">
            <Radio aria-hidden="true" />
          </div>
          <div className="connected-device__name">
            <strong>{snapshot.device.name ?? "AgentCore-Light"}</strong>
            <span className="mono">{snapshot.device.address}</span>
          </div>
          <dl className="device-stats">
            <div>
              <dt>电量</dt>
              <dd>{snapshot.device.batteryPercent ?? "—"}%</dd>
            </div>
            <div>
              <dt>固件</dt>
              <dd>{snapshot.device.fwVersion ?? "—"}</dd>
            </div>
            <div>
              <dt>硬件</dt>
              <dd>{snapshot.device.hardwareVariant ?? "—"}</dd>
            </div>
          </dl>
          <StatusTag tone="success">已连接</StatusTag>
          <div className="device-actions">
            <ActionButton
              busy={deviceAction === "disconnect"}
              disabled={deviceAction !== null}
              onClick={() => runAsync(disconnect())}
            >
              断开连接
            </ActionButton>
            <ActionButton
              disabled={deviceAction !== null}
              onClick={() => setConfirmForget(true)}
              tone="danger"
            >
              忘记设备
            </ActionButton>
          </div>
        </Card>
      </section>
    );
  } else if (snapshot?.device.reconnecting) {
    connectionSection = (
      <section aria-labelledby="reconnecting-title">
        <h2 className="section-title" id="reconnecting-title">
          设备状态
        </h2>
        <Card className="scan-status" role="status">
          <span className="scan-pip" />
          <span>设备已断开，正在自动重连…（最多尝试 5 次）</span>
          <div className="device-actions">
            <ActionButton
              busy={deviceAction === "disconnect"}
              disabled={deviceAction !== null}
              onClick={() => runAsync(disconnect())}
            >
              停止重连
            </ActionButton>
            <ActionButton
              disabled={deviceAction !== null}
              onClick={() => setConfirmForget(true)}
              tone="danger"
            >
              忘记设备
            </ActionButton>
          </div>
        </Card>
      </section>
    );
  }

  return (
    <div className="page-stack">
      <PageHeader
        actions={
          <ActionButton
            busy={scanning}
            className="scan-action-button"
            onClick={() => runAsync(scan())}
            tone="primary"
          >
            {scanning ? null : <RefreshCw aria-hidden="true" size={16} />}
            <span className="scan-action-button__labels">
              <span aria-hidden={scanning}>重新查找设备</span>
              <span aria-hidden={!scanning}>正在查找…</span>
            </span>
          </ActionButton>
        }
        description="连接你附近的 AgentCore-Light 灯牌"
        title="设备"
      />

      {scanning ? (
        <Card className="scan-status" role="status">
          <span className="scan-pip" />
          <span>正在查找附近的灯牌，扫描大约需要 5 秒…</span>
          <div
            aria-label="扫描进行中"
            aria-valuemax={100}
            aria-valuemin={0}
            className="progress"
            role="progressbar"
          >
            <span />
          </div>
        </Card>
      ) : null}
      {scanError ? (
        <InlineAlert title="无法扫描蓝牙设备">
          {scanError}。请检查系统蓝牙权限后重试。
        </InlineAlert>
      ) : null}
      {fault ? (
        <InlineAlert title="设备故障">
          来源 {fault.source}，故障码 {fault.code}，上下文 {fault.context}。
        </InlineAlert>
      ) : null}

      {connectionSection}

      <section aria-labelledby="nearby-title">
        <h2 className="section-title" id="nearby-title">
          附近设备
        </h2>
        {!scanning && devices.length === 0 && hasScanned ? (
          <Card>
            <EmptyState
              action={
                <ActionButton onClick={() => runAsync(scan())}>
                  <RefreshCw size={16} /> 重新查找设备
                </ActionButton>
              }
              description="请确认灯牌已上电、处于广播范围内，并允许 AI-Light 使用蓝牙。"
              icon={<WifiOff />}
              title="未发现 AgentCore-Light 设备"
            />
          </Card>
        ) : (
          <div className="device-list">
            {devices.map((device) => (
              <Card
                className="device-row"
                key={`${device.address}-${device.name}-${device.rssi}`}
              >
                <div className="device-orb">
                  <Bluetooth aria-hidden="true" />
                </div>
                <div className="device-row__copy">
                  <strong>{device.name || "未命名蓝牙设备"}</strong>
                  <span className="mono">{device.address}</span>
                </div>
                <span className="signal-copy">
                  <Signal aria-hidden="true" size={15} />{" "}
                  {signalLabel(device.rssi)}
                </span>
                {device.recognized ? (
                  <StatusTag tone="success">已识别</StatusTag>
                ) : (
                  <StatusTag>其他设备</StatusTag>
                )}
                <ActionButton
                  busy={connecting === device.address}
                  disabled={
                    !device.recognized ||
                    snapshot?.device.address === device.address
                  }
                  onClick={() => runAsync(connect(device))}
                  tone="primary"
                >
                  {snapshot?.device.address === device.address
                    ? "已连接"
                    : "连接"}
                </ActionButton>
              </Card>
            ))}
          </div>
        )}
      </section>
      {confirmForget ? (
        <Dialog
          description="应用会先断开当前连接、停止自动重连，并清除已记住的设备。"
          footer={
            <>
              <ActionButton
                disabled={deviceAction !== null}
                onClick={() => setConfirmForget(false)}
              >
                取消
              </ActionButton>
              <ActionButton
                busy={deviceAction === "forget"}
                disabled={deviceAction !== null}
                onClick={() => runAsync(forget())}
                tone="danger"
              >
                确认忘记
              </ActionButton>
            </>
          }
          onClose={() => setConfirmForget(false)}
          open={confirmForget}
          title="忘记这台设备？"
        >
          <p>忘记后，下次启动 AI-Light 不会再自动连接这台设备。</p>
        </Dialog>
      ) : null}
    </div>
  );
}

export const Component = DevicesPage;
