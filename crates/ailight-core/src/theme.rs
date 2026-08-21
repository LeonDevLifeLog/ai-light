//! 主题加载 / 校验 / SCENE 编译（theme-format V1.0 + ADR-0002）
//!
//! - 加载：serde_json 解析 `.ailight-theme.json`
//! - 校验：整体校验（ADR-0002 T-06）——任一字段非法 → 整个主题拒绝
//! - 编译：状态名 → SCENE JSON → `protocol::OutputScene`（字节级）

use std::collections::HashMap;

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::Deserialize;
use serde_json::json;

use crate::protocol::{
    self, BuzzerSegment, BuzzerTrack, LedTrack, OutputScene, Rgb, APPLY_IF_CHANGED, CURVE_CONSTANT,
    CURVE_SAW_DOWN, CURVE_SAW_UP, CURVE_SQUARE, CURVE_TRIANGLE, END_HIGH, END_LOW, END_OFF,
};

/// 主题格式版本
pub const THEME_FORMAT_VERSION: u32 = 1;
/// 状态级 transition_ms 静态上限（运行时以设备能力为准）
pub const MAX_TRANSITION_STATIC_MS: u16 = 2500;

/// 内置主题（编译进二进制，KAD-04）：(文件名, 内容)
/// 路径相对 src/：src → ailight-core → crates → ai-light（../../../docs）
pub const BUILTIN_THEMES: &[(&str, &str)] = &[
    (
        "default",
        include_str!("../../../docs/specs/themes/default.ailight-theme.json"),
    ),
    (
        "minimal",
        include_str!("../../../docs/specs/themes/minimal.ailight-theme.json"),
    ),
    (
        "neon",
        include_str!("../../../docs/specs/themes/neon.ailight-theme.json"),
    ),
    (
        "nature",
        include_str!("../../../docs/specs/themes/nature.ailight-theme.json"),
    ),
    (
        "aurora",
        include_str!("../../../docs/specs/themes/aurora.ailight-theme.json"),
    ),
    (
        "focus",
        include_str!("../../../docs/specs/themes/focus.ailight-theme.json"),
    ),
];

/// 内置主题名列表
pub fn builtin_theme_names() -> Vec<&'static str> {
    BUILTIN_THEMES.iter().map(|(n, _)| *n).collect()
}

/// 加载内置主题（校验通过才返回）
pub fn load_builtin(name: &str) -> Option<ThemeFile> {
    BUILTIN_THEMES
        .iter()
        .find(|(n, _)| *n == name)
        .and_then(|(_, content)| load(content).ok())
}

// ---- 数据结构（theme-format V1.0） ----

/// 可导入、分享和编辑的完整主题文件。
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ThemeFile {
    /// 主题名称与格式版本。
    pub theme: ThemeMeta,
    /// 可复用的命名灯光/声音场景；至少包含一个场景。
    pub scenes: HashMap<String, Scene>,
    /// 业务状态到场景名称的映射。
    pub states: HashMap<String, StateConfig>,
}

/// 主题元信息。
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ThemeMeta {
    /// 主题名称，只允许字母、数字、下划线和连字符。
    #[schemars(
        length(min = 1, max = 64),
        pattern(r"^[A-Za-z0-9_-]+$"),
        extend("examples" = ["aurora"])
    )]
    pub name: String,
    /// 主题格式版本，当前固定为 1。
    #[schemars(extend("const" = 1, "examples" = [1]))]
    pub version: u32,
}

/// 一组可同时执行的三灯轨道与可选蜂鸣轨道。
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Scene {
    /// 顶、中、底三条灯轨；null 表示该灯输出黑色。
    #[schemars(length(equal = 3))]
    pub leds: Vec<Option<LedTrackDef>>,
    /// 可选蜂鸣轨道；省略或 null 表示静音。
    #[serde(default)]
    pub buzzer: Option<BuzzerTrackDef>,
}

/// 灯光曲线。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Curve {
    /// 静态常亮。
    Constant,
    /// 方波闪烁。
    Square,
    /// 三角波呼吸。
    Triangle,
    /// 由暗到亮的锯齿波。
    SawUp,
    /// 由亮到暗的锯齿波。
    SawDown,
}

