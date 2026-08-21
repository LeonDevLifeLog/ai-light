//! L4 BLE 设备层：btleplug 实现 `TransportIo` + 扫描/连接管理（协议 V0.4 §2/§5）
//!
//! - 识别：广播名 `ACLight-` 前缀 **或** 服务发现含 GB_TRANS 协议 UUID（对齐 pyPcTest）
//! - 连接：connect → discover services → 订阅 TX Notify → 组帧（FrameParser）→ 帧流
//! - 写入：按 ATT payload 上限分片（设备端按协议组帧，不依赖包边界，V0.4 §2.3）

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use btleplug::api::{
    Central, Characteristic, Manager as _, Peripheral as _, ScanFilter, WriteType,
};
use btleplug::platform::{Adapter, Manager, Peripheral};
use serde::Serialize;
use tokio::sync::mpsc;

use crate::protocol::{Frame, FrameParser};
use crate::transport::TransportIo;

/// GB_TRANS 服务/特征 UUID（协议 §2.2）
pub const GB_TRANS_SERVICE_UUID: &str = "E7BAA2E6-97AD-E697-A0E7-BABF73657276";
pub const GB_TRANS_RX_UUID: &str = "E7BAA2E6-97AD-E697-A0E7-BABF72786372";
pub const GB_TRANS_TX_UUID: &str = "E7BAA2E6-97AD-E697-A0E7-BABF74786372";

/// 广播名识别前缀（协议 §2.1）
pub const NAME_PREFIX: &str = "ACLight-";

/// ATT payload 分片上限（保守取 MTU 23 的 payload；协议目标 MTU 247 可后续协商优化）
const ATT_PAYLOAD_MAX: usize = 20;

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

/// 扫描并连接指定地址的设备（地址自动归一化）。
/// 返回 (BleIo, 显示名)。
pub async fn connect_to_address(
    adapter: &Adapter,
    address: &str,
) -> Result<(BleIo, String), BleError> {
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
    let io = BleIo::connect(peripheral).await?;
    Ok((io, name))
}

/// BLE 传输实现：扫描结果中的 peripheral + 已连接的 GATT 特征
pub struct BleIo {
    peripheral: Peripheral,
    rx_char: Characteristic,
    frames: tokio::sync::Mutex<mpsc::UnboundedReceiver<Frame>>,
}

impl BleIo {
    /// 连接并初始化：discover → 找 GB_TRANS 特征 → 订阅 TX → 启动组帧任务
    pub async fn connect(peripheral: Peripheral) -> Result<Self, BleError> {
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
        tokio::spawn(async move {
            let mut parser = FrameParser::new();
            while let Some(notification) = futures_util::StreamExt::next(&mut stream).await {
                // 只处理 GB_TRANS TX 特征的通知（其他特征（如 BAS）由上层订阅处理）
                if notification.uuid != tx_char_uuid {
                    continue;
                }
                parser.push(&notification.value);
                while let Some(frame) = parser.next_frame() {
                    if tx.send(frame).is_err() {
                        return;
                    }
                }
            }
        });

        Ok(Self {
            peripheral,
            rx_char,
            frames: tokio::sync::Mutex::new(rx),
        })
    }

    /// 广播名识别判断（调用方扫描后使用）
    pub fn recognized(&self) -> bool {
        false // 由 BleDeviceInfo 携带；此处保留接口占位
    }

    pub async fn disconnect(&self) {
        let _ = self.peripheral.disconnect().await;
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
