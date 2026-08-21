//! L4 BLE 设备层：btleplug 实现 `TransportIo` + 扫描/连接管理（协议 V0.4 §2/§5）
//!
//! - 识别：广播名 `ACLight-` 前缀 **或** 服务发现含 GB_TRANS 协议 UUID（对齐 pyPcTest）
//! - 连接：connect → discover services → 订阅 TX Notify → 组帧（FrameParser）→ 帧流
//! - 写入：按 ATT payload 上限分片（设备端按协议组帧，不依赖包边界，V0.4 §2.3）

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use btleplug::api::{
    Central, CentralEvent, Characteristic, Manager as _, Peripheral as _, ScanFilter, WriteType,
};
use btleplug::platform::{Adapter, Manager, Peripheral};
use futures_util::StreamExt;
use serde::Serialize;
use tokio::sync::mpsc;

use crate::protocol::{
    self, Frame, FrameParser, PowerStatus, CMD_GET_CAPABILITIES, CMD_GET_DEVICE_INFO,
    CMD_GET_POWER_STATUS, EVT_BUTTON_EVENT, EVT_DEVICE_READY, EVT_FAULT_EVENT, EVT_POWER_CHANGED,
};
use crate::transport::TransportIo;

/// GB_TRANS 服务/特征 UUID（协议 §2.2）
pub const GB_TRANS_SERVICE_UUID: &str = "E7BAA2E6-97AD-E697-A0E7-BABF73657276";
pub const GB_TRANS_RX_UUID: &str = "E7BAA2E6-97AD-E697-A0E7-BABF72786372";
pub const GB_TRANS_TX_UUID: &str = "E7BAA2E6-97AD-E697-A0E7-BABF74786372";

/// 广播名识别前缀（协议 §2.1）
pub const NAME_PREFIX: &str = "ACLight-";

/// ATT payload 分片上限（保守取 MTU 23 的 payload；协议目标 MTU 247 可后续协商优化）
const ATT_PAYLOAD_MAX: usize = 20;
/// 等待 DEVICE_READY 的超时（协议 §5：CCC 使能后设备应主动上报）
const HANDSHAKE_READY_TIMEOUT_MS: u64 = 3000;
/// 握手各请求的应答超时
const HANDSHAKE_RESPONSE_TIMEOUT_MS: u64 = 2000;

/// 设备主动上报事件（协议 §11），由连接方消费并映射为 Tauri events
#[derive(Debug, Clone)]
pub enum BleEvent {
    /// DEVICE_READY (0xE0)：连接建立后设备主动上报（握手消费）
    DeviceReady(protocol::DeviceReady),
    /// POWER_CHANGED (0xE2)：电源状态变化
    PowerChanged(PowerStatus),
    /// BUTTON_EVENT (0xE3)：仅 BUTTON 能力置位时上报
    ButtonEvent { event: u8, duration_ms: u16 },
    /// FAULT_EVENT (0xEF)：设备故障
    Fault { source: u8, code: u8, context: u16 },
    /// 设备断开（通知流结束或 CentralEvent::DeviceDisconnected）
    Disconnected,
}

/// V0.4 §5 握手结果
#[derive(Debug, Clone)]
pub struct HandshakeInfo {
    pub device_info: protocol::DeviceInfo,
    pub capabilities: protocol::Capabilities,
    /// 设备具备电源能力位时读取；否则为 None
    pub power: Option<PowerStatus>,
}

/// 是否按能力位需要读取电源状态（§15.1：按能力位决定 BAS 订阅与 UI）
pub fn power_status_needed(capability_bits: u32) -> bool {
    capability_bits
        & (protocol::CAP_BATTERY_PRESENT
            | protocol::CAP_BATTERY_ADC
            | protocol::CAP_CHARGE_STATUS
            | protocol::CAP_EXTERNAL_POWER_DETECT)
        != 0
}

/// 断连退避重连延迟（秒）：5, 10, 15, 20, 25（客户端主动尝试窗口约 60~75s，协议 §13 宽限期）
pub fn reconnect_delay_secs(attempt: u32) -> u64 {
    u64::from(5 * attempt.max(1)).min(60)
}