/// 有限重复结束后的灯轨电平。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EndLevel {
    /// 熄灭。
    Off,
    /// 保持低点颜色。
    Low,
    /// 保持高点颜色。
    High,
}

/// 单条灯轨。其字段约束随 curve 改变，生成的 Schema 使用 oneOf 表达。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LedTrackDef {
    /// 灯光曲线类型。
    pub curve: Curve,
    /// 波形低点颜色，格式为 #RRGGBB。
    #[serde(default)]
    pub low: Option<String>,
    /// 波形高点颜色，格式为 #RRGGBB。
    pub high: String,
    /// 整条灯轨亮度，范围 0~100。
    pub brightness: u8,
    /// 完整波形周期毫秒数；非 CONSTANT 曲线必填且大于 0。
    #[serde(default)]
    pub period_ms: Option<u16>,
    /// 波形相位角，范围 0~360。
    #[serde(default)]
    pub phase_deg: Option<u16>,
    /// 方波高电平占比，SQUARE 曲线必填且范围 1~99。
    #[serde(default)]
    pub duty_percent: Option<u8>,
    /// 有限重复次数；0 或省略表示持续运行。
    #[serde(default)]
    pub repeat: Option<u16>,
    /// 有限重复完成后的输出电平。
    #[serde(default)]
    pub end_level: Option<EndLevel>,
}

/// 蜂鸣器播放轨道。
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BuzzerTrackDef {
    /// 开始播放前的延迟毫秒数。
    #[schemars(range(min = 0, max = 65535), extend("examples" = [120]))]
    #[serde(default)]
    pub start_delay_ms: Option<u16>,
    /// 整组片段重复次数；0 或省略表示持续循环。
    #[schemars(range(min = 0, max = 65535), extend("examples" = [1]))]
    #[serde(default)]
    pub repeat: Option<u16>,
    /// 按顺序播放的蜂鸣片段，数量为 1~16。
    #[schemars(length(min = 1, max = 16))]
    pub segments: Vec<BuzzerSegmentDef>,
}

/// 单个蜂鸣音调或静音间隔。
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BuzzerSegmentDef {
    /// 频率 Hz；0 表示静音间隔，其他值还需满足设备能力范围。
    #[schemars(range(min = 0, max = 65535), extend("examples" = [880]))]
    pub frequency_hz: u16,
    /// 片段持续时间毫秒数，必须大于 0。
    #[schemars(range(min = 1, max = 65535), extend("examples" = [160]))]
    pub duration_ms: u16,
    /// 蜂鸣音量，范围 0~100。
    #[schemars(range(min = 0, max = 100), extend("examples" = [70]))]
    pub volume: u8,
}

/// 单个业务状态的场景映射与切换语义。
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StateConfig {
    /// 引用当前主题 scenes 中存在的场景名称。
    #[schemars(
        length(min = 1, max = 64),
        pattern(r"^[A-Za-z0-9_-]+$"),
        extend("examples" = ["working"])
    )]
    pub scene: String,
    /// 进入状态时的过渡时长，范围 0~2500 毫秒。
    #[schemars(range(min = 0, max = 2500), extend("examples" = [180]))]
    #[serde(default)]
    pub transition_ms: Option<u16>,
    /// 终态驻留时长毫秒数；0 或省略表示不自动回落。
    #[schemars(range(min = 0), extend("examples" = [2000]))]
    #[serde(default)]
    pub hold_ms: Option<u64>,
}

