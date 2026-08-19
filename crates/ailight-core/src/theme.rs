//! 主题加载 / 校验 / SCENE 编译（theme-format V1.0 + ADR-0002）
//!
//! - 加载：serde_json 解析 `.ailight-theme.json`
//! - 校验：整体校验（ADR-0002 T-06）——任一字段非法 → 整个主题拒绝
//! - 编译：状态名 → SCENE JSON → `protocol::OutputScene`（字节级）

use std::collections::HashMap;

use serde::Deserialize;

use crate::protocol::{
    self, BuzzerSegment, BuzzerTrack, LedTrack, OutputScene, Rgb, APPLY_IF_CHANGED,
    CURVE_CONSTANT, CURVE_SAW_DOWN, CURVE_SAW_UP, CURVE_SQUARE, CURVE_TRIANGLE, END_HIGH,
    END_LOW, END_OFF,
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

#[derive(Debug, Clone, Deserialize)]
pub struct ThemeFile {
    pub theme: ThemeMeta,
    pub scenes: HashMap<String, Scene>,
    pub states: HashMap<String, StateConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ThemeMeta {
    pub name: String,
    pub version: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Scene {
    pub leds: Vec<Option<LedTrackDef>>,
    #[serde(default)]
    pub buzzer: Option<BuzzerTrackDef>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LedTrackDef {
    pub curve: String,
    #[serde(default)]
    pub low: Option<String>,
    pub high: String,
    pub brightness: u8,
    #[serde(default)]
    pub period_ms: Option<u16>,
    #[serde(default)]
    pub phase_deg: Option<u16>,
    #[serde(default)]
    pub duty_percent: Option<u8>,
    #[serde(default)]
    pub repeat: Option<u16>,
    #[serde(default)]
    pub end_level: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BuzzerTrackDef {
    #[serde(default)]
    pub start_delay_ms: Option<u16>,
    #[serde(default)]
    pub repeat: Option<u16>,
    pub segments: Vec<BuzzerSegmentDef>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BuzzerSegmentDef {
    pub frequency_hz: u16,
    pub duration_ms: u16,
    pub volume: u8,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StateConfig {
    pub scene: String,
    #[serde(default)]
    pub transition_ms: Option<u16>,
    #[serde(default)]
    pub hold_ms: Option<u64>,
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

fn curve_to_u8(name: &str) -> Option<u8> {
    match name {
        "CONSTANT" => Some(CURVE_CONSTANT),
        "SQUARE" => Some(CURVE_SQUARE),
        "TRIANGLE" => Some(CURVE_TRIANGLE),
        "SAW_UP" => Some(CURVE_SAW_UP),
        "SAW_DOWN" => Some(CURVE_SAW_DOWN),
        _ => None,
    }
}

fn end_level_to_u8(name: &str) -> Option<u8> {
    match name {
        "OFF" => Some(END_OFF),
        "LOW" => Some(END_LOW),
        "HIGH" => Some(END_HIGH),
        _ => None,
    }
}

// ---- 校验（ADR-0002 T-06：整体校验） ----

pub fn validate(theme: &ThemeFile) -> Result<(), ThemeError> {
    if theme.theme.name.is_empty() {
        return Err(ThemeError::Invalid("theme.name 为空".into()));
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
    if led.brightness == 0 || led.brightness > 100 {
        return Err(ThemeError::Invalid(format!(
            "{where_}.brightness 必须在 1~100，实际 {}",
            led.brightness
        )));
    }
    let curve = curve_to_u8(&led.curve).ok_or_else(|| {
        ThemeError::Invalid(format!(
            "{where_}.curve 非法: {}（可选 CONSTANT/SQUARE/TRIANGLE/SAW_UP/SAW_DOWN）",
            led.curve
        ))
    })?;
    if Rgb::from_hex(&led.high).is_none() {
        return Err(ThemeError::Invalid(format!("{where_}.high 颜色非法: {}", led.high)));
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
    if let Some(end) = &led.end_level {
        if end_level_to_u8(end).is_none() {
            return Err(ThemeError::Invalid(format!(
                "{where_}.end_level 非法: {end}（可选 OFF/LOW/HIGH）"
            )));
        }
    }
    Ok(())
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
        if seg.volume == 0 || seg.volume > 100 {
            return Err(ThemeError::Invalid(format!(
                "{where_}.segments[{i}].volume 必须在 1~100，实际 {}",
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
        None => BuzzerTrack { start_delay_ms: 0, repeat_count: 0, segments: vec![] },
    };

    Ok(OutputScene {
        apply_mode: APPLY_IF_CHANGED,
        transition_ms: cfg.transition_ms.unwrap_or(0),
        leds,
        buzzer,
    })
}

fn compile_led(def: &LedTrackDef) -> Result<LedTrack, ThemeError> {
    let curve = curve_to_u8(&def.curve).unwrap(); // validate 已保证
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
        end_level: def
            .end_level
            .as_deref()
            .and_then(end_level_to_u8)
            .unwrap_or(END_OFF),
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
        let theme = load(&builtin(&format!("{THEMES_DIR}/default.ailight-theme.json"))).unwrap();
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
        // CONSTANT 带 period
        let t: ThemeFile = serde_json::from_str(
            r##"{"theme":{"name":"t","version":1},"scenes":{"a":{"leds":[{"curve":"CONSTANT","high":"#FFFFFF","brightness":50,"period_ms":500},null,null]}},"states":{}}"##,
        )
        .unwrap();
        assert!(matches!(validate(&t), Err(ThemeError::Invalid(_))));
        // brightness 0
        let t: ThemeFile = serde_json::from_str(
            r##"{"theme":{"name":"t","version":1},"scenes":{"a":{"leds":[{"curve":"CONSTANT","high":"#FFFFFF","brightness":0},null,null]}},"states":{}}"##,
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
    fn state_not_found() {
        let theme = load(&builtin(&format!("{THEMES_DIR}/default.ailight-theme.json"))).unwrap();
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
