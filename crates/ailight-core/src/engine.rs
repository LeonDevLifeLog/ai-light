//! L2 业务编排引擎：状态事件 → 仲裁 → 主题编译 → 单 writer 下发
//!
//! 链路（KAD-03 唯一事实源）：
//!   hook/manual 事件 → `process_event`（仲裁 + 编译 SCENE）→ outbound 队列
//!   → Engine 后台任务消费 → transport 单 writer 下发设备
//!
//! 职责：
//! - `process_event`：同步纯逻辑（可测），hook_server 与 manual 命令共用
//! - `Engine`：消费 outbound + 断线重连对齐（resync）/ 试听（preview）/ 复位（reset）

use std::sync::Arc;

use crate::arbiter::{ApplyOutcome, ArbitrationMode, HookEvent};
use crate::hook_server::SharedState;
use crate::protocol::{self, OutputScene};
use crate::theme;
use crate::transport::{Transport, TransportError};

/// outbound 通道容量（背压上限，KAD-07）
const _OUTBOUND_CAPACITY: usize = 64;

#[derive(Debug)]
pub enum EngineError {
    Theme(theme::ThemeError),
    State(String),
    Transport(TransportError),
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EngineError::Theme(e) => write!(f, "主题错误: {e}"),
            EngineError::State(e) => write!(f, "状态错误: {e}"),
            EngineError::Transport(e) => write!(f, "传输错误: {e}"),
        }
    }
}

impl std::error::Error for EngineError {}

/// 处理状态事件（同步部分：仲裁 + 编译）。
///
/// 返回 `applied`（幂等对账，hook-api §3.2）。applied 时编译结果已推入 outbound 队列，
/// 由 Engine 后台任务下发。
pub fn process_event(
    shared: &SharedState,
    source: &str,
    state: &str,
    session: Option<&str>,
    ts: Option<u64>,
) -> Result<bool, EngineError> {
    let now = (shared.now_ms)();
    // hold_ms 从主题查（ADR-0001 Q2）
    let hold_ms = {
        let guard = shared
            .theme
            .read()
            .map_err(|_| EngineError::State("theme 锁失败".into()))?;
        guard
            .as_ref()
            .and_then(|t| t.states.get(state).and_then(|s| s.hold_ms))
    };
    let ev = HookEvent {
        source: source.to_string(),
        state: state.to_string(),
        session: session.map(String::from),
        ts_ms: ts.unwrap_or(now),
    };
    let outcome = shared
        .arbiter
        .write()
        .map_err(|_| EngineError::State("arbiter 锁失败".into()))?
        .apply(&ev, hold_ms, now);
    let applied = matches!(outcome, ApplyOutcome::Applied(_));
    if applied {
        let scene = compile_current(shared)?;
        shared.send_outbound(scene).map_err(EngineError::State)?;
    }
    Ok(applied)
}

/// 编译当前业务状态为 SCENE；未映射状态 → 全灭（theme-format §3 兜底）
pub fn compile_current(shared: &SharedState) -> Result<OutputScene, EngineError> {
    let (state, theme) = {
        let state = shared
            .arbiter
            .read()
            .map_err(|_| EngineError::State("arbiter 锁失败".into()))?
            .current()
            .state
            .clone();
        let theme = shared
            .theme
            .read()
            .map_err(|_| EngineError::State("theme 锁失败".into()))?
            .clone();
        (state, theme)
    };
    match theme {
        Some(t) => match theme::compile_state(&t, &state) {
            Ok(scene) => Ok(scene),
            Err(theme::ThemeError::StateNotFound(_)) => Ok(OutputScene::none()),
            Err(e) => Err(EngineError::Theme(e)),
        },
        None => Ok(OutputScene::none()),
    }
}

/// 业务引擎：消费 outbound 下发设备 + 管理性操作
pub struct Engine {
    pub shared: Arc<SharedState>,
    transport: Transport,
}

impl Engine {
    /// 创建引擎并启动 outbound 消费任务。
    /// `io` 为底层传输（BLE 实现或 mock）。
    ///
    /// **Precondition**: 必须在 Tokio runtime 上下文中调用（内部使用 `tokio::spawn`）。
    /// Tauri `.setup()` 回调运行在 AppKit 主线程（不在 runtime），调用前需用
    /// `tauri::async_runtime::handle().inner().enter()` 的 guard 包住。
    /// 详见 `docs/decisions/ADR-0003-async-执行模型边界与setup契约.md` D-02 / D-03
    /// 与 `docs/specs/architecture.md` KAD-08。
    pub fn new(shared: Arc<SharedState>, io: Arc<dyn crate::transport::TransportIo>) -> Self {
        // 开发期防御：未来若有人从 sync 上下文误调，debug 构建立刻可见（KAD-08 D-03）。
        debug_assert!(
            tokio::runtime::Handle::try_current().is_ok(),
            "Engine::new 必须在 Tokio runtime 上下文中调用（内部使用 tokio::spawn）。\
             见 docs/decisions/ADR-0003 / KAD-08。"
        );
        let transport = Transport::new(io, None);
        let task_transport = transport.clone();
        let mut rx = shared.outbound_rx();
        tokio::spawn(async move {
            while let Some(scene) = rx.recv().await {
                // 单 writer 队列内串行下发（协议 §15.6）
                if let Err(e) = transport_set_scene(&task_transport, &scene).await {
                    tracing::error!("SCENE 下发失败: {e}");
                }
            }
        });
        Self { shared, transport }
    }