/// 帧分流：设备主动事件 vs 请求应答
fn classify_frame(frame: &Frame) -> bool {
    matches!(
        frame.cmd,
        EVT_DEVICE_READY | EVT_POWER_CHANGED | EVT_BUTTON_EVENT | EVT_FAULT_EVENT
    )
}

/// 主动事件帧 → BleEvent（解析失败返回 None，仅记录日志）
fn parse_event(frame: &Frame) -> Option<BleEvent> {
    match frame.cmd {
        EVT_DEVICE_READY => protocol::parse_device_ready(&frame.data).map(BleEvent::DeviceReady),
        EVT_POWER_CHANGED => protocol::parse_power_changed(&frame.data).map(BleEvent::PowerChanged),
        EVT_BUTTON_EVENT => protocol::parse_button_event(&frame.data).map(|b| BleEvent::ButtonEvent {
            event: b.event,
            duration_ms: b.duration_ms,
        }),
        EVT_FAULT_EVENT => protocol::parse_fault_event(&frame.data).map(|f| BleEvent::Fault {
            source: f.source,
            code: f.code,
            context: f.context,
        }),
        _ => None,
    }
}

#[derive(Debug)]
pub enum BleError {
    NoAdapter,
    Scan(String),
    DeviceNotFound(String),
    Connect(String),
    ServiceNotFound,
    CharacteristicNotFound,
    Subscribe(String),
    Write(String),
    Closed,
}

impl std::fmt::Display for BleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BleError::NoAdapter => write!(f, "无可用 BLE 适配器"),
            BleError::Scan(e) => write!(f, "扫描失败: {e}"),
            BleError::DeviceNotFound(a) => write!(f, "未找到设备: {a}"),
            BleError::Connect(e) => write!(f, "连接失败: {e}"),
            BleError::ServiceNotFound => write!(f, "未发现 GB_TRANS 服务"),
            BleError::CharacteristicNotFound => write!(f, "未发现 RX/TX 特征"),
            BleError::Subscribe(e) => write!(f, "订阅 Notify 失败: {e}"),
            BleError::Write(e) => write!(f, "写入失败: {e}"),
            BleError::Closed => write!(f, "通知流已关闭"),
        }
    }
}

impl std::error::Error for BleError {}

#[derive(Debug, Clone, Serialize)]
pub struct BleDeviceInfo {
    pub name: Option<String>,
    pub address: String,
    pub rssi: Option<i16>,
    pub recognized: bool,
}

/// 归一化地址：`AA-BB-CC-DD-EE-FF` / `AA:BB:CC:DD:EE:FF` → 大写冒号形式
pub fn normalize_address(s: &str) -> String {
    let clean: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    let upper = clean.to_uppercase();
    if upper.len() == 12 {
        upper
            .as_bytes()
            .chunks(2)
            .map(|chunk| String::from_utf8_lossy(chunk).to_string())
            .collect::<Vec<_>>()
            .join(":")
    } else {
        s.to_string()
    }
}

fn is_recognized(name: &Option<String>) -> bool {
    matches!(name, Some(n) if n.starts_with(NAME_PREFIX))
}

/// 扫描（默认 5s）。`recognized` = 名称前缀命中（服务 UUID 识别需连接后，见握手流程）
pub async fn scan(adapter: &Adapter, timeout_secs: u64) -> Result<Vec<BleDeviceInfo>, BleError> {
    adapter
        .start_scan(ScanFilter::default())
        .await
        .map_err(|e| BleError::Scan(e.to_string()))?;
    tokio::time::sleep(Duration::from_secs(timeout_secs)).await;
    let peripherals = adapter
        .peripherals()
        .await
        .map_err(|e| BleError::Scan(e.to_string()))?;
    let mut out = Vec::new();
    for p in peripherals {
        if let Ok(Some(props)) = p.properties().await {
            out.push(BleDeviceInfo {
                name: props.local_name.clone(),
                address: normalize_address(&props.address.to_string()),
                rssi: props.rssi,
                recognized: is_recognized(&props.local_name),
            });
        }
    }
    let _ = adapter.stop_scan().await;
    Ok(out)
}

