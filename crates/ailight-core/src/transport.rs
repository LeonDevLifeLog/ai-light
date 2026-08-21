//! L3 单 writer 发送队列 + 事务状态机（协议 §3.5 / §15.6）
//!
//! - 所有出站命令走同一 mpsc 队列，单一 writer task 消费（同一时刻仅一个事务在途）
//! - 事务：发送 → 等待应答（默认 500ms）→ 超时重发（保持原序列号，最多 2 次）
//! - 设备主动事件（0xE0~0xEF）在等待应答期间透传给上层 handler，不参与事务匹配
//! - 应答匹配：cmd = 请求 | 0x80（协议 §3.4）；序列号透传对账

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;

use crate::protocol::{self, Frame, ParseError, RESPONSE_TIMEOUT_MS};

/// 底层传输接口（由 BLE 层实现；也支持 mock 测试）
#[async_trait]
pub trait TransportIo: Send + Sync + 'static {
    /// 发送原始字节（完整帧）
    async fn write(&self, bytes: Vec<u8>) -> Result<(), String>;
    /// 等待下一帧（设备 Notify 流）；通道关闭返回 None
    async fn next_frame(&self) -> Option<Frame>;
}

#[derive(Debug)]
pub enum TransportError {
    /// 底层 IO 失败
    Io(String),
    /// 应答超时且重试耗尽
    Timeout,
    /// 接收通道关闭
    Closed,
    /// 帧解析失败
    Parse(ParseError),
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransportError::Io(e) => write!(f, "IO 错误: {e}"),
            TransportError::Timeout => write!(f, "应答超时"),
            TransportError::Closed => write!(f, "传输通道关闭"),
            TransportError::Parse(e) => write!(f, "帧解析失败: {e:?}"),
        }
    }
}

impl std::error::Error for TransportError {}

/// 出站事务
struct Outbound {
    cmd: u8,
    data: Vec<u8>,
    on_done: oneshot::Sender<Result<Frame, TransportError>>,
}

/// 发送队列句柄（克隆后可从任意线程投递命令）
#[derive(Clone)]
pub struct Transport {
    tx: mpsc::Sender<Outbound>,
}

impl Transport {
    /// 创建传输；`timeout_ms` 应答超时（0 = 默认 500ms）
    ///
    /// **Precondition**: 必须在 Tokio runtime 上下文中调用（内部使用 `tokio::spawn`
    /// 启动 writer 任务；该函数依赖 thread-local runtime handle）。
    /// Tauri 的 `.setup()` 回调运行在 AppKit 主线程、**不在** runtime 上下文中，
    /// 调用前需用 `tauri::async_runtime::handle().inner().enter()` 的 guard 包住。
    /// 详见 `docs/decisions/ADR-0003-async-执行模型边界与setup契约.md` D-02 / D-03
    /// 与 `docs/specs/architecture.md` KAD-08。
    pub fn new(io: Arc<dyn TransportIo>, timeout_ms: Option<u64>) -> Self {
        // 开发期防御：未来若有人从 sync 上下文（特别是 Tauri setup 回调）误调，
        // 在 debug 构建立刻可见，避免线上 abort（KAD-08 D-03）。
        debug_assert!(
            tokio::runtime::Handle::try_current().is_ok(),
            "Transport::new 必须在 Tokio runtime 上下文中调用（内部使用 tokio::spawn）。\
             Tauri setup 回调请用 tauri::async_runtime::handle().inner().enter() 的 guard 包住调用。\
             见 docs/decisions/ADR-0003 / KAD-08。"
        );
        let (tx, rx) = mpsc::channel(64);
        let timeout_ms = timeout_ms.unwrap_or(RESPONSE_TIMEOUT_MS);
        tokio::spawn(writer_task(io, rx, timeout_ms));
        Self { tx }
    }

    /// 投递一个命令事务（自动分配序列号由任务内部递增）。
    /// 返回应答帧（已按 cmd 匹配）。
    pub async fn request(&self, cmd: u8, data: Vec<u8>) -> Result<Frame, TransportError> {
        let (on_done, done) = oneshot::channel();
        self.tx
            .send(Outbound { cmd, data, on_done })
            .await
            .map_err(|_| TransportError::Closed)?;
        done.await.map_err(|_| TransportError::Closed)?
    }

    /// 便捷方法：SET_SCENE
    pub async fn set_scene(&self, scene: &protocol::OutputScene) -> Result<Frame, TransportError> {
        self.request(protocol::CMD_SET_SCENE, scene.encode_data())
            .await
    }

    /// 便捷方法：PING
    pub async fn ping(&self) -> Result<Frame, TransportError> {
        self.request(protocol::CMD_PING, vec![]).await
    }