    /// 断线重连对齐：重发当前业务 SCENE（APPLY_IF_CHANGED 幂等，协议 §15.5）
    pub async fn resync(&self) -> Result<(), EngineError> {
        let scene = compile_current(&self.shared)?;
        transport_set_scene(&self.transport, &scene).await?;
        Ok(())
    }

    /// 试听：RESTART_SCENE 语义强制重播指定状态的 SCENE（不改变业务状态，ipc-contract §2.4）
    pub async fn preview(&self, state: &str, _theme_name: Option<&str>) -> Result<(), EngineError> {
        let theme = {
            let guard = self
                .shared
                .theme
                .read()
                .map_err(|_| EngineError::State("theme 锁失败".into()))?;
            guard.clone()
        };
        let t = theme.ok_or_else(|| EngineError::State("未加载主题".into()))?;
        let mut scene = theme::compile_state(&t, state).map_err(EngineError::Theme)?;
        scene.apply_mode = protocol::RESTART_SCENE;
        transport_set_scene(&self.transport, &scene).await?;
        Ok(())
    }

    /// 试听未保存的主题草稿；仅编译并下发，不替换当前主题或业务状态。
    pub async fn preview_theme(
        &self,
        draft: &theme::ThemeFile,
        state: &str,
    ) -> Result<(), EngineError> {
        theme::validate(draft).map_err(EngineError::Theme)?;
        let mut scene = theme::compile_state(draft, state).map_err(EngineError::Theme)?;
        scene.apply_mode = protocol::RESTART_SCENE;
        transport_set_scene(&self.transport, &scene).await?;
        Ok(())
    }

    /// 复位：RESET_OUTPUTS + 业务状态回 IDLE（ipc-contract §2.4 联动）
    pub async fn reset(&self) -> Result<(), EngineError> {
        let now = (self.shared.now_ms)();
        self.shared
            .arbiter
            .write()
            .map_err(|_| EngineError::State("arbiter 锁失败".into()))?
            .reset(now);
        self.transport
            .reset_outputs()
            .await
            .map(|_| ())
            .map_err(EngineError::Transport)
    }

    /// 设置仲裁模式（ipc-contract update_config）
    pub fn set_arbitration_mode(&self, mode: ArbitrationMode) {
        if let Ok(mut arb) = self.shared.arbiter.write() {
            arb.set_mode(mode);
        }
    }
}

