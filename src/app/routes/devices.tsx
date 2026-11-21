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
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
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
  type UsbStatus,
  usbFaultMessage,
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

function deviceKey(device: RememberedDevice) {
  return "usbUid" in device ? `usb:${device.usbUid}` : `ble:${device.address}`;
}

function snapshotDevice(device?: DeviceState): RememberedDevice | null {
  const name = device?.name ?? "AgentCore-Light";
  if (device?.usbUid) {
    return { name, usbUid: device.usbUid };
  }
  return device?.address ? { name, address: device.address } : null;
}

function sortNearbyDevices(devices: ScannedDevice[], remembered?: string) {
  return [...devices].sort((left, right) => {
    const rememberedDifference =
      Number(deviceKey(right) === remembered) -
      Number(deviceKey(left) === remembered);
    if (rememberedDifference !== 0) {
      return rememberedDifference;
    }

    const transportDifference =
      Number("usbUid" in right) - Number("usbUid" in left);
    if (transportDifference !== 0) {
      return transportDifference;
    }
    const signalDifference = (right.rssi ?? -128) - (left.rssi ?? -128);
    if (signalDifference !== 0) {
      return signalDifference;
    }
    return deviceKey(left).localeCompare(deviceKey(right));
  });
}

function connectionStatus(
  connected: boolean,
  connecting: boolean,
  reconnecting: boolean,
  waitingForDevice: boolean
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
  if (waitingForDevice) {
    return { label: "等待设备上线", tone: "neutral" as const };
  }
  return { label: "未连接", tone: "neutral" as const };
}

function batteryLabel(device: DeviceState, connected: boolean) {
  if (!connected) {
    return "—";
  }
  return batteryStatusLabel(batteryStatus(device));
}

const usbSceneLabels = ["已清空", "运行中", "已完成", "故障"];
const USB_CAP_PASSIVE_BUZZER = 2;

function usbBuzzerLabel(output: UsbStatus["output"], capabilities: number) {
  // biome-ignore lint/suspicious/noBitwiseOperators: USB V1.0 reports capabilities as a bit field.
  if (!(capabilities & USB_CAP_PASSIVE_BUZZER)) {
    return "未配备蜂鸣";
  }
  if (output.buzzerState === 0) {
    return "蜂鸣已停止";
  }
  return `蜂鸣播放中 · ${output.buzzerFrequencyHz} Hz · 强度 ${output.buzzerVolume}%`;
}

