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
use utoipa::{Modify, OpenApi, ToSchema};
use utoipa_swagger_ui::SwaggerUi;

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

#[derive(Debug, Clone, Serialize, Default, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeviceSnapshot {
    /// 当前是否已经连接到状态灯。
    #[schema(example = true)]
    pub connected: bool,
    /// 设备的系统蓝牙地址；未连接时为 null。
    #[schema(example = "AA:BB:CC:DD:EE:FF")]
    pub address: Option<String>,
    /// 蓝牙广播名称；未连接或设备未提供时为 null。
    #[schema(example = "StatusLight-1A2B")]
    pub name: Option<String>,
    /// 设备固件版本；设备未提供时为 null。
    #[schema(example = "1.0.0")]
    pub fw_version: Option<String>,
    /// 硬件变体编号；设备未提供时为 null。
    #[schema(example = 1)]
    pub hardware_variant: Option<u8>,
    /// 剩余电量百分比，范围 0~100；无电池能力时为 null。
    #[schema(example = 75, minimum = 0, maximum = 100)]
    pub battery_percent: Option<u8>,
    /// 电源来源协议枚举值；设备未提供时为 null。
    #[schema(example = 1)]
    pub power_source: Option<u8>,
    /// 充电状态协议枚举值；设备未提供时为 null。
    #[schema(example = 0)]
    pub charge_state: Option<u8>,
    /// 电源标志位协议枚举值；设备未提供时为 null。
    #[schema(example = 7)]
    pub power_flags: Option<u16>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BusinessSnapshot {
    /// 当前仲裁生效的标准状态或自定义状态。
    #[schema(example = "WORKING")]
    pub state: String,
    /// 当前状态的事件来源；空闲且无来源时为 null。
    #[schema(example = "codex")]
    pub source: Option<String>,
    /// 调用方会话标识；未提供时为 null。
    #[schema(example = "task-001")]
    pub session: Option<String>,
    /// 当前状态开始生效的 Unix 毫秒时间戳。
    #[schema(example = 1724040000000_u64, minimum = 0)]
    pub since_ts: u64,
    /// 当前生效的主题名称。
    #[schema(example = "default")]
    pub theme: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ServiceSnapshot {
    /// AI-Light 应用版本。
    #[schema(example = "0.1.1")]
    pub version: String,
    /// Hook Server 实际监听端口。
    #[schema(example = 47800, minimum = 47800, maximum = 47810)]
    pub port: u16,
    /// 是否已经启用 Bearer Token 校验。
    #[schema(example = false)]
    pub token_enabled: bool,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct StatusSnapshot {
    /// Hook Server 自身信息。
    pub service: ServiceSnapshot,
    /// 当前设备连接与能力快照。
    pub device: DeviceSnapshot,
    /// 当前业务状态仲裁结果。
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
        self.outbound
            .send(scene)
            .map_err(|_| "outbound 队列已关闭".into())
    }

    /// 由调用方驱动驻留回落（tick），返回是否发生了回落
    pub fn tick(&self) -> Option<crate::arbiter::BusinessState> {
        let now = (self.now_ms)();
        self.arbiter.write().ok()?.tick(now)
    }
}

// ---- hook 请求/响应（hook-api §3） ----

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct HookRequest {
    /// 事件来源标识，如 codex、claude-code 或 manual。
    #[schema(
        example = "codex",
        min_length = 1,
        max_length = 64,
        pattern = "^[A-Za-z0-9_-]+$"
    )]
    pub source: String,
    /// 事件类型。V1 仅支持 state_change。
    #[schema(example = "state_change")]
    pub event: String,
    /// 标准状态或当前主题定义的自定义状态。
    #[schema(
        example = "WORKING",
        min_length = 1,
        max_length = 64,
        pattern = "^[A-Za-z0-9_-]+$"
    )]
    pub state: String,
    /// 调用方会话标识，仅用于追踪和展示。
    #[schema(example = "task-001")]
    #[serde(default)]
    pub session: Option<String>,
    /// Unix 毫秒时间戳；省略时使用服务端接收时间。
    #[schema(example = 1724040000000_u64, minimum = 0)]
    #[serde(default)]
    pub ts: Option<u64>,
    /// 任意 JSON 对象形式的透传元数据，不参与状态仲裁。
    #[schema(value_type = Object, example = json!({"detail": "running tests", "progress": 60}))]
    #[serde(default)]
    pub meta: Option<serde_json::Map<String, serde_json::Value>>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct HookResponse {
    /// 请求是否成功受理；成功响应固定为 true。
    #[schema(example = true)]
    pub ok: bool,
    /// 是否实际改变了灯效；false 表示被幂等去重。
    #[schema(example = true)]
    pub applied: bool,
    /// 便于人工排障的处理结果说明。
    #[schema(example = "state=WORKING applied=true")]
    pub detail: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorResponse {
    /// 错误响应固定为 false。
    #[schema(example = false)]
    pub ok: bool,
    /// 稳定的机器可读错误码。
    #[schema(example = "INVALID_REQUEST")]
    pub code: &'static str,
    /// 面向开发者的错误原因。
    #[schema(example = "event 不支持: unknown")]
    pub detail: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct HealthResponse {
    /// Hook Server 是否正常运行；成功响应固定为 true。
    #[schema(example = true)]
    pub ok: bool,
    /// AI-Light 应用版本。
    #[schema(example = "0.1.1")]
    pub version: String,
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "AI-Light Hook API",
        version = "1.0.0",
        description = "本机智能体状态上报、状态查询与健康检查接口。服务仅监听回环地址。"
    ),
    paths(hook_handler, status_handler, health_handler),
    components(schemas(
        HookRequest,
        HookResponse,
        ErrorResponse,
        StatusSnapshot,
        ServiceSnapshot,
        DeviceSnapshot,
        BusinessSnapshot,
        HealthResponse
    )),
    modifiers(&ApiMetadata),
    tags(
        (name = "Events", description = "智能体生命周期事件"),
        (name = "Service", description = "服务状态与诊断")
    )
)]
struct ApiDoc;