/// 获取默认适配器
pub async fn default_adapter() -> Result<Adapter, BleError> {
    let manager = Manager::new()
        .await
        .map_err(|e| BleError::Scan(e.to_string()))?;
    let adapters = manager
        .adapters()
        .await
        .map_err(|e| BleError::Scan(e.to_string()))?;
    adapters.into_iter().next().ok_or(BleError::NoAdapter)
}

/// 扫描并连接指定地址的设备（地址自动归一化），完成 V0.4 §5 握手。
/// 返回 (BleIo, 显示名, 握手信息)。
pub async fn connect_to_address(
    adapter: &Adapter,
    address: &str,
) -> Result<(BleIo, String, HandshakeInfo), BleError> {
    let peripherals = adapter
        .peripherals()
        .await
        .map_err(|e| BleError::Connect(e.to_string()))?;
    let addr_norm = normalize_address(address);
    let mut found: Option<(Peripheral, String)> = None;
    for p in peripherals {
        let props = p.properties().await.ok().flatten();
        let addr = props
            .as_ref()
            .map(|pr| normalize_address(&pr.address.to_string()));
        if addr.as_deref() == Some(addr_norm.as_str()) {
            let name = props
                .as_ref()
                .and_then(|pr| pr.local_name.clone())
                .unwrap_or_else(|| address.to_string());
            found = Some((p, name));
            break;
        }
    }
    let (peripheral, name) = found.ok_or_else(|| BleError::DeviceNotFound(address.to_string()))?;
    let (io, handshake) = BleIo::connect(adapter.clone(), peripheral).await?;
    Ok((io, name, handshake))
}

/// BLE 传输实现：扫描结果中的 peripheral + 已连接的 GATT 特征
pub struct BleIo {
    peripheral: Peripheral,
    rx_char: Characteristic,
    frames: tokio::sync::Mutex<mpsc::UnboundedReceiver<Frame>>,
    events: Option<mpsc::UnboundedReceiver<BleEvent>>,
}

impl BleIo {
    /// 连接并初始化（协议 §5）：
    /// discover → 找 GB_TRANS 特征 → 订阅 TX → 组帧/事件分流 → 断连监听 → 握手
    pub async fn connect(
        adapter: Adapter,
        peripheral: Peripheral,
    ) -> Result<(Self, HandshakeInfo), BleError> {
        peripheral
            .connect()
            .await
            .map_err(|e| BleError::Connect(e.to_string()))?;
        peripheral
            .discover_services()
            .await
            .map_err(|e| BleError::Connect(e.to_string()))?;

        let chars = peripheral.characteristics();
        let rx_char = chars
            .iter()
            .find(|c| c.uuid.to_string().to_uppercase() == GB_TRANS_RX_UUID)
            .ok_or(BleError::CharacteristicNotFound)?
            .clone();
        let tx_char = chars
            .iter()
            .find(|c| c.uuid.to_string().to_uppercase() == GB_TRANS_TX_UUID)
            .ok_or(BleError::CharacteristicNotFound)?
            .clone();

        // 订阅 TX Notify（btleplug 0.11：subscribe 无返回，notifications() 取流）
        peripheral
            .subscribe(&tx_char)
            .await
            .map_err(|e| BleError::Subscribe(e.to_string()))?;
        let mut stream = peripheral
            .notifications()
            .await
            .map_err(|e| BleError::Subscribe(e.to_string()))?;
        let tx_char_uuid = tx_char.uuid;
        let (tx, rx) = mpsc::unbounded_channel();
        let (ev_tx, ev_rx) = mpsc::unbounded_channel();

        // 组帧任务：解析 Notify 流；主动事件分流到 events，应答帧进 frames
        let ev_task_tx = ev_tx.clone();
        tokio::spawn(async move {
            let mut parser = FrameParser::new();
            while let Some(notification) = futures_util::StreamExt::next(&mut stream).await {
                // 只处理 GB_TRANS TX 特征的通知（其他特征（如 BAS）由上层订阅处理）
                if notification.uuid != tx_char_uuid {
                    continue;
                }
                parser.push(&notification.value);
                while let Some(frame) = parser.next_frame() {
                    if classify_frame(&frame) {
                        if let Some(ev) = parse_event(&frame) {
                            if ev_task_tx.send(ev).is_err() {
                                return;
                            }
                        }
                    } else if tx.send(frame).is_err() {
                        return;
                    }
                }
            }
            // 通知流结束 = 设备断开（CentralEvent 监听也会上报，接收端幂等）
            let _ = ev_task_tx.send(BleEvent::Disconnected);
        });

        // 断连监听：CentralEvent::DeviceDisconnected（该 peripheral）
        let pid = peripheral.id();
        let ev_disc_tx = ev_tx.clone();
        tokio::spawn(async move {
            let mut events = match adapter.events().await {
                Ok(stream) => stream,
                Err(e) => {
                    tracing::warn!("订阅 CentralEvent 失败: {e}");
                    return;
                }
            };
            while let Some(event) = StreamExt::next(&mut events).await {
                if let CentralEvent::DeviceDisconnected(id) = event {
                    if id == pid {
                        let _ = ev_disc_tx.send(BleEvent::Disconnected);
                        break;
                    }
                }
            }
        });

        let mut io = Self {
            peripheral: peripheral.clone(),
            rx_char,
            frames: tokio::sync::Mutex::new(rx),
            events: Some(ev_rx),
        };
        match io.handshake().await {
            Ok(info) => Ok((io, info)),
            Err(e) => {
                let _ = peripheral.disconnect().await;
                Err(e)
            }
        }
    }

