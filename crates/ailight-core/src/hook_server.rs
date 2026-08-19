//! L1 接入层 HTTP 服务（hook-api V1.0）
//!
//! - `POST /hook`：状态事件上报（source/event/state/session/ts/meta）
//! - `GET /api/status`：业务状态 + 设备状态 + 服务信息快照
//! - `GET /api/health`：健康检查
//! - 仅监听 127.0.0.1；端口 47800 起退避至 47810；可选 Bearer token

use std::net::SocketAddr;
use std::sync::{Arc, RwLock};

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::arbiter::{Arbiter, ArbitrationMode};
use crate::config::DEFAULT_PORT;
use crate::engine;
use crate::protocol::OutputScene;
use crate::theme::ThemeFile;

/// 端口退避上限（hook-api §1）
pub const MAX_PORT: u16 = 47810;

/// 状态名/source 命名约束（hook-api §6）
pub fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

// ---- 对外快照结构（ipc-contract get_app_state / hook-api /api/status） ----

#[derive(Debug, Clone, Serialize, Default)]
pub struct DeviceSnapshot {
    pub connected: bool,
    pub address: Option<String>,
    pub name: Option<String>,
    pub fw_version: Option<String>,
    pub hardware_variant: Option<u8>,
    pub battery_percent: Option<u8>,
    pub power_source: Option<u8>,
    pub charge_state: Option<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BusinessSnapshot {
    pub state: String,
    pub source: Option<String>,
    pub session: Option<String>,
    pub since_ts: u64,
    pub theme: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServiceSnapshot {
    pub version: String,
    pub port: u16,
    pub token_enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatusSnapshot {
    pub service: ServiceSnapshot,
    pub device: DeviceSnapshot,
    pub business: BusinessSnapshot,
}

/// 共享状态（Rust 唯一事实源，KAD-03）
pub struct SharedState {
    pub app_version: String,
    pub arbiter: RwLock<Arbiter>,
    /// 当前生效主题（hold_ms 查询与 SCENE 编译）
    pub theme: RwLock<Option<ThemeFile>>,
    pub theme_name: RwLock<String>,
    pub device: RwLock<DeviceSnapshot>,
    pub token: RwLock<Option<String>>,
    pub port: RwLock<u16>,
    pub now_ms: Box<dyn Fn() -> u64 + Send + Sync>,
    /// 编译后的 SCENE 出站队列（Engine 后台任务消费下发）
    outbound: mpsc::UnboundedSender<OutputScene>,
    outbound_rx_slot: RwLock<Option<mpsc::UnboundedReceiver<OutputScene>>>,
}

impl SharedState {
    pub fn new(
        app_version: &str,
        mode: ArbitrationMode,
        now_ms: impl Fn() -> u64 + Send + Sync + 'static,
    ) -> Arc<Self> {
        let now = now_ms();
        let (outbound_tx, outbound_rx) = mpsc::unbounded_channel();
        Arc::new(Self {
            app_version: app_version.to_string(),
            arbiter: RwLock::new(Arbiter::new(mode, now)),
            theme: RwLock::new(None),
            theme_name: RwLock::new("default".into()),
            device: RwLock::new(DeviceSnapshot::default()),
            token: RwLock::new(None),
            port: RwLock::new(DEFAULT_PORT),
            now_ms: Box::new(now_ms),
            outbound: outbound_tx,
            outbound_rx_slot: RwLock::new(Some(outbound_rx)),
        })
    }

    /// 取出 outbound 消费端（仅一次；由 Engine 调用）
    pub fn outbound_rx(&self) -> mpsc::UnboundedReceiver<OutputScene> {
        self.outbound_rx_slot
            .write()
            .ok()
            .and_then(|mut s| s.take())
            .expect("outbound receiver 已被取出（Engine 只能创建一次）")
    }

    /// 投递一个编译后的 SCENE 到出站队列
    pub fn send_outbound(&self, scene: OutputScene) -> Result<(), String> {
        self.outbound.send(scene).map_err(|_| "outbound 队列已关闭".into())
    }

    /// 由调用方驱动驻留回落（tick），返回是否发生了回落
    pub fn tick(&self) -> Option<crate::arbiter::BusinessState> {
        let now = (self.now_ms)();
        self.arbiter.write().ok()?.tick(now)
    }
}

// ---- hook 请求/响应（hook-api §3） ----

#[derive(Debug, Deserialize)]
pub struct HookRequest {
    pub source: String,
    pub event: String,
    pub state: String,
    #[serde(default)]
    pub session: Option<String>,
    #[serde(default)]
    pub ts: Option<u64>,
    #[serde(default)]
    pub meta: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct HookResponse {
    pub ok: bool,
    pub applied: bool,
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub ok: bool,
    pub code: &'static str,
    pub detail: String,
}

fn err(code: &'static str, detail: impl Into<String>) -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorResponse { ok: false, code, detail: detail.into() }),
    )
}

// ---- 路由 ----

pub fn router(state: Arc<SharedState>) -> Router {
    Router::new()
        .route("/hook", post(hook_handler))
        .route("/api/status", get(status_handler))
        .route("/api/health", get(health_handler))
        .with_state(state)
}

async fn hook_handler(
    State(state): State<Arc<SharedState>>,
    headers: HeaderMap,
    Json(req): Json<HookRequest>,
) -> Result<Json<HookResponse>, (StatusCode, Json<ErrorResponse>)> {
    // token 校验（hook-api §7）
    if let Some(expected) = state.token.read().map(|t| t.clone()).unwrap_or(None) {
        let provided = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .unwrap_or("");
        if provided != expected {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    ok: false,
                    code: "UNAUTHORIZED",
                    detail: "token 不匹配".into(),
                }),
            ));
        }
    }

    // 事件类型：当前仅 state_change（hook-api §3.1，V2 扩展 direct_scene）
    if req.event != "state_change" {
        return Err(err("INVALID_REQUEST", format!("event 不支持: {}", req.event)));
    }
    if !valid_name(&req.source) || !valid_name(&req.state) {
        return Err(err("INVALID_REQUEST", "source/state 命名非法（允许字母数字_-，≤64）"));
    }

    // 仲裁 + 编译 + 入出站队列（engine 后台任务下发）
    let applied = engine::process_event(
        &state,
        &req.source,
        &req.state,
        req.session.as_deref(),
        req.ts,
    )
    .map_err(|e| err("INTERNAL", e.to_string()))?;

    Ok(Json(HookResponse {
        ok: true,
        applied,
        detail: format!(
            "state={} applied={}",
            req.state,
            if applied { "true" } else { "false" }
        ),
    }))
}