struct ApiMetadata;

impl Modify for ApiMetadata {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};

        openapi.servers = Some(vec![utoipa::openapi::ServerBuilder::new()
            .url("/")
            .description(Some("当前 Hook Server；自动适配实际监听端口"))
            .build()]);
        openapi
            .components
            .get_or_insert_default()
            .add_security_scheme(
                "bearerAuth",
                SecurityScheme::Http(
                    HttpBuilder::new()
                        .scheme(HttpAuthScheme::Bearer)
                        .description(Some("仅在应用设置中启用接入密码后需要"))
                        .build(),
                ),
            );
    }
}

fn err(code: &'static str, detail: impl Into<String>) -> (StatusCode, Json<ErrorResponse>) {
    (
        if code == "INTERNAL_ERROR" {
            StatusCode::INTERNAL_SERVER_ERROR
        } else {
            StatusCode::BAD_REQUEST
        },
        Json(ErrorResponse {
            ok: false,
            code,
            detail: detail.into(),
        }),
    )
}

// ---- 路由 ----

pub fn router(state: Arc<SharedState>) -> Router {
    let openapi = ApiDoc::openapi();
    let docs: Router<Arc<SharedState>> =
        SwaggerUi::new("/docs").url("/openapi.json", openapi).into();

    Router::new()
        .route("/hook", post(hook_handler))
        .route("/api/status", get(status_handler))
        .route("/api/health", get(health_handler))
        .merge(docs)
        .with_state(state)
}

#[utoipa::path(
    post,
    path = "/hook",
    tag = "Events",
    request_body(content = HookRequest, description = "智能体状态变化事件", content_type = "application/json"),
    responses(
        (status = 200, description = "事件已受理", body = HookResponse),
        (status = 400, description = "事件类型或名称格式非法", body = ErrorResponse),
        (status = 401, description = "Bearer Token 缺失或不匹配", body = ErrorResponse),
        (status = 422, description = "JSON 缺少必填字段、字段类型错误或包含未知字段"),
        (status = 500, description = "事件处理或场景编译失败", body = ErrorResponse)
    ),
    security((), ("bearerAuth" = []))
)]
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
        return Err(err(
            "INVALID_REQUEST",
            format!("event 不支持: {}", req.event),
        ));
    }
    if !valid_name(&req.source) || !valid_name(&req.state) {
        return Err(err(
            "INVALID_REQUEST",
            "source/state 命名非法（允许字母数字_-，≤64）",
        ));
    }

    // 仲裁 + 编译 + 入出站队列（engine 后台任务下发）
    let applied = engine::process_event(
        &state,
        &req.source,
        &req.state,
        req.session.as_deref(),
        req.ts,
    )
    .map_err(|e| err("INTERNAL_ERROR", e.to_string()))?;

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