    /// 取出设备事件流（连接方在 Arc 包装前调用）
    pub fn take_events(&mut self) -> Option<mpsc::UnboundedReceiver<BleEvent>> {
        self.events.take()
    }

    /// V0.4 §5 握手：等 DEVICE_READY → GET_DEVICE_INFO → GET_CAPABILITIES → GET_POWER_STATUS（按能力位）
    async fn handshake(&mut self) -> Result<HandshakeInfo, BleError> {
        let mut events = self.events.take().ok_or(BleError::Closed)?;
        let mut frames = self.frames.lock().await;
        let mut seq: u16 = 0;

        // 1. 等 DEVICE_READY（固件版本与硬件变体以事件为准，协议 §11.1 单一数据源）
        let ready = tokio::time::timeout(
            Duration::from_millis(HANDSHAKE_READY_TIMEOUT_MS),
            wait_device_ready(&mut events),
        )
        .await
        .map_err(|_| BleError::Connect("等待 DEVICE_READY 超时".into()))?
        .map_err(|e| BleError::Connect(e.to_string()))?;
        if ready.protocol_version != protocol::PROTOCOL_VERSION {
            return Err(BleError::Connect(format!(
                "协议版本不兼容: 0x{:02X}",
                ready.protocol_version
            )));
        }

        // 2. GET_DEVICE_INFO
        seq += 1;
        let info_frame = self
            .request_response(CMD_GET_DEVICE_INFO, seq, &mut frames)
            .await?;
        let (rc, device_info) = protocol::parse_device_info_response(&info_frame.data)
            .map_err(|_| BleError::Connect("GET_DEVICE_INFO 应答解析失败".into()))?;
        if rc != protocol::ResultCode::Ok {
            return Err(BleError::Connect(format!("GET_DEVICE_INFO 被拒绝: {rc}")));
        }

        // 3. GET_CAPABILITIES
        seq += 1;
        let caps_frame = self
            .request_response(CMD_GET_CAPABILITIES, seq, &mut frames)
            .await?;
        let (rc, capabilities) = protocol::parse_capabilities_response(&caps_frame.data)
            .map_err(|_| BleError::Connect("GET_CAPABILITIES 应答解析失败".into()))?;
        if rc != protocol::ResultCode::Ok {
            return Err(BleError::Connect(format!("GET_CAPABILITIES 被拒绝: {rc}")));
        }

        // 4. GET_POWER_STATUS（按能力位，§15.1）
        let power = if power_status_needed(capabilities.capability_bits) {
            seq += 1;
            let power_frame = self
                .request_response(CMD_GET_POWER_STATUS, seq, &mut frames)
                .await?;
            let (rc, power) = protocol::parse_power_status_response(&power_frame.data)
                .map_err(|_| BleError::Connect("GET_POWER_STATUS 应答解析失败".into()))?;
            if rc != protocol::ResultCode::Ok {
                tracing::warn!("GET_POWER_STATUS 被拒绝: {rc}，按无电源处理");
                None
            } else {
                Some(power)
            }
        } else {
            None
        };

        self.events = Some(events);
        Ok(HandshakeInfo {
            device_info,
            capabilities,
            power,
        })
    }