impl JsonSchema for LedTrackDef {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "LedTrackDef".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let end_level = generator.subschema_for::<EndLevel>();
        let color = json!({
            "type": "string",
            "pattern": "^#[0-9A-Fa-f]{6}$",
            "description": "RGB 十六进制颜色，格式为 #RRGGBB。",
            "examples": ["#168CFF"]
        });
        let brightness = json!({
            "type": "integer",
            "minimum": 0,
            "maximum": 100,
            "description": "整条灯轨亮度，范围 0~100。",
            "examples": [60]
        });
        let phase = json!({
            "type": "integer",
            "minimum": 0,
            "maximum": 360,
            "description": "波形相位角，范围 0~360。",
            "examples": [120]
        });
        let repeat = json!({
            "type": "integer",
            "minimum": 0,
            "maximum": 65535,
            "description": "有限重复次数；0 或省略表示持续运行。",
            "examples": [3]
        });

        json!({
            "description": "单条灯轨。不同 curve 使用互斥的字段约束。",
            "oneOf": [
                led_track_variant(
                    json!({"const": "CONSTANT", "description": "静态常亮曲线。", "examples": ["CONSTANT"]}),
                    color.clone(), brightness.clone(), phase.clone(), repeat.clone(),
                    json!({"type": "integer", "const": 0, "description": "CONSTANT 曲线的周期只能省略或为 0。"}),
                    json!({"type": "integer", "const": 0, "description": "CONSTANT 曲线不使用占空比。"}),
                    &["curve", "high", "brightness"],
                    end_level.clone(),
                ),
                led_track_variant(
                    json!({"const": "SQUARE", "description": "方波闪烁曲线。", "examples": ["SQUARE"]}),
                    color.clone(), brightness.clone(), phase.clone(), repeat.clone(),
                    json!({"type": "integer", "minimum": 1, "maximum": 65535, "description": "完整方波周期毫秒数。", "examples": [1000]}),
                    json!({"type": "integer", "minimum": 1, "maximum": 99, "description": "方波高电平占比。", "examples": [50]}),
                    &["curve", "high", "brightness", "period_ms", "duty_percent"],
                    end_level.clone(),
                ),
                led_track_variant(
                    json!({"enum": ["TRIANGLE", "SAW_UP", "SAW_DOWN"], "description": "呼吸、渐亮或渐弱曲线。", "examples": ["TRIANGLE"]}),
                    color, brightness, phase, repeat,
                    json!({"type": "integer", "minimum": 1, "maximum": 65535, "description": "完整波形周期毫秒数。", "examples": [1600]}),
                    json!({"type": "integer", "const": 0, "description": "非 SQUARE 曲线不使用占空比。"}),
                    &["curve", "high", "brightness", "period_ms"],
                    end_level,
                )
            ]
        })
        .try_into()
        .expect("LedTrackDef Schema 必须合法")
    }
}

fn led_track_variant(
    curve: serde_json::Value,
    color: serde_json::Value,
    brightness: serde_json::Value,
    phase: serde_json::Value,
    repeat: serde_json::Value,
    period: serde_json::Value,
    duty: serde_json::Value,
    required: &[&str],
    end_level: Schema,
) -> serde_json::Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": required,
        "properties": {
            "curve": curve,
            "low": color.clone(),
            "high": color,
            "brightness": brightness,
            "period_ms": period,
            "phase_deg": phase,
            "duty_percent": duty,
            "repeat": repeat,
            "end_level": {
                "allOf": [end_level],
                "description": "有限重复完成后的输出电平。",
                "examples": ["OFF"]
            }
        }
    })
}

/// 从运行时 Theme DTO 生成 JSON Schema Draft 2020-12。
pub fn theme_schema_value() -> serde_json::Value {
    let schema = schemars::schema_for!(ThemeFile);
    let mut value = serde_json::to_value(schema).expect("Theme Schema 必须可序列化");
    let root = value
        .as_object_mut()
        .expect("Theme Schema 根节点必须是对象");
    root.insert(
        "$id".into(),
        json!("https://ai-light.local/schemas/theme-v1.schema.json"),
    );
    root.insert("title".into(), json!("AI-Light Theme"));
    root.insert(
        "description".into(),
        json!("AI-Light 主题文件格式 V1：状态到灯光与声音场景的映射。"),
    );

    let name_schema = json!({
        "type": "string",
        "minLength": 1,
        "maxLength": 64,
        "pattern": "^[A-Za-z0-9_-]+$"
    });
    let properties = root
        .get_mut("properties")
        .and_then(serde_json::Value::as_object_mut)
        .expect("Theme Schema 必须包含 properties");
    let scenes = properties
        .get_mut("scenes")
        .and_then(serde_json::Value::as_object_mut)
        .expect("Theme Schema 必须包含 scenes");
    scenes.insert("minProperties".into(), json!(1));
    scenes.insert("propertyNames".into(), name_schema.clone());
    let states = properties
        .get_mut("states")
        .and_then(serde_json::Value::as_object_mut)
        .expect("Theme Schema 必须包含 states");
    states.insert("propertyNames".into(), name_schema);

    value
}