#[utoipa::path(
    get,
    path = "/api/status",
    tag = "Service",
    responses((status = 200, description = "服务、设备及当前业务状态", body = StatusSnapshot))
)]
async fn status_handler(State(state): State<Arc<SharedState>>) -> Json<StatusSnapshot> {
    let theme_name = state
        .theme_name
        .read()
        .map(|t| t.clone())
        .unwrap_or_default();
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

#[utoipa::path(
    get,
    path = "/api/health",
    tag = "Service",
    responses((status = 200, description = "Hook Server 正常运行", body = HealthResponse))
)]
async fn health_handler(State(state): State<Arc<SharedState>>) -> Json<HealthResponse> {
    Json(HealthResponse {
        ok: true,
        version: state.app_version.clone(),
    })
}

/// 启动 HTTP 服务（端口 47800 起退避至 47810，hook-api §1）
///
/// 返回 (实际端口, JoinHandle)。端口耗尽返回 Err。
pub async fn serve(state: Arc<SharedState>) -> Result<(u16, tokio::task::JoinHandle<()>), String> {
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
        let mut builder = Request::builder()
            .method("POST")
            .uri(path)
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
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::json!({})),
        )
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
            .oneshot(
                Request::builder()
                    .uri("/api/status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), HttpStatus::OK);
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["service"]["version"], "test");
        assert_eq!(json["business"]["state"], "IDLE");

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), HttpStatus::OK);
    }

    #[tokio::test]
    async fn serves_openapi_and_swagger_ui() {
        let app = router(test_state());
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/openapi.json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), HttpStatus::OK);
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "application/json"
        );
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let spec: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(spec["openapi"], "3.1.0");
        for path in ["/hook", "/api/status", "/api/health"] {
            assert!(spec["paths"].get(path).is_some(), "OpenAPI 缺少 {path}");
        }
        assert_eq!(
            spec["components"]["securitySchemes"]["bearerAuth"]["scheme"],
            "bearer"
        );
        assert_eq!(spec["servers"][0]["url"], "/");

        let hook = &spec["components"]["schemas"]["HookRequest"];
        assert!(hook["properties"]["source"]["description"]
            .as_str()
            .is_some_and(|value| !value.is_empty()));
        assert_eq!(hook["properties"]["source"]["pattern"], "^[A-Za-z0-9_-]+$");
        assert_eq!(hook["properties"]["source"]["maxLength"], 64);

        let service = &spec["components"]["schemas"]["ServiceSnapshot"];
        assert!(service["properties"].get("tokenEnabled").is_some());
        assert!(service["properties"].get("token_enabled").is_none());
        assert!(service["properties"]["tokenEnabled"]["description"]
            .as_str()
            .is_some_and(|value| !value.is_empty()));

        let health = &spec["components"]["schemas"]["HealthResponse"];
        for field in ["ok", "version"] {
            assert!(health["properties"][field]["description"]
                .as_str()
                .is_some_and(|value| !value.is_empty()));
        }

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/docs/")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), HttpStatus::OK);
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let html = std::str::from_utf8(&bytes).unwrap();
        assert!(!html.contains("unpkg.com"));

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/docs/swagger-initializer.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), HttpStatus::OK);
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let initializer = std::str::from_utf8(&bytes).unwrap();
        assert!(initializer.contains("/openapi.json"));
        assert!(!initializer.contains("unpkg.com"));

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/docs/swagger-ui.css")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), HttpStatus::OK);
        assert!(resp.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/css"));
    }

    #[tokio::test]
    async fn port_fallback() {
        // 占用 47800，服务应退避到 47801
        let _blocker = tokio::net::TcpListener::bind("127.0.0.1:47800")
            .await
            .unwrap();
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
