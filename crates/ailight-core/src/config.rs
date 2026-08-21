//! config.json 结构与默认值（ipc-contract §3）

use serde::{Deserialize, Serialize};

use crate::theme;

/// 默认 hook 服务端口
pub const DEFAULT_PORT: u16 = 47800;
/// 端口退避上限（hook-api §1）
pub const MAX_PORT: u16 = 47810;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RememberedDevice {
    pub address: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct AppConfig {
    /// schema 版本，当前 = 1
    pub version: u32,
    /// 仲裁模式："priority" | "last_active"（ADR-0001 Q8）
    #[serde(alias = "arbitration_mode")]
    pub arbitration_mode: String,
    /// 当前生效主题名（默认 "default"）
    #[serde(alias = "active_theme")]
    pub active_theme: String,
    /// hook 服务首选端口；0 = 自动（47800 起退避至 47810）
    #[serde(alias = "port_preference")]
    pub port_preference: u16,
    /// 记住的设备；null = 无
    #[serde(alias = "remembered_device")]
    pub remembered_device: Option<RememberedDevice>,
    /// 空字符串 = 不校验；非空 = 启用 Bearer 校验（hook-api §7）
    pub token: String,
    /// 开机自启（KAD-06 SHOULD）
    pub autostart: bool,
    /// Dashboard 红绿灯徽章朝向："horizontal" | "vertical"
    #[serde(alias = "badge_orientation")]
    pub badge_orientation: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: 1,
            arbitration_mode: "priority".into(),
            active_theme: "default".into(),
            port_preference: DEFAULT_PORT,
            remembered_device: None,
            token: String::new(),
            autostart: false,
            badge_orientation: "horizontal".into(),
        }
    }
}

impl AppConfig {
    /// 从 JSON 加载；非法 JSON/未知字段 → 回退默认值并返回错误说明（KAD-04 容错规则）
    pub fn load(json: &str) -> (Self, Option<String>) {
        match serde_json::from_str::<AppConfig>(json) {
            Ok(mut cfg) => {
                // 字段级容错：非法值回退默认
                let mut warn = Vec::new();
                if cfg.version != 1 {
                    warn.push(format!("version 非法({}), 回退 1", cfg.version));
                    cfg.version = 1;
                }
                if cfg.arbitration_mode != "priority" && cfg.arbitration_mode != "last_active" {
                    warn.push(format!(
                        "arbitration_mode 非法({}), 回退 priority",
                        cfg.arbitration_mode
                    ));
                    cfg.arbitration_mode = "priority".into();
                }
                if cfg.port_preference > MAX_PORT {
                    warn.push(format!(
                        "port_preference 非法({}), 回退 {}",
                        cfg.port_preference, DEFAULT_PORT
                    ));
                    cfg.port_preference = DEFAULT_PORT;
                }
                if cfg.badge_orientation != "horizontal" && cfg.badge_orientation != "vertical" {
                    warn.push(format!(
                        "badge_orientation 非法({}), 回退 horizontal",
                        cfg.badge_orientation
                    ));
                    cfg.badge_orientation = "horizontal".into();
                }
                if !theme::builtin_theme_names().contains(&cfg.active_theme.as_str()) {
                    // 主题名非法或不存在：回退 default（用户主题可能未加载，这里只做静态检查）
                    if cfg.active_theme.is_empty() || cfg.active_theme.len() > 64 {
                        warn.push(format!("active_theme 非法({}), 回退 default", cfg.active_theme));
                        cfg.active_theme = "default".into();
                    }
                }
                let warn = if warn.is_empty() { None } else { Some(warn.join("; ")) };
                (cfg, warn)
            }
            Err(e) => (Self::default(), Some(format!("config 解析失败回退默认: {e}"))),
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.arbitration_mode, "priority");
        assert_eq!(cfg.port_preference, 47800);
        assert!(cfg.remembered_device.is_none());
        assert!(cfg.token.is_empty());
    }

    #[test]
    fn load_valid() {
        let (cfg, warn) = AppConfig::load(
            r#"{"version":1,"arbitration_mode":"last_active","port_preference":47805,
                "remembered_device":{"address":"AA:BB:CC","name":"ACLight-1A2B"},
                "token":"secret","autostart":true}"#,
        );
        assert!(warn.is_none());
        assert_eq!(cfg.arbitration_mode, "last_active");
        assert_eq!(cfg.port_preference, 47805);
        assert_eq!(cfg.remembered_device.unwrap().address, "AA:BB:CC");
        assert_eq!(cfg.token, "secret");
        assert!(cfg.autostart);
    }

    #[test]
    fn load_invalid_falls_back() {
        // 非法 JSON → 全默认
        let (cfg, warn) = AppConfig::load("not json");
        assert!(warn.is_some());
        assert_eq!(cfg, AppConfig::default());
        // 字段非法 → 字段级回退
        let (cfg, warn) = AppConfig::load(
            r#"{"version":9,"arbitration_mode":"weird","port_preference":99999}"#,
        );
        assert!(warn.is_some());
        assert_eq!(cfg.version, 1);
        assert_eq!(cfg.arbitration_mode, "priority");
        assert_eq!(cfg.port_preference, 47800);
    }

    #[test]
    fn roundtrip() {
        let cfg = AppConfig {
            token: "tok".into(),
            ..AppConfig::default()
        };
        let json = cfg.to_json();
        let (loaded, warn) = AppConfig::load(&json);
        assert!(warn.is_none());
        assert_eq!(loaded, cfg);
    }
}