/// 生成适合提交和分发的稳定格式 JSON Schema 文本。
pub fn theme_schema_pretty() -> String {
    let mut output =
        serde_json::to_string_pretty(&theme_schema_value()).expect("Theme Schema 必须可格式化");
    output.push('\n');
    output
}

// ---- 错误 ----

#[derive(Debug, Clone, PartialEq)]
pub enum ThemeError {
    /// JSON 解析失败
    Parse(String),
    /// 校验失败（含原因）
    Invalid(String),
    /// 状态未映射（theme-format §3：未映射 → IDLE 兜底由调用方处理）
    StateNotFound(String),
}

impl std::fmt::Display for ThemeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ThemeError::Parse(e) => write!(f, "主题 JSON 解析失败: {e}"),
            ThemeError::Invalid(e) => write!(f, "主题校验失败: {e}"),
            ThemeError::StateNotFound(s) => write!(f, "状态未映射: {s}"),
        }
    }
}

impl std::error::Error for ThemeError {}

// ---- 曲线/终态映射 ----

fn curve_to_u8(curve: Curve) -> u8 {
    match curve {
        Curve::Constant => CURVE_CONSTANT,
        Curve::Square => CURVE_SQUARE,
        Curve::Triangle => CURVE_TRIANGLE,
        Curve::SawUp => CURVE_SAW_UP,
        Curve::SawDown => CURVE_SAW_DOWN,
    }
}

fn end_level_to_u8(level: EndLevel) -> u8 {
    match level {
        EndLevel::Off => END_OFF,
        EndLevel::Low => END_LOW,
        EndLevel::High => END_HIGH,
    }
}

// ---- 校验（ADR-0002 T-06：整体校验） ----

pub fn validate(theme: &ThemeFile) -> Result<(), ThemeError> {
    if !valid_name(&theme.theme.name) {
        return Err(ThemeError::Invalid(
            "theme.name 命名非法（允许字母数字_-，≤64）".into(),
        ));
    }
    if theme.theme.version != THEME_FORMAT_VERSION {
        return Err(ThemeError::Invalid(format!(
            "theme.version 必须为 {THEME_FORMAT_VERSION}，实际 {}",
            theme.theme.version
        )));
    }
    if theme.scenes.is_empty() {
        return Err(ThemeError::Invalid("scenes 不能为空".into()));
    }
    for (name, scene) in &theme.scenes {
        if !valid_name(name) {
            return Err(ThemeError::Invalid(format!(
                "scene 名命名非法（允许字母数字_-，≤64）: {name}"
            )));
        }
        if scene.leds.len() != 3 {
            return Err(ThemeError::Invalid(format!(
                "scene[{name}].leds 长度必须为 3，实际 {}",
                scene.leds.len()
            )));
        }
        for (i, led) in scene.leds.iter().enumerate() {
            if let Some(led) = led {
                validate_led(name, i, led)?;
            }
        }
        if let Some(buz) = &scene.buzzer {
            validate_buzzer(name, buz)?;
        }
    }
    for (state, cfg) in &theme.states {
        if !valid_name(state) {
            return Err(ThemeError::Invalid(format!(
                "state 名命名非法（允许字母数字_-，≤64）: {state}"
            )));
        }
        if !theme.scenes.contains_key(&cfg.scene) {
            return Err(ThemeError::Invalid(format!(
                "states[{state}].scene 引用不存在的 SCENE: {}",
                cfg.scene
            )));
        }
        if let Some(t) = cfg.transition_ms {
            if t > MAX_TRANSITION_STATIC_MS {
                return Err(ThemeError::Invalid(format!(
                    "states[{state}].transition_ms 超过静态上限 {MAX_TRANSITION_STATIC_MS}"
                )));
            }
        }
    }
    Ok(())
}