function UsbStatusReadback({ capabilities }: { capabilities: number | null }) {
  const [status, setStatus] = useState<UsbStatus | null>(null);
  const [reading, setReading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const read = async () => {
    setReading(true);
    setError(null);
    try {
      setStatus(await api.readUsbStatus());
    } catch (failure) {
      setStatus(null);
      setError(asAppError(failure).message);
    } finally {
      setReading(false);
    }
  };

  return (
    <div className="connected-device__readback">
      <ActionButton busy={reading} onClick={() => runAsync(read())}>
        读取硬件状态
      </ActionButton>
      {error ? (
        <InlineAlert title="USB 状态读取失败">{error}</InlineAlert>
      ) : null}
      {status ? (
        <p className="mono" role="status">
          {usbSceneLabels[status.output.sceneState]} · RGB{" "}
          {status.output.rgb
            .map((color) => `(${color.join(", ")})`)
            .join(" / ")}
          {" · "}
          {usbBuzzerLabel(status.output, capabilities ?? 0)}
          {" · "}租期 {status.runtime.leaseRemainingMs} ms
          {status.runtime.faultFlags
            ? ` · ${usbFaultMessage(status.runtime.faultFlags)}`
            : ""}
        </p>
      ) : null}
    </div>
  );
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
  waitingForDevice,
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
  waitingForDevice: boolean;
}) {
  const isUsb = "usbUid" in managedDevice;
  let ConnectionIcon = Bluetooth;
  if (isUsb) {
    ConnectionIcon = PlugZap;
  } else if (connected) {
    ConnectionIcon = Radio;
  }
  const status = connectionStatus(
    connected,
    connecting,
    reconnecting,
    waitingForDevice
  );
  const detail = (value: string | number | null) =>
    connected ? (value ?? "—") : "—";
  const hardwareRevision =
    device.hardwareRevision === null
      ? null
      : `0x${device.hardwareRevision.toString(16).padStart(4, "0")}`;
  let disconnectLabel = "断开连接";
  if (reconnecting) {
    disconnectLabel = "停止重连";
  }
  if (waitingForDevice) {
    disconnectLabel = "停止等待";
  }

  return (
    <section aria-labelledby="my-device-title">
      <h2 className="section-title" id="my-device-title">
        我的设备
      </h2>
      <Card className="connected-device">
        <div className="connected-device__identity">
          <div className={cn("device-orb", connected && "is-connected")}>
            <ConnectionIcon aria-hidden="true" />
          </div>
          <div className="connected-device__name">
            <strong>{managedDevice.name || "AgentCore-Light"}</strong>
            <span className="mono">
              {isUsb ? managedDevice.usbUid : managedDevice.address}
            </span>
          </div>
          <StatusTag tone={status.tone}>
            {connected ? <CheckCircle2 aria-hidden="true" size={13} /> : null}
            {status.label}
          </StatusTag>
        </div>
        <div className="connected-device__details">
          <dl className="device-stats">
            <div>
              {isUsb ? (
                <PlugZap
                  aria-hidden="true"
                  className="device-stat__icon"
                  size={16}
                />
              ) : (
                <Battery
                  aria-hidden="true"
                  className="device-stat__icon"
                  size={16}
                />
              )}
              <div className="device-stat__copy">
                <dt>{isUsb ? "供电" : "电量"}</dt>
                <dd>{isUsb ? "USB 供电" : batteryLabel(device, connected)}</dd>
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
                <dt>{isUsb ? "硬件版本" : "硬件型号"}</dt>
                <dd>
                  {detail(isUsb ? hardwareRevision : device.hardwareVariant)}
                </dd>
              </div>
            </div>
          </dl>
          <div className="device-actions">
            {connected || reconnecting || waitingForDevice ? (
              <ActionButton
                busy={deviceAction === "disconnect"}
                disabled={deviceAction !== null || connecting}
                onClick={onDisconnect}
              >
                <Unplug aria-hidden="true" size={16} />
                {disconnectLabel}
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
        {isUsb && connected ? (
          <UsbStatusReadback
            capabilities={device.capabilityBits}
            key={managedDevice.usbUid}
          />
        ) : null}
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
  const isUsb = "usbUid" in device;
  let actionLabel = "连接";
  if (isConnected) {
    actionLabel = "已连接";
  } else if (isRemembered) {
    actionLabel = "重新连接";
  }

  return (
    <Card className="device-row">
      <div className="device-orb">
        {isUsb ? (
          <PlugZap aria-hidden="true" />
        ) : (
          <Bluetooth aria-hidden="true" />
        )}
      </div>
      <div className="device-row__copy">
        <strong>{device.name || "AgentCore-Light"}</strong>
        <span className="mono">{isUsb ? device.usbUid : device.address}</span>
      </div>
      <span className="signal-copy">
        {isUsb ? (
          "USB"
        ) : (
          <>
            <Signal aria-hidden="true" size={15} /> {signalLabel(device.rssi)}
          </>
        )}
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
  const [scanErrors, setScanErrors] = useState<{ ble?: string; usb?: string }>(
    {}
  );
  const [connecting, setConnecting] = useState<string | null>(null);
  const [hasScanned, setHasScanned] = useState(false);
  const [deviceAction, setDeviceAction] = useState<
    "disconnect" | "forget" | null
  >(null);
  const [confirmForget, setConfirmForget] = useState(false);
  const scanId = useRef(0);

  const scan = useCallback(async () => {
    const id = ++scanId.current;
    setScanning(true);
    setScanErrors({});

    const collect = async (
      transport: "ble" | "usb",
      result: Promise<ScannedDevice[]>
    ) => {
      let found: ScannedDevice[] = [];
      try {
        found = await result;
      } catch (error) {
        if (scanId.current === id) {
          setScanErrors((current) => ({
            ...current,
            [transport]: asAppError(error).message,
          }));
        }
      }
      if (scanId.current === id) {
        setDevices((current) => [
          ...current.filter(
            (device) => "usbUid" in device !== (transport === "usb")
          ),
          ...found,
        ]);
      }
    };

    await Promise.all([
      collect(
        "ble",
        api
          .scanDevices()
          .then((items) => items.filter((item) => item.recognized))
      ),
      collect(
        "usb",
        api
          .scanUsbDevices()
          .then((items) =>
            items.map((item) => ({ ...item, recognized: true, rssi: null }))
          )
      ),
      new Promise((resolve) => window.setTimeout(resolve, 400)),
    ]);
    if (scanId.current === id) {
      setScanning(false);
      setHasScanned(true);
    }
  }, []);

  useEffect(() => {
    runAsync(scan());
    return () => {
      scanId.current += 1;
    };
  }, [scan]);

  const connect = async (device: RememberedDevice) => {
    setConnecting(deviceKey(device));
    try {
      await api.connectDevice(device);
      await refresh();
      notify({
        tone: "success",
        title: "设备已连接",
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
  const rememberedKey = rememberedDevice
    ? deviceKey(rememberedDevice)
    : undefined;
  const currentDevice = snapshotDevice(snapshot?.device);
  const currentKey = currentDevice ? deviceKey(currentDevice) : undefined;
  const sortedDevices = useMemo(
    () => sortNearbyDevices(devices, rememberedKey),
    [devices, rememberedKey]
  );
  const managedDevice = rememberedDevice ?? currentDevice;
  const managedKey = managedDevice ? deviceKey(managedDevice) : undefined;
  const managedDeviceIsConnected = Boolean(
    managedDevice && snapshot?.device.connected && currentKey === managedKey
  );
  const managedDeviceIsConnecting =
    managedDevice != null && connecting === managedKey;
  const usbFaultFlags = snapshot?.device.usbFaultFlags ?? 0;

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
        description="查找并连接 USB 或蓝牙状态灯"
        title="设备"
      />

      {scanning ? (
        <Card className="scan-status" role="status">
          <span className="scan-pip" />
          <span>正在查找 USB 和蓝牙灯牌…</span>
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
      {scanErrors.ble ? (
        <InlineAlert title="无法扫描蓝牙设备">
          {scanErrors.ble}。请检查系统蓝牙权限后重试。
        </InlineAlert>
      ) : null}
      {scanErrors.usb ? (
        <InlineAlert title="无法查找 USB 灯牌">{scanErrors.usb}</InlineAlert>
      ) : null}
      {fault ? (
        <InlineAlert title="设备故障">
          来源 {fault.source}，故障码 {fault.code}，上下文 {fault.context}。
        </InlineAlert>
      ) : null}
      {usbFaultFlags ? (
        <InlineAlert title="USB 灯牌故障">
          {usbFaultMessage(usbFaultFlags)}
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
          waitingForDevice={snapshot.device.waitingForDevice}
        />
      ) : null}

      <section aria-labelledby="nearby-title">
        <h2 className="section-title" id="nearby-title">
          发现的设备
        </h2>
        {!scanning && devices.length === 0 && hasScanned ? (
          <Card>
            <EmptyState
              action={
                <ActionButton onClick={() => runAsync(scan())}>
                  <RefreshCw size={16} /> 重新查找设备
                </ActionButton>
              }
              description="请检查 USB 数据线，或确认蓝牙灯牌已上电且在广播范围内。"
              icon={<WifiOff />}
              title="未发现状态灯"
            />
          </Card>
        ) : (
          <div className="device-list">
            {sortedDevices.map((device) => (
              <ScanDeviceRow
                connecting={connecting === deviceKey(device)}
                device={device}
                isConnected={Boolean(
                  snapshot?.device.connected && currentKey === deviceKey(device)
                )}
                isRemembered={rememberedKey === deviceKey(device)}
                key={deviceKey(device)}
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