async fn status_handler(State(state): State<Arc<SharedState>>) -> Json<StatusSnapshot> {
    let theme_name = state.theme_name.read().map(|t| t.clone()).unwrap_or_default();
    let business = match state.arbiter.read() {
        Ok(guard) => {
            let b = guard.current();
            BusinessSnapshot {
                state: b.state.clone(),
                source: b.source.clone(),
                session: b.session.clone(),
                since_ts: b.since_ms,
                theme: theme_name,
            }
        }
        Err(_) => BusinessSnapshot {
            state: crate::arbiter::ST_IDLE.into(),
            source: None,
            session: None,
            since_ts: 0,
            theme: theme_name,
        },
    };
    let device = state.device.read().map(|d| d.clone()).unwrap_or_default();
    let port = state.port.read().map(|p| *p).unwrap_or(DEFAULT_PORT);
    let token_enabled = state.token.read().map(|t| t.is_some()).unwrap_or(false);
    Json(StatusSnapshot {
        service: ServiceSnapshot {
            version: state.app_version.clone(),
            port,
            token_enabled,
        },
        device,
        business,
    })
}

async fn health_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "ok": true }))
}

/// 启动 HTTP 服务（端口 47800 起退避至 47810，hook-api §1）
///
/// 返回 (实际端口, JoinHandle)。端口耗尽返回 Err。
pub async fn serve(
    state: Arc<SharedState>,
) -> Result<(u16, tokio::task::JoinHandle<()>), String> {
    let app = router(state.clone());
    for port in DEFAULT_PORT..=MAX_PORT {
        let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
        match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => {
                *state.port.write().map_err(|_| "port 锁失败")? = port;
                let handle = tokio::spawn(async move {
                    if let Err(e) = axum::serve(listener, app).await {
                        tracing::error!("hook server 异常退出: {e}");
                    }
                });
                tracing::info!("hook server 监听 127.0.0.1:{port}");
                return Ok((port, handle));
            }
            Err(_) => {
                tracing::warn!("端口 {port} 被占用，尝试下一端口");
                continue;
            }
        }
    }
    Err(format!("端口 {DEFAULT_PORT}~{MAX_PORT} 全部被占用"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arbiter::ArbitrationMode;
    use axum::body::Body;
    use axum::http::{Request, StatusCode as HttpStatus};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn test_state() -> Arc<SharedState> {
        let state = SharedState::new("test", ArbitrationMode::Priority, || 1000);
        // 挂一个最小主题（hold_ms 查询用）
        let theme = serde_json::from_str::<ThemeFile>(
            r#"{"theme":{"name":"t","version":1},
                "scenes":{"s":{"leds":[null,null,null]}},
                "states":{"SUCCESS":{"scene":"s","hold_ms":5000},"WORKING":{"scene":"s"}}}"#,
        )
        .unwrap();
        *state.theme.write().unwrap() = Some(theme);
        state
    }

    async fn post_json(
        app: &Router,
        path: &str,
        body: &str,
        token: Option<&str>,
    ) -> (HttpStatus, serde_json::Value) {
        let mut builder = Request::builder().method("POST").uri(path)
            .header("content-type", "application/json");
        if let Some(t) = token {
            builder = builder.header("authorization", format!("Bearer {t}"));
        }
        let resp = app
            .clone()
            .oneshot(builder.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap();
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(serde_json::json!({})))
    }

    #[tokio::test]
    async fn hook_applies_state() {
        let app = router(test_state());
        let (status, json) = post_json(
            &app,
            "/hook",
            r#"{"source":"claude-code","event":"state_change","state":"WORKING"}"#,
            None,
        )
        .await;
        assert_eq!(status, HttpStatus::OK);
        assert_eq!(json["ok"], true);
        assert_eq!(json["applied"], true);
        // 幂等：相同 source+state 第二次 applied=false
        let (_, json) = post_json(
            &app,
            "/hook",
            r#"{"source":"claude-code","event":"state_change","state":"WORKING"}"#,
            None,
        )
        .await;
        assert_eq!(json["applied"], false);
    }

    #[tokio::test]
    async fn hook_rejects_bad_requests() {
        let app = router(test_state());
        // 坏事件类型
        let (status, _) = post_json(
            &app,
            "/hook",
            r#"{"source":"x","event":"weird","state":"WORKING"}"#,
            None,
        )
        .await;
        assert_eq!(status, HttpStatus::BAD_REQUEST);
        // 非法状态名
        let (status, _) = post_json(
            &app,
            "/hook",
            r#"{"source":"x","event":"state_change","state":"BAD NAME!"}"#,
            None,
        )
        .await;
        assert_eq!(status, HttpStatus::BAD_REQUEST);
        // 缺字段 → axum 默认 Json 反序列化失败返回 422
        let (status, _) = post_json(&app, "/hook", r#"{"source":"x"}"#, None).await;
        assert_eq!(status, HttpStatus::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn token_auth() {
        let state = test_state();
        *state.token.write().unwrap() = Some("secret".into());
        let app = router(state);
        // 无 token → 401
        let (status, _) = post_json(
            &app,
            "/hook",
            r#"{"source":"x","event":"state_change","state":"WORKING"}"#,
            None,
        )
        .await;
        assert_eq!(status, HttpStatus::UNAUTHORIZED);
        // 错误 token → 401
        let (status, _) = post_json(
            &app,
            "/hook",
            r#"{"source":"x","event":"state_change","state":"WORKING"}"#,
            Some("wrong"),
        )
        .await;
        assert_eq!(status, HttpStatus::UNAUTHORIZED);
        // 正确 token → 200
        let (status, json) = post_json(
            &app,
            "/hook",
            r#"{"source":"x","event":"state_change","state":"WORKING"}"#,
            Some("secret"),
        )
        .await;
        assert_eq!(status, HttpStatus::OK);
        assert_eq!(json["applied"], true);
    }

    #[tokio::test]
    async fn status_and_health() {
        let app = router(test_state());
        let resp = app
            .clone()
            .oneshot(Request::builder().uri("/api/status").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), HttpStatus::OK);
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["service"]["version"], "test");
        assert_eq!(json["business"]["state"], "IDLE");

        let resp = app
            .oneshot(Request::builder().uri("/api/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), HttpStatus::OK);
    }

    #[tokio::test]
    async fn port_fallback() {
        // 占用 47800，服务应退避到 47801
        let _blocker = tokio::net::TcpListener::bind("127.0.0.1:47800").await.unwrap();
        let state = test_state();
        let (port, _handle) = serve(state.clone()).await.unwrap();
        assert_eq!(port, 47801);
        // 停掉服务释放端口
        drop(_blocker);
    }

    #[test]
    fn name_validation() {
        assert!(valid_name("claude-code"));
        assert!(valid_name("a_b-1"));
        assert!(!valid_name(""));
        assert!(!valid_name("bad name"));
        assert!(!valid_name(&"x".repeat(65)));
    }
}