fn validate_led(scene: &str, idx: usize, led: &LedTrackDef) -> Result<(), ThemeError> {
    let where_ = format!("scene[{scene}].leds[{idx}]");
    if led.brightness > 100 {
        return Err(ThemeError::Invalid(format!(
            "{where_}.brightness 必须在 0~100，实际 {}",
            led.brightness
        )));
    }
    let curve = curve_to_u8(led.curve);
    if Rgb::from_hex(&led.high).is_none() {
        return Err(ThemeError::Invalid(format!(
            "{where_}.high 颜色非法: {}",
            led.high
        )));
    }
    if let Some(low) = &led.low {
        if Rgb::from_hex(low).is_none() {
            return Err(ThemeError::Invalid(format!("{where_}.low 颜色非法: {low}")));
        }
    }
    match curve {
        CURVE_CONSTANT => {
            // CONSTANT 只允许 high+brightness；period/phase/duty/repeat 须省略或 0
            for (field, v) in [
                ("period_ms", led.period_ms.map(|v| v as u64)),
                ("phase_deg", led.phase_deg.map(|v| v as u64)),
                ("duty_percent", led.duty_percent.map(|v| v as u64)),
                ("repeat", led.repeat.map(|v| v as u64)),
            ] {
                if let Some(v) = v {
                    if v != 0 {
                        return Err(ThemeError::Invalid(format!(
                            "{where_}.{field} 在 CONSTANT 下必须省略或为 0"
                        )));
                    }
                }
            }
        }
        CURVE_SQUARE => {
            let period = led.period_ms.ok_or_else(|| {
                ThemeError::Invalid(format!("{where_}.period_ms 必填（非 CONSTANT 曲线）"))
            })?;
            if period == 0 {
                return Err(ThemeError::Invalid(format!(
                    "{where_}.period_ms 必须 > 0（非 CONSTANT 曲线）"
                )));
            }
            let duty = led.duty_percent.ok_or_else(|| {
                ThemeError::Invalid(format!("{where_}.duty_percent 在 SQUARE 下必填"))
            })?;
            if !(1..=99).contains(&duty) {
                return Err(ThemeError::Invalid(format!(
                    "{where_}.duty_percent 必须在 1~99，实际 {duty}"
                )));
            }
        }
        _ => {
            // 其他曲线：period > 0
            let period = led.period_ms.ok_or_else(|| {
                ThemeError::Invalid(format!("{where_}.period_ms 必填（非 CONSTANT 曲线）"))
            })?;
            if period == 0 {
                return Err(ThemeError::Invalid(format!(
                    "{where_}.period_ms 必须 > 0（非 CONSTANT 曲线）"
                )));
            }
            // 非 SQUARE 不得出现 duty
            if let Some(d) = led.duty_percent {
                if d != 0 {
                    return Err(ThemeError::Invalid(format!(
                        "{where_}.duty_percent 仅 SQUARE 有效"
                    )));
                }
            }
        }
    }
    if let Some(deg) = led.phase_deg {
        if deg > 360 {
            return Err(ThemeError::Invalid(format!(
                "{where_}.phase_deg 必须在 0~360，实际 {deg}"
            )));
        }
    }
    Ok(())
}

fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn validate_buzzer(scene: &str, buz: &BuzzerTrackDef) -> Result<(), ThemeError> {
    let where_ = format!("scene[{scene}].buzzer");
    if buz.segments.is_empty() || buz.segments.len() > 16 {
        return Err(ThemeError::Invalid(format!(
            "{where_}.segments 必须在 1~16 条，实际 {}",
            buz.segments.len()
        )));
    }
    for (i, seg) in buz.segments.iter().enumerate() {
        if seg.duration_ms == 0 {
            return Err(ThemeError::Invalid(format!(
                "{where_}.segments[{i}].duration_ms 必须 > 0"
            )));
        }
        if seg.volume > 100 {
            return Err(ThemeError::Invalid(format!(
                "{where_}.segments[{i}].volume 必须在 0~100，实际 {}",
                seg.volume
            )));
        }
    }
    Ok(())
}