    /// 发送单帧请求并等待匹配应答（seq 透传；非匹配帧跳过）
    async fn request_response(
        &self,
        cmd: u8,
        seq: u16,
        frames: &mut mpsc::UnboundedReceiver<Frame>,
    ) -> Result<Frame, BleError> {
        let frame = protocol::build_frame(cmd, seq, &[]);
        // 握手帧短于 ATT 分片上限，单次写入
        self.peripheral
            .write(&self.rx_char, &frame, WriteType::WithoutResponse)
            .await
            .map_err(|e| BleError::Write(e.to_string()))?;
        tokio::time::timeout(
            Duration::from_millis(HANDSHAKE_RESPONSE_TIMEOUT_MS),
            async {
                loop {
                    match frames.recv().await {
                        Some(f) if f.cmd == protocol::response_cmd(cmd) => return Ok(f),
                        Some(_) => continue,
                        None => return Err(BleError::Closed),
                    }
                }
            },
        )
        .await
        .map_err(|_| BleError::Connect(format!("命令 0x{cmd:02X} 应答超时")))?
    }

    /// 广播名识别判断（调用方扫描后使用）
    pub fn recognized(&self) -> bool {
        false // 由 BleDeviceInfo 携带；此处保留接口占位
    }

    pub async fn disconnect(&self) {
        let _ = self.peripheral.disconnect().await;
    }
}

/// 握手阶段等待 DEVICE_READY；忽略握手前到达的其他事件，断开则失败
async fn wait_device_ready(
    events: &mut mpsc::UnboundedReceiver<BleEvent>,
) -> Result<protocol::DeviceReady, BleError> {
    loop {
        match events.recv().await {
            Some(BleEvent::DeviceReady(ready)) => return Ok(ready),
            Some(BleEvent::Disconnected) => {
                return Err(BleError::Connect("等待 DEVICE_READY 期间设备断开".into()))
            }
            Some(_) => continue, // 握手前不应有其他主动事件，忽略
            None => return Err(BleError::Closed),
        }
    }
}

#[async_trait]
impl TransportIo for BleIo {
    async fn write(&self, bytes: Vec<u8>) -> Result<(), String> {
        // 按 ATT payload 上限分片（协议 §2.3：设备端缓存组帧，不依赖包边界）
        for chunk in bytes.chunks(ATT_PAYLOAD_MAX) {
            self.peripheral
                .write(&self.rx_char, chunk, WriteType::WithoutResponse)
                .await
                .map_err(|e| format!("BLE 写入失败: {e}"))?;
        }
        Ok(())
    }

    async fn next_frame(&self) -> Option<Frame> {
        self.frames.lock().await.recv().await
    }
}

/// 可热切换设备的代理传输（Engine 固定持有；连接/断开时动态替换，无需重建 Engine）
///
/// - 未连接：write 返回"设备未连接"；next_frame 轮询等待设备出现
/// - 已连接：转发到当前设备
pub struct DeviceIo {
    inner: tokio::sync::RwLock<Option<Arc<dyn TransportIo>>>,
}