    /// 便捷方法：RESET_OUTPUTS
    pub async fn reset_outputs(&self) -> Result<Frame, TransportError> {
        self.request(protocol::CMD_RESET_OUTPUTS, vec![]).await
    }
}

async fn writer_task(io: Arc<dyn TransportIo>, mut rx: mpsc::Receiver<Outbound>, timeout_ms: u64) {
    let mut seq: u16 = 0;
    while let Some(out) = rx.recv().await {
        seq = seq.wrapping_add(1);
        if seq == 0 {
            seq = 1;
        }
        let result = execute_transaction(&io, &out, seq, timeout_ms).await;
        let _ = out.on_done.send(result);
    }
}

async fn execute_transaction(
    io: &Arc<dyn TransportIo>,
    out: &Outbound,
    seq: u16,
    timeout_ms: u64,
) -> Result<Frame, TransportError> {
    let expected_cmd = protocol::response_cmd(out.cmd);
    let mut retries: u8 = 0;
    loop {
        let frame = protocol::build_frame(out.cmd, seq, &out.data);
        io.write(frame).await.map_err(TransportError::Io)?;

        match timeout(
            Duration::from_millis(timeout_ms),
            wait_for_response(io, expected_cmd),
        )
        .await
        {
            Ok(Ok(frame)) => return Ok(frame),
            Ok(Err(e)) => return Err(e),
            Err(_) => {
                // 超时：保持原序列号重发（协议 §3.5）
                if retries >= protocol::MAX_RETRIES {
                    tracing::warn!("命令 0x{:02X} seq={seq} 应答超时，重试耗尽", out.cmd);
                    return Err(TransportError::Timeout);
                }
                retries += 1;
                tracing::warn!(
                    "命令 0x{:02X} seq={seq} 应答超时，第 {retries}/{} 次重发",
                    out.cmd,
                    protocol::MAX_RETRIES
                );
            }
        }
    }
}