// ---- 编译 ----

/// 加载并校验主题（JSON 字符串 → ThemeFile）
pub fn load(content: &str) -> Result<ThemeFile, ThemeError> {
    let theme: ThemeFile =
        serde_json::from_str(content).map_err(|e| ThemeError::Parse(e.to_string()))?;
    validate(&theme)?;
    Ok(theme)
}

/// 将指定状态编译为协议 SCENE
///
/// 未映射状态返回 `StateNotFound`（调用方按 IDLE 兜底 + 记日志，theme-format §3）
pub fn compile_state(theme: &ThemeFile, state: &str) -> Result<OutputScene, ThemeError> {
    let cfg = theme
        .states
        .get(state)
        .ok_or_else(|| ThemeError::StateNotFound(state.to_string()))?;
    let scene = theme
        .scenes
        .get(&cfg.scene)
        .ok_or_else(|| ThemeError::Invalid(format!("states[{state}] 引用不存在的 SCENE")))?;

    let mut leds = [LedTrack::default(); 3];
    for (i, led_def) in scene.leds.iter().enumerate() {
        if let Some(def) = led_def {
            leds[i] = compile_led(def)?;
        }
        // None → 默认 LedTrack（CONSTANT 黑）→ 该灯不输出
    }

    let buzzer = match &scene.buzzer {
        Some(b) => BuzzerTrack {
            start_delay_ms: b.start_delay_ms.unwrap_or(0),
            repeat_count: b.repeat.unwrap_or(0),
            segments: b
                .segments
                .iter()
                .map(|s| BuzzerSegment {
                    frequency_hz: s.frequency_hz,
                    duration_ms: s.duration_ms,
                    volume: s.volume,
                })
                .collect(),
        },
        None => BuzzerTrack {
            start_delay_ms: 0,
            repeat_count: 0,
            segments: vec![],
        },
    };

    Ok(OutputScene {
        apply_mode: APPLY_IF_CHANGED,
        transition_ms: cfg.transition_ms.unwrap_or(0),
        leds,
        buzzer,
    })
}