impl DeviceIo {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: tokio::sync::RwLock::new(None),
        })
    }

    /// 热切换设备（None = 断开）
    pub async fn set(&self, io: Option<Arc<dyn TransportIo>>) {
        *self.inner.write().await = io;
    }

    /// 当前是否已连接设备
    pub async fn is_connected(&self) -> bool {
        self.inner.read().await.is_some()
    }
}

#[async_trait]
impl TransportIo for DeviceIo {
    async fn write(&self, bytes: Vec<u8>) -> Result<(), String> {
        let guard = self.inner.read().await;
        match guard.as_ref() {
            Some(io) => io.write(bytes).await,
            None => Err("设备未连接".into()),
        }
    }

    async fn next_frame(&self) -> Option<Frame> {
        loop {
            let guard = self.inner.read().await;
            match guard.as_ref() {
                Some(io) => {
                    return io.next_frame().await;
                }
                None => {
                    drop(guard);
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_various_formats() {
        assert_eq!(normalize_address("AA-BB-CC-DD-EE-FF"), "AA:BB:CC:DD:EE:FF");
        assert_eq!(normalize_address("aa:bb:cc:dd:ee:ff"), "AA:BB:CC:DD:EE:FF");
        assert_eq!(normalize_address("AABBCCDDEEFF"), "AA:BB:CC:DD:EE:FF");
        // 非标准格式原样返回
        assert_eq!(normalize_address("garbage"), "garbage");
    }

    #[test]
    fn name_recognition() {
        assert!(is_recognized(&Some("ACLight-1A2B".into())));
        assert!(!is_recognized(&Some("Other-1A2B".into())));
        assert!(!is_recognized(&None));
    }

    fn frame(cmd: u8, data: Vec<u8>) -> Frame {
        Frame { seq: 1, cmd, data }
    }

    #[test]
    fn event_vs_response_classification() {
        // 主动事件
        assert!(classify_frame(&frame(EVT_POWER_CHANGED, vec![0x03])));
        assert!(classify_frame(&frame(EVT_DEVICE_READY, vec![0x04])));
        assert!(classify_frame(&frame(EVT_BUTTON_EVENT, vec![0x01])));
        assert!(classify_frame(&frame(EVT_FAULT_EVENT, vec![0x01])));
        // 请求应答（cmd | 0x80）
        assert!(!classify_frame(&frame(protocol::response_cmd(CMD_GET_DEVICE_INFO), vec![0x00])));
        assert!(!classify_frame(&frame(protocol::response_cmd(CMD_GET_CAPABILITIES), vec![0x00])));
        assert!(!classify_frame(&frame(protocol::response_cmd(CMD_GET_POWER_STATUS), vec![0x00])));
    }

    #[test]
    fn parse_events_from_wire_samples() {
        // 协议 §17.13 帧示例（data 区）
        let ready = parse_event(&frame(EVT_DEVICE_READY, vec![0x04, 0x01, 0x00, 0x00, 0x01, 0x01]));
        match ready {
            Some(BleEvent::DeviceReady(r)) => {
                assert_eq!(r.protocol_version, 4);
                assert_eq!(r.fw, (1, 0, 0));
                assert_eq!(r.hardware_variant, 1);
                assert_eq!(r.boot_reason, 1);
            }
            other => panic!("DEVICE_READY 解析失败: {other:?}"),
        }

        let power = parse_event(&frame(EVT_POWER_CHANGED, vec![0x03, 0x00, 0x07, 0x0F, 0x3C, 0x4B, 0x03]));
        match power {
            Some(BleEvent::PowerChanged(p)) => {
                assert_eq!(p.power_source, 3);
                assert_eq!(p.power_flags, 0x0007);
                assert_eq!(p.battery_mv, 3900);
                assert_eq!(p.battery_percent, 75);
                assert_eq!(p.charge_state, 3);
            }
            other => panic!("POWER_CHANGED 解析失败: {other:?}"),
        }

        let button = parse_event(&frame(EVT_BUTTON_EVENT, vec![0x01, 0x00, 0x78]));
        match button {
            Some(BleEvent::ButtonEvent { event, duration_ms }) => {
                assert_eq!(event, 1);
                assert_eq!(duration_ms, 120);
            }
            other => panic!("BUTTON_EVENT 解析失败: {other:?}"),
        }

        let fault = parse_event(&frame(EVT_FAULT_EVENT, vec![0x01, 0x02, 0x00, 0x03]));
        match fault {
            Some(BleEvent::Fault { source, code, context }) => {
                assert_eq!(source, 1);
                assert_eq!(code, 2);
                assert_eq!(context, 3);
            }
            other => panic!("FAULT_EVENT 解析失败: {other:?}"),
        }
    }

    #[test]
    fn power_status_needed_by_capability() {
        use crate::protocol::{
            CAP_BATTERY_ADC, CAP_BATTERY_PRESENT, CAP_CHARGE_STATUS, CAP_EXTERNAL_POWER_DETECT,
            CAP_RGB_LED,
        };
        assert!(power_status_needed(CAP_BATTERY_PRESENT));
        assert!(power_status_needed(CAP_BATTERY_ADC));
        assert!(power_status_needed(CAP_CHARGE_STATUS));
        assert!(power_status_needed(CAP_EXTERNAL_POWER_DETECT));
        assert!(power_status_needed(CAP_BATTERY_PRESENT | CAP_RGB_LED));
        assert!(!power_status_needed(CAP_RGB_LED));
        assert!(!power_status_needed(0));
    }

    #[test]
    fn reconnect_backoff_sequence() {
        assert_eq!(reconnect_delay_secs(0), 5);
        assert_eq!(reconnect_delay_secs(1), 5);
        assert_eq!(reconnect_delay_secs(2), 10);
        assert_eq!(reconnect_delay_secs(5), 25);
        assert_eq!(reconnect_delay_secs(10), 50);
        // 上限 60s
        assert_eq!(reconnect_delay_secs(12), 60);
        assert_eq!(reconnect_delay_secs(20), 60);
    }

    #[test]
    fn uuid_constants_valid() {
        for u in [GB_TRANS_SERVICE_UUID, GB_TRANS_RX_UUID, GB_TRANS_TX_UUID] {
            assert!(uuid::Uuid::parse_str(u).is_ok(), "{u} 不是合法 UUID");
        }
    }
}

/// DeviceIo 热切换测试：纯逻辑，无需硬件
#[cfg(test)]
mod device_io_tests {
    use super::*;
    use crate::protocol::CMD_PING;
    use crate::transport::TransportIo;

    /// 简单回声 mock：write 成功；next_frame 返回一个 PING 应答
    struct EchoIo;

    #[async_trait]
    impl TransportIo for EchoIo {
        async fn write(&self, _bytes: Vec<u8>) -> Result<(), String> {
            Ok(())
        }
        async fn next_frame(&self) -> Option<Frame> {
            Some(Frame {
                seq: 1,
                cmd: crate::protocol::response_cmd(CMD_PING),
                data: vec![0x00, 0x00, 0x00, 0x0E, 0x10],
            })
        }
    }

    #[tokio::test]
    async fn disconnected_write_fails() {
        let io = DeviceIo::new();
        assert!(!io.is_connected().await);
        assert!(io.write(vec![1, 2, 3]).await.is_err());
    }

    #[tokio::test]
    async fn hot_switch_connect_disconnect() {
        let io = DeviceIo::new();
        // 连接：set(Some) → write 转发成功
        io.set(Some(Arc::new(EchoIo))).await;
        assert!(io.is_connected().await);
        assert!(io.write(vec![1]).await.is_ok());
        // 断开：set(None) → write 报"设备未连接"
        io.set(None).await;
        assert!(!io.is_connected().await);
        let err = io.write(vec![1]).await.unwrap_err();
        assert!(err.contains("设备未连接"));
    }

    #[tokio::test]
    async fn next_frame_forwards_to_inner() {
        let io = DeviceIo::new();
        io.set(Some(Arc::new(EchoIo))).await;
        let f = io.next_frame().await.expect("应转发内层帧");
        assert_eq!(f.cmd, crate::protocol::response_cmd(CMD_PING));
    }
}