async fn transport_set_scene(
    transport: &Transport,
    scene: &OutputScene,
) -> Result<(), EngineError> {
    let mut wire_scene = scene.clone();
    for led in &mut wire_scene.leds {
        // 临时兼容当前固件：即使是黑色空轨，brightness 也必须是 1..=100。
        led.brightness = led.brightness.max(1);
    }
    for segment in &mut wire_scene.buzzer.segments {
        // 静音间隔必须保持 frequency=0, volume=0；有声段暂按 1..=100。
        segment.volume = if segment.frequency_hz == 0 {
            0
        } else {
            segment.volume.max(1)
        };
    }
    let frame = transport
        .set_scene(&wire_scene)
        .await
        .map_err(EngineError::Transport)?;
    // 应答语义对账（协议 §8.5）：非 OK 结果码告警（低电量/参数拒绝等）
    let rc = protocol::parse_set_scene_response(&frame.data);
    if let Ok((rc, _)) = rc {
        if rc != protocol::ResultCode::Ok {
            tracing::warn!(?wire_scene, "SET_SCENE 被设备拒绝: {rc}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{Frame, OutputScene};
    use crate::theme::{self, ThemeFile};
    use std::sync::Mutex;
    use tokio::sync::mpsc as tmpsc;

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
        fn ack_next(&self) {
            // 自动应答所有 SET_SCENE：applied=1 digest=0x0000
            let f = Frame {
                seq: 1,
                cmd: protocol::response_cmd(protocol::CMD_SET_SCENE),
                data: vec![0x00, 0x01, 0x00, 0x00],
            };
            self.feed.send(f).unwrap();
        }
    }

    #[async_trait::async_trait]
    impl crate::transport::TransportIo for MockIo {
        async fn write(&self, bytes: Vec<u8>) -> Result<(), String> {
            // 自动应答，避免后台任务等待超时（SET_SCENE → applied=1 digest=0）
            if let Ok((frame, _)) = crate::protocol::parse_frame(&bytes) {
                let ack = Frame {
                    seq: frame.seq,
                    cmd: crate::protocol::response_cmd(frame.cmd),
                    data: vec![0x00, 0x01, 0x00, 0x00],
                };
                let _ = self.feed.send(ack);
            }
            self.writes.lock().unwrap().push(bytes);
            Ok(())
        }
        async fn next_frame(&self) -> Option<Frame> {
            self.rx.lock().await.recv().await
        }
    }

    fn load_default_theme() -> ThemeFile {
        let content = std::fs::read_to_string("../../docs/specs/themes/default.ailight-theme.json")
            .expect("读取 default 主题");
        theme::load(&content).expect("默认主题应合法")
    }

    fn setup() -> (Arc<SharedState>, Arc<MockIo>, Engine) {
        let shared = SharedState::new("test", ArbitrationMode::Priority, || 1000);
        *shared.theme.write().unwrap() = Some(load_default_theme());
        let io = MockIo::new();
        let engine = Engine::new(shared.clone(), io.clone());
        (shared, io, engine)
    }

    #[tokio::test]
    async fn full_business_chain() {
        let (shared, io, engine) = setup();
        let _engine = engine;

        // WORKING → applied=true，且下发一帧 SET_SCENE
        let applied = process_event(&shared, "claude-code", "WORKING", None, None).unwrap();
        assert!(applied);
        // 等待后台任务消费 outbound 并下发
        for _ in 0..100 {
            if !io.writes().is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert_eq!(io.writes().len(), 1, "应下发 1 帧");
        let parsed = crate::protocol::parse_frame(&io.writes()[0]).unwrap().0;
        assert_eq!(parsed.cmd, protocol::CMD_SET_SCENE);
        assert_eq!(parsed.seq, 1);

        // ERROR 抢占 → 再下发一帧
        let applied = process_event(&shared, "claude-code", "ERROR", None, None).unwrap();
        assert!(applied);
        for _ in 0..100 {
            if io.writes().len() >= 2 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert_eq!(io.writes().len(), 2);
        let parsed = crate::protocol::parse_frame(&io.writes()[1]).unwrap().0;
        let scene = OutputScene::decode_data(&parsed.data).unwrap();
        assert_eq!(scene.leds[0].curve, crate::protocol::CURVE_SQUARE);
        assert_eq!(scene.leds[0].repeat_count, 8);

        // 幂等：相同 source+state 不再下发
        let applied = process_event(&shared, "claude-code", "ERROR", None, None).unwrap();
        assert!(!applied);
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        assert_eq!(io.writes().len(), 2, "幂等不应重复下发");
    }

    #[tokio::test]
    async fn reset_clears_business_and_device() {
        let (shared, io, engine) = setup();
        process_event(&shared, "cc", "ERROR", None, None).unwrap();
        // 消费掉 ERROR 下发
        for _ in 0..100 {
            if !io.writes().is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        engine.reset().await.unwrap();
        // 业务状态回 IDLE
        assert_eq!(shared.arbiter.read().unwrap().current().state, "IDLE");
        // 设备收到 RESET_OUTPUTS
        let writes = io.writes();
        let last = writes.last().unwrap();
        let parsed = crate::protocol::parse_frame(last).unwrap().0;
        assert_eq!(parsed.cmd, protocol::CMD_RESET_OUTPUTS);
    }

    #[tokio::test]
    async fn unmapped_state_falls_back_to_none() {
        let (shared, io, _engine) = setup();
        let applied = process_event(&shared, "cc", "MY_CUSTOM", None, None).unwrap();
        assert!(applied);
        for _ in 0..100 {
            if !io.writes().is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        // 主题未映射 MY_CUSTOM？—— default 主题也没有 → 兜底全灭
        let parsed = crate::protocol::parse_frame(&io.writes()[0]).unwrap().0;
        let scene = OutputScene::decode_data(&parsed.data).unwrap();
        assert!(scene
            .leds
            .iter()
            .all(|led| led.high == protocol::Rgb(0, 0, 0) && led.brightness == 1));
        assert!(scene.buzzer.segments.is_empty());
    }

    #[tokio::test]
    async fn preview_restart_scene_semantics() {
        // 试听：RESTART_SCENE 强制重播，且不改变业务状态（ipc-contract §2.4）
        let (shared, io, engine) = setup();
        engine.preview("WORKING", None).await.unwrap();
        for _ in 0..100 {
            if !io.writes().is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert_eq!(io.writes().len(), 1);
        let parsed = crate::protocol::parse_frame(&io.writes()[0]).unwrap().0;
        let scene = OutputScene::decode_data(&parsed.data).unwrap();
        assert_eq!(scene.apply_mode, crate::protocol::RESTART_SCENE);
        // 业务状态未被改变（仍 IDLE）
        assert_eq!(shared.arbiter.read().unwrap().current().state, "IDLE");
    }

    #[tokio::test]
    async fn preview_unmapped_state_errors() {
        let (_shared, _io, engine) = setup();
        let r = engine.preview("NOPE", None).await;
        assert!(matches!(
            r,
            Err(EngineError::Theme(theme::ThemeError::StateNotFound(_)))
        ));
    }

    #[tokio::test]
    async fn preview_theme_draft_does_not_replace_active_theme() {
        let (shared, io, engine) = setup();
        let mut draft = load_default_theme();
        draft.scenes.get_mut("breath-blue").unwrap().leds[0]
            .as_mut()
            .unwrap()
            .brightness = 0;
        engine.preview_theme(&draft, "WORKING").await.unwrap();
        assert_eq!(io.writes().len(), 1);
        let parsed = crate::protocol::parse_frame(&io.writes()[0]).unwrap().0;
        let scene = OutputScene::decode_data(&parsed.data).unwrap();
        assert_eq!(scene.apply_mode, crate::protocol::RESTART_SCENE);
        assert_eq!(scene.leds[0].brightness, 1);
        assert_eq!(shared.theme_name.read().unwrap().as_str(), "default");
    }

    #[tokio::test]
    async fn resync_replays_current_scene() {
        // 断线重连对齐：重发当前业务 SCENE（APPLY_IF_CHANGED，协议 §15.5）
        let (shared, io, engine) = setup();
        process_event(&shared, "cc", "WORKING", None, None).unwrap();
        for _ in 0..100 {
            if !io.writes().is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert_eq!(io.writes().len(), 1);

        engine.resync().await.unwrap();
        for _ in 0..100 {
            if io.writes().len() >= 2 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert_eq!(io.writes().len(), 2);
        let parsed = crate::protocol::parse_frame(&io.writes()[1]).unwrap().0;
        let scene = OutputScene::decode_data(&parsed.data).unwrap();
        assert_eq!(scene.apply_mode, crate::protocol::APPLY_IF_CHANGED);
        assert_eq!(scene.leds[0].curve, crate::protocol::CURVE_TRIANGLE);
    }

    #[tokio::test]
    async fn reset_then_resync_no_scene() {
        // reset 后业务回 IDLE，resync 应下发全灭（无当前场景）
        let (shared, io, engine) = setup();
        process_event(&shared, "cc", "ERROR", None, None).unwrap();
        engine.reset().await.unwrap();
        // 消费掉 ERROR 下发 + RESET_OUTPUTS
        for _ in 0..100 {
            if io.writes().len() >= 2 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        engine.resync().await.unwrap();
        for _ in 0..100 {
            if io.writes().len() >= 3 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        let parsed = crate::protocol::parse_frame(&io.writes()[2]).unwrap().0;
        assert_eq!(parsed.cmd, crate::protocol::CMD_SET_SCENE);
        let scene = OutputScene::decode_data(&parsed.data).unwrap();
        assert!(scene
            .leds
            .iter()
            .all(|led| led.high == protocol::Rgb(0, 0, 0) && led.brightness == 1));
        assert!(scene.buzzer.segments.is_empty()); // IDLE → 全灭
    }

    /// 反向测试：复现 macOS 启动期 abort 路径（KAD-08 / ADR-0003）。
    ///
    /// `Engine::new` 在新线程（无 runtime 上下文）里构造，等价于生产环境
    /// Tauri `.setup()` 回调（AppKit 主线程）直接构造 Engine 的失败路径。
    /// `debug_assert!` 应触发并被 `catch_unwind` 接住。
    ///
    /// 仅在 debug 构建有意义（release 下 `debug_assert!` 被消除）。
    #[test]
    #[cfg(debug_assertions)]
    fn engine_new_requires_tokio_runtime() {
        let shared = SharedState::new("test", ArbitrationMode::Priority, || 1000);
        *shared.theme.write().unwrap() = Some(load_default_theme());
        let io = MockIo::new();

        let handle = std::thread::Builder::new()
            .spawn(|| {
                // 双重确认：在新线程里 try_current() 必返回 Err（生产事故路径）。
                assert!(
                    tokio::runtime::Handle::try_current().is_err(),
                    "新线程不应自带 runtime 上下文"
                );
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let _ = Engine::new(shared, io);
                }));
                assert!(
                    outcome.is_err(),
                    "Engine::new 在非 runtime 上下文应触发 debug_assert；\
                     若此处通过，说明 KAD-08 / ADR-0003 的防御被悄悄关掉了。"
                );
            })
            .expect("spawn 测试线程");
        handle.join().expect("join 测试线程");
    }
}