fn compile_led(def: &LedTrackDef) -> Result<LedTrack, ThemeError> {
    let curve = curve_to_u8(def.curve);
    let high = Rgb::from_hex(&def.high).unwrap();
    let low = match &def.low {
        Some(h) => Rgb::from_hex(h).unwrap(),
        None => Rgb(0, 0, 0),
    };
    let phase = def.phase_deg.map(protocol::phase_from_deg).unwrap_or(0);
    Ok(LedTrack {
        curve,
        low,
        high,
        brightness: def.brightness,
        period_ms: def.period_ms.unwrap_or(0),
        phase,
        duty_percent: def.duty_percent.unwrap_or(0),
        repeat_count: def.repeat.unwrap_or(0),
        end_level: def.end_level.map(end_level_to_u8).unwrap_or(END_OFF),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 内置主题文件（测试夹具，同时验证 6 套主题全部合法）
    fn builtin(path: &str) -> String {
        std::fs::read_to_string(path).expect("读取内置主题失败")
    }

    const THEMES_DIR: &str = "../../docs/specs/themes";

    #[test]
    fn all_builtin_themes_valid() {
        assert_eq!(builtin_theme_names().len(), 6);
        for (name, content) in BUILTIN_THEMES {
            let theme = load(content).unwrap_or_else(|e| panic!("主题 {name} 校验失败: {e}"));
            // 5 态映射齐全
            for st in ["IDLE", "WORKING", "WAITING", "SUCCESS", "ERROR"] {
                // IDLE 可省略（内置熄灭）；其余必须映射
                if st != "IDLE" {
                    assert!(theme.states.contains_key(st), "{name} 缺少状态 {st}");
                }
            }
        }
        // load_builtin 存在性
        assert!(load_builtin("default").is_some());
        assert!(load_builtin("nope").is_none());
    }

    #[test]
    fn compile_default_states() {
        let theme = load(&builtin(&format!(
            "{THEMES_DIR}/default.ailight-theme.json"
        )))
        .unwrap();
        // WORKING：三灯 TRIANGLE 呼吸
        let scene = compile_state(&theme, "WORKING").unwrap();
        assert_eq!(scene.transition_ms, 300);
        for led in &scene.leds {
            assert_eq!(led.curve, CURVE_TRIANGLE);
            assert_eq!(led.period_ms, 1200);
        }
        assert!(scene.buzzer.segments.is_empty());
        // ERROR：SQUARE 闪 8 次 + 蜂鸣一声 + hold 0
        let scene = compile_state(&theme, "ERROR").unwrap();
        assert_eq!(scene.transition_ms, 0);
        for led in &scene.leds {
            assert_eq!(led.curve, CURVE_SQUARE);
            assert_eq!(led.repeat_count, 8);
            assert_eq!(led.end_level, END_HIGH);
        }
        assert_eq!(scene.buzzer.segments.len(), 1);
        // SUCCESS：CONSTANT 绿 + hold 5000
        let scene = compile_state(&theme, "SUCCESS").unwrap();
        assert_eq!(scene.leds[0].curve, CURVE_CONSTANT);
        assert_eq!(scene.leds[0].high, Rgb(0x00, 0xE6, 0x76));
        assert_eq!(theme.states["SUCCESS"].hold_ms, Some(5000));
    }

    #[test]
    fn compile_focus_single_led() {
        let theme = load(&builtin(&format!("{THEMES_DIR}/focus.ailight-theme.json"))).unwrap();
        let scene = compile_state(&theme, "WORKING").unwrap();
        // 顶/底为 None → 黑；中间呼吸
        assert_eq!(scene.leds[0].high, Rgb(0, 0, 0));
        assert_eq!(scene.leds[2].high, Rgb(0, 0, 0));
        assert_eq!(scene.leds[1].curve, CURVE_TRIANGLE);
    }

    #[test]
    fn compile_neon_sweep() {
        let theme = load(&builtin(&format!("{THEMES_DIR}/neon.ailight-theme.json"))).unwrap();
        let scene = compile_state(&theme, "WORKING").unwrap();
        // SAW_UP 扫光跑马：三灯相位 0/120/240
        assert_eq!(scene.leds[0].curve, CURVE_SAW_UP);
        assert_eq!(scene.leds[0].phase, 0x0000);
        assert_eq!(scene.leds[1].phase, 0x5555);
        assert_eq!(scene.leds[2].phase, 0xAAAA);
    }

    #[test]
    fn invalid_theme_rejected() {
        // 坏 JSON
        assert!(matches!(load("{not json"), Err(ThemeError::Parse(_))));
        // 引用缺失
        let t: ThemeFile = serde_json::from_str(
            r#"{"theme":{"name":"t","version":1},"scenes":{"a":{"leds":[null,null,null]}},"states":{"WORKING":{"scene":"missing"}}}"#,
        )
        .unwrap();
        assert!(matches!(validate(&t), Err(ThemeError::Invalid(_))));
        // 未知字段由 serde 整体拒绝（与 JSON Schema additionalProperties=false 一致）
        assert!(matches!(
            load(r#"{"theme":{"name":"t","version":1,"extra":true},"scenes":{},"states":{}}"#),
            Err(ThemeError::Parse(_))
        ));
        // SQUARE 与其他非静态波形一样必须提供正周期
        let t: ThemeFile = serde_json::from_str(
            r##"{"theme":{"name":"t","version":1},"scenes":{"a":{"leds":[{"curve":"SQUARE","high":"#FFFFFF","brightness":50,"duty_percent":50},null,null]}},"states":{}}"##,
        )
        .unwrap();
        assert!(matches!(validate(&t), Err(ThemeError::Invalid(_))));
        // CONSTANT 带 period
        let t: ThemeFile = serde_json::from_str(
            r##"{"theme":{"name":"t","version":1},"scenes":{"a":{"leds":[{"curve":"CONSTANT","high":"#FFFFFF","brightness":50,"period_ms":500},null,null]}},"states":{}}"##,
        )
        .unwrap();
        assert!(matches!(validate(&t), Err(ThemeError::Invalid(_))));
        // brightness 0 = 全黑，合法
        let t: ThemeFile = serde_json::from_str(
            r##"{"theme":{"name":"t","version":1},"scenes":{"a":{"leds":[{"curve":"CONSTANT","high":"#FFFFFF","brightness":0},null,null]}},"states":{}}"##,
        )
        .unwrap();
        assert!(validate(&t).is_ok());
        // 静音音量 0 合法
        let t: ThemeFile = serde_json::from_str(
            r##"{"theme":{"name":"t","version":1},"scenes":{"a":{"leds":[null,null,null],"buzzer":{"segments":[{"frequency_hz":0,"duration_ms":100,"volume":0}]}}},"states":{}}"##,
        )
        .unwrap();
        assert!(validate(&t).is_ok());
        // brightness / volume 上界为 100
        let t: ThemeFile = serde_json::from_str(
            r##"{"theme":{"name":"t","version":1},"scenes":{"a":{"leds":[{"curve":"CONSTANT","high":"#FFFFFF","brightness":101},null,null]}},"states":{}}"##,
        )
        .unwrap();
        assert!(matches!(validate(&t), Err(ThemeError::Invalid(_))));
        let t: ThemeFile = serde_json::from_str(
            r##"{"theme":{"name":"t","version":1},"scenes":{"a":{"leds":[null,null,null],"buzzer":{"segments":[{"frequency_hz":1000,"duration_ms":100,"volume":101}]}}},"states":{}}"##,
        )
        .unwrap();
        assert!(matches!(validate(&t), Err(ThemeError::Invalid(_))));
        // leds 长度错误
        let t: ThemeFile = serde_json::from_str(
            r#"{"theme":{"name":"t","version":1},"scenes":{"a":{"leds":[null]}},"states":{}}"#,
        )
        .unwrap();
        assert!(matches!(validate(&t), Err(ThemeError::Invalid(_))));
    }

    #[test]
    fn builtin_themes_match_json_schema() {
        let schema: serde_json::Value =
            serde_json::from_str(include_str!("../../../docs/specs/theme.schema.json")).unwrap();
        let validator = jsonschema::validator_for(&schema).unwrap();

        for (name, content) in BUILTIN_THEMES {
            let instance: serde_json::Value = serde_json::from_str(content).unwrap();
            if let Err(error) = validator.validate(&instance) {
                panic!("内置主题 {name} 不符合 JSON Schema: {error}");
            }
        }
    }

    #[test]
    fn checked_in_json_schema_matches_dtos() {
        let checked_in: serde_json::Value =
            serde_json::from_str(include_str!("../../../docs/specs/theme.schema.json")).unwrap();
        assert_eq!(
            checked_in,
            theme_schema_value(),
            "Theme DTO 已变化；运行 cargo run --example generate_theme_schema 更新 Schema"
        );
    }

    #[test]
    fn state_not_found() {
        let theme = load(&builtin(&format!(
            "{THEMES_DIR}/default.ailight-theme.json"
        )))
        .unwrap();
        assert!(matches!(
            compile_state(&theme, "NOPE"),
            Err(ThemeError::StateNotFound(_))
        ));
    }

    #[test]
    fn hex_parse() {
        assert_eq!(Rgb::from_hex("#00E676"), Some(Rgb(0x00, 0xE6, 0x76)));
        assert_eq!(Rgb::from_hex("FF0000"), Some(Rgb(0xFF, 0, 0)));
        assert_eq!(Rgb::from_hex("#FFF"), None);
        assert_eq!(Rgb::from_hex("GGGGGG"), None);
    }
}