async fn wait_for_response(
    io: &Arc<dyn TransportIo>,
    expected_cmd: u8,
) -> Result<Frame, TransportError> {
    loop {
        match io.next_frame().await {
            Some(f) => {
                if f.cmd == expected_cmd {
                    return Ok(f);
                }
                // 非匹配帧：设备主动事件或其他应答，静默跳过（事件由 BLE 层另行分发）
                tracing::debug!("等待应答时收到非匹配帧 cmd=0x{:02X}", f.cmd);
            }
            None => return Err(TransportError::Closed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tokio::sync::mpsc as tmpsc;

    /// 可控 mock：writes 记录发送的帧；feed 通道由测试侧喂入接收帧
    struct MockIo {
        writes: Arc<Mutex<Vec<Vec<u8>>>>,
        feed: tmpsc::UnboundedSender<Frame>,
        rx: tokio::sync::Mutex<tmpsc::UnboundedReceiver<Frame>>,
    }

    impl MockIo {
        fn new() -> Arc<Self> {
            let (tx, rx) = tmpsc::unbounded_channel();
            Arc::new(Self {
                writes: Arc::new(Mutex::new(vec![])),
                feed: tx,
                rx: tokio::sync::Mutex::new(rx),
            })
        }
        fn writes(&self) -> Vec<Vec<u8>> {
            self.writes.lock().unwrap().clone()
        }
        fn feed_frame(&self, f: Frame) {
            self.feed.send(f).unwrap();
        }
    }

    #[async_trait]
    impl TransportIo for MockIo {
        async fn write(&self, bytes: Vec<u8>) -> Result<(), String> {
            self.writes.lock().unwrap().push(bytes);
            Ok(())
        }
        async fn next_frame(&self) -> Option<Frame> {
            self.rx.lock().await.recv().await
        }
    }

    /// 测试驱动：喂应答帧
    struct Harness {
        io: Arc<MockIo>,
        transport: Transport,
    }

    fn harness(timeout_ms: u64) -> Harness {
        let io = MockIo::new();
        let transport = Transport::new(io.clone(), Some(timeout_ms));
        Harness { io, transport }
    }

    fn ack_frame(cmd: u8, seq: u16) -> Frame {
        Frame {
            seq,
            cmd: protocol::response_cmd(cmd),
            data: vec![0x00],
        }
    }

    #[tokio::test]
    async fn successful_transaction() {
        let h = harness(500);
        let task = tokio::spawn(async move {
            let f = h.transport.ping().await.unwrap();
            assert_eq!(f.cmd, protocol::response_cmd(protocol::CMD_PING));
        });
        // 等 write 到达后喂应答
        tokio::time::sleep(Duration::from_millis(50)).await;
        h.io.feed_frame(ack_frame(protocol::CMD_PING, 1));
        task.await.unwrap();
        assert_eq!(h.io.writes().len(), 1);
        // 帧正确（seq=1）
        let parsed = protocol::parse_frame(&h.io.writes()[0]).unwrap().0;
        assert_eq!(parsed.cmd, protocol::CMD_PING);
        assert_eq!(parsed.seq, 1);
    }

    #[tokio::test]
    async fn retry_then_success_same_seq() {
        let h = harness(100); // 短超时加速测试
        let task = tokio::spawn(async move {
            let f = h
                .transport
                .request(protocol::CMD_RESET_OUTPUTS, vec![])
                .await
                .unwrap();
            assert_eq!(f.cmd, protocol::response_cmd(protocol::CMD_RESET_OUTPUTS));
        });
        // 等待第 3 次重发完成（前两次超时，不喂帧），再喂应答
        for _ in 0..100 {
            if h.io.writes().len() >= 3 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(h.io.writes().len(), 3, "应已重发 3 次");
        h.io.feed_frame(ack_frame(protocol::CMD_RESET_OUTPUTS, 1));
        task.await.unwrap();
        // 共发送 3 次（1 次 + 2 次重发），序列号保持一致
        let writes = h.io.writes();
        assert_eq!(writes.len(), 3);
        for w in &writes {
            let parsed = protocol::parse_frame(w).unwrap().0;
            assert_eq!(parsed.seq, 1, "重试必须保持原序列号");
            assert_eq!(parsed.cmd, protocol::CMD_RESET_OUTPUTS);
        }
    }

    #[tokio::test]
    async fn retry_exhausted_timeout() {
        let h = harness(50);
        let result = h.transport.ping().await;
        assert!(matches!(result, Err(TransportError::Timeout)));
        // 1 + 2 次重发
        assert_eq!(h.io.writes().len(), 3);
    }

    #[tokio::test]
    async fn events_skipped_during_wait() {
        let h = harness(500);
        let task = tokio::spawn(async move {
            let f = h.transport.ping().await.unwrap();
            assert_eq!(f.cmd, protocol::response_cmd(protocol::CMD_PING));
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        // 先喂一个设备事件（DEVICE_READY），再喂应答
        h.io.feed_frame(Frame {
            seq: 99,
            cmd: protocol::EVT_DEVICE_READY,
            data: vec![4, 1, 0, 0, 1, 1],
        });
        h.io.feed_frame(ack_frame(protocol::CMD_PING, 1));
        task.await.unwrap();
    }

    #[tokio::test]
    async fn serial_queue_ordering() {
        let h = harness(200);
        let t1 = tokio::spawn({
            let tr = h.transport.clone();
            async move {
                let f = tr.ping().await.unwrap();
                f.seq
            }
        });
        let t2 = tokio::spawn({
            let tr = h.transport.clone();
            async move {
                let f = tr.reset_outputs().await.unwrap();
                f.seq
            }
        });
        // 等待两个 write，再依次应答
        tokio::time::sleep(Duration::from_millis(80)).await;
        // 第一个事务完成（seq=1）
        h.io.feed_frame(ack_frame(protocol::CMD_PING, 1));
        assert_eq!(t1.await.unwrap(), 1);
        // 第二个事务（seq=2）
        h.io.feed_frame(ack_frame(protocol::CMD_RESET_OUTPUTS, 2));
        assert_eq!(t2.await.unwrap(), 2);
        // 严格串行：两个 write 已按序发出
        let writes = h.io.writes();
        assert_eq!(writes.len(), 2);
    }

    /// 反向测试：复现 macOS 启动期 abort 路径（KAD-08 / ADR-0003）。
    ///
    /// 在新线程里构造 `Transport`（thread-local runtime 上下文为空），等价于
    /// 生产环境 Tauri `.setup()` 回调在 AppKit 主线程（非 runtime）构造 Engine 的
    /// 失败路径。`debug_assert!` 应触发并被 `catch_unwind` 接住。
    ///
    /// 仅在 debug 构建有意义（release 下 `debug_assert!` 被消除）。
    #[test]
    #[cfg(debug_assertions)]
    fn transport_new_requires_tokio_runtime() {
        let io = MockIo::new();
        let handle = std::thread::Builder::new()
            .spawn(|| {
                // 双重确认：在新线程里 try_current() 必返回 Err（生产事故路径）。
                assert!(
                    tokio::runtime::Handle::try_current().is_err(),
                    "新线程不应自带 runtime 上下文"
                );
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let _ = Transport::new(io, None);
                }));
                assert!(
                    outcome.is_err(),
                    "Transport::new 在非 runtime 上下文应触发 debug_assert；\
                     若此处通过，说明 KAD-08 / ADR-0003 的防御被悄悄关掉了。"
                );
            })
            .expect("spawn 测试线程");
        handle.join().expect("join 测试线程");
    }
}
