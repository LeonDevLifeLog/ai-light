import {
  Battery,
  Bluetooth,
  CheckCircle2,
  Cpu,
  Microchip,
  PlugZap,
  Radio,
  RefreshCw,
  Signal,
  Trash2,
  Unplug,
  WifiOff,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
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
import {
  api,
  asAppError,
  type DeviceState,
  type RememberedDevice,
  type ScannedDevice,
} from "@/lib/ailight";
import { batteryStatus, batteryStatusLabel } from "@/lib/battery-status";
import { cn, runAsync } from "@/lib/utils";

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

function sortNearbyDevices(
  devices: ScannedDevice[],
  rememberedAddress?: string
) {
  return [...devices].sort((left, right) => {
    const rememberedDifference =
      Number(right.address === rememberedAddress) -
      Number(left.address === rememberedAddress);
    if (rememberedDifference !== 0) {
      return rememberedDifference;
    }

    const recognizedDifference =
      Number(right.recognized) - Number(left.recognized);
    if (recognizedDifference !== 0) {
      return recognizedDifference;
    }

    const signalDifference =
      (right.rssi ?? Number.NEGATIVE_INFINITY) -
      (left.rssi ?? Number.NEGATIVE_INFINITY);
    if (signalDifference !== 0) {
      return signalDifference;
    }
    return left.address.localeCompare(right.address);
  });
}

function connectionStatus(
  connected: boolean,
  connecting: boolean,
  reconnecting: boolean
) {
  if (connected) {
    return { label: "连接正常", tone: "success" as const };
  }
  if (connecting) {
    return { label: "正在连接", tone: "warning" as const };
  }
  if (reconnecting) {
    return { label: "正在重连", tone: "warning" as const };
  }
  return { label: "未连接", tone: "neutral" as const };
}

function batteryLabel(device: DeviceState, connected: boolean) {
  if (!connected) {
    return "—";
  }
  return batteryStatusLabel(batteryStatus(device));
}

function ManagedDeviceCard({
  connected,
  connecting,
  device,
  deviceAction,
  managedDevice,
  onConnect,
  onDisconnect,
  onForget,
  reconnecting,
}: {
  connected: boolean;
  connecting: boolean;
  device: DeviceState;
  deviceAction: "disconnect" | "forget" | null;
  managedDevice: RememberedDevice;
  onConnect: () => void;
  onDisconnect: () => void;
  onForget: () => void;
  reconnecting: boolean;
}) {
  const status = connectionStatus(connected, connecting, reconnecting);
  const detail = (value: string | number | null) =>
    connected ? (value ?? "—") : "—";

  return (
    <section aria-labelledby="my-device-title">
      <h2 className="section-title" id="my-device-title">
        我的设备
      </h2>
      <Card className="connected-device">
        <div className="connected-device__identity">
          <div className={cn("device-orb", connected && "is-connected")}>
            {connected ? (
              <Radio aria-hidden="true" />
            ) : (
              <Bluetooth aria-hidden="true" />
            )}
          </div>
          <div className="connected-device__name">
            <strong>{managedDevice.name || "AgentCore-Light"}</strong>
            <span className="mono">{managedDevice.address}</span>
          </div>
          <StatusTag tone={status.tone}>
            {connected ? <CheckCircle2 aria-hidden="true" size={13} /> : null}
            {status.label}
          </StatusTag>
        </div>
        <div className="connected-device__details">
          <dl className="device-stats">
            <div>
              <Battery
                aria-hidden="true"
                className="device-stat__icon"
                size={16}
              />
              <div className="device-stat__copy">
                <dt>电量</dt>
                <dd>{batteryLabel(device, connected)}</dd>
              </div>
            </div>
            <div>
              <Cpu aria-hidden="true" className="device-stat__icon" size={16} />
              <div className="device-stat__copy">
                <dt>固件版本</dt>
                <dd>{detail(device.fwVersion)}</dd>
              </div>
            </div>
            <div>
              <Microchip
                aria-hidden="true"
                className="device-stat__icon"
                size={16}
              />
              <div className="device-stat__copy">
                <dt>硬件型号</dt>
                <dd>{detail(device.hardwareVariant)}</dd>
              </div>
            </div>
          </dl>
          <div className="device-actions">
            {connected || reconnecting ? (
              <ActionButton
                busy={deviceAction === "disconnect"}
                disabled={deviceAction !== null || connecting}
                onClick={onDisconnect}
              >
                <Unplug aria-hidden="true" size={16} />
                {reconnecting ? "停止重连" : "断开连接"}
              </ActionButton>
            ) : (
              <ActionButton
                busy={connecting}
                disabled={deviceAction !== null}
                onClick={onConnect}
                tone="primary"
              >
                <PlugZap aria-hidden="true" size={16} />
                重新连接
              </ActionButton>
            )}
            <ActionButton
              disabled={deviceAction !== null || connecting}
              onClick={onForget}
              tone="ghost"
            >
              <Trash2 aria-hidden="true" size={16} />
              忘记设备
            </ActionButton>
          </div>
        </div>
      </Card>
    </section>
  );
}

function ScanDeviceRow({
  connecting,
  device,
  isConnected,
  isRemembered,
  onConnect,
}: {
  connecting: boolean;
  device: ScannedDevice;
  isConnected: boolean;
  isRemembered: boolean;
  onConnect: () => void;
}) {
  let actionLabel = "连接";
  if (isConnected) {
    actionLabel = "已连接";
  } else if (isRemembered) {
    actionLabel = "重新连接";
  }

  return (
    <Card className="device-row">
      <div className="device-orb">
        <Bluetooth aria-hidden="true" />
      </div>
      <div className="device-row__copy">
        <strong>{device.name || "未命名蓝牙设备"}</strong>
        <span className="mono">{device.address}</span>
      </div>
      <span className="signal-copy">
        <Signal aria-hidden="true" size={15} /> {signalLabel(device.rssi)}
      </span>
      {device.recognized ? (
        <StatusTag tone={isRemembered ? "warning" : "success"}>
          {isRemembered ? "已记住" : "已识别"}
        </StatusTag>
      ) : (
        <StatusTag>其他设备</StatusTag>
      )}
      <ActionButton
        busy={connecting}
        disabled={!device.recognized || isConnected}
        onClick={onConnect}
        tone="primary"
      >
        {actionLabel}
      </ActionButton>
    </Card>
  );
}

export function DevicesPage() {
  const { config, snapshot, fault, notify, refresh } = useAppState();
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

  const connect = async (device: Pick<ScannedDevice, "address" | "name">) => {
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

  const rememberedDevice = config?.rememberedDevice;
  const sortedDevices = useMemo(
    () => sortNearbyDevices(devices, rememberedDevice?.address),
    [devices, rememberedDevice?.address]
  );
  const managedDevice =
    rememberedDevice ??
    (snapshot?.device.address
      ? {
          address: snapshot.device.address,
          name: snapshot.device.name ?? "AgentCore-Light",
        }
      : null);
  const managedDeviceIsConnected = Boolean(
    managedDevice &&
      snapshot?.device.connected &&
      snapshot.device.address === managedDevice.address
  );
  const managedDeviceIsConnecting =
    managedDevice != null && connecting === managedDevice.address;

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
        description="查找并连接附近的状态灯"
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

      {managedDevice && snapshot ? (
        <ManagedDeviceCard
          connected={managedDeviceIsConnected}
          connecting={managedDeviceIsConnecting}
          device={snapshot.device}
          deviceAction={deviceAction}
          managedDevice={managedDevice}
          onConnect={() => runAsync(connect(managedDevice))}
          onDisconnect={() => runAsync(disconnect())}
          onForget={() => setConfirmForget(true)}
          reconnecting={snapshot.device.reconnecting}
        />
      ) : null}

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
              title="未发现附近的状态灯"
            />
          </Card>
        ) : (
          <div className="device-list">
            {sortedDevices.map((device) => (
              <ScanDeviceRow
                connecting={connecting === device.address}
                device={device}
                isConnected={Boolean(
                  snapshot?.device.connected &&
                    snapshot.device.address === device.address
                )}
                isRemembered={rememberedDevice?.address === device.address}
                key={device.address}
                onConnect={() => runAsync(connect(device))}
              />
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
