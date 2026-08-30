//! toolchain.json 原子持久化（设计方案 §5.1 / §5.2）
//!
//! 位置 `~/.ailight/toolchain.json`（`AILIGHT_HOME` 可覆盖）；独立于 config.json，
//! 因为工具链包含平台路径、探测来源和诊断缓存，不属于主题/显示/设备偏好。

use std::path::{Path, PathBuf};

use super::model::{ToolchainDocument, TOOLCHAIN_SCHEMA_VERSION};

/// toolchain.json 路径
pub fn toolchain_path() -> Result<PathBuf, String> {
    Ok(crate::storage::ai_light_home()?.join("toolchain.json"))
}

/// 当前时刻的 RFC3339 UTC 时间戳（如 `2026-08-30T10:00:00Z`）。
/// 独立实现避免为单一字段引入 chrono/time 依赖（Gregorian 日数算法，见测试）。
pub fn rfc3339_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    rfc3339_from_unix(now)
}

pub fn rfc3339_from_unix(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Howard Hinnant `civil_from_days`：Unix 天数 → (y, m, d)
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m as u32, d as u32)
}

/// 加载结果：文档 + 可恢复告警。文件缺失 → 默认文档（首次升级，设计方案 §13.1）。
pub fn load() -> Result<(ToolchainDocument, Option<String>), String> {
    let path = toolchain_path()?;
    load_from(&path)
}

pub fn load_from(path: &Path) -> Result<(ToolchainDocument, Option<String>), String> {
    if !path.exists() {
        return Ok((ToolchainDocument::new(), None));
    }
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("读取 toolchain.json 失败: {e}"))?;
    match parse_document(&content) {
        Ok(mut doc) => {
            let mut warning = None;
            if doc.version != TOOLCHAIN_SCHEMA_VERSION {
                warning = Some(format!(
                    "toolchain.json schema 版本 {} 不受支持（当前 {}），已忽略缓存",
                    doc.version, TOOLCHAIN_SCHEMA_VERSION
                ));
                doc = ToolchainDocument::new();
            }
            // 路径必须为绝对（设计方案 §5.2：不做 ~ / 环境变量展开）；
            // 非绝对 override 视为无效意图并告警（不静默删除，仅本次忽略）。
            for (kind, value) in [
                ("node", &mut doc.overrides.node),
                ("npm", &mut doc.overrides.npm),
                ("adapter", &mut doc.overrides.adapter),
            ] {
                if let Some(v) = value {
                    if Path::new(v).is_absolute() {
                        continue;
                    }
                    warning = Some(format!("{kind} override 不是绝对路径，已忽略: {v}"));
                    *value = None;
                }
            }
            Ok((doc, warning))
        }
        Err(error) => Ok((
            ToolchainDocument::new(),
            Some(format!(
                "toolchain.json 无法解析，已按默认配置继续（原文件未修改）: {error}"
            )),
        )),
    }
}

fn parse_document(content: &str) -> Result<ToolchainDocument, String> {
    serde_json::from_str(content).map_err(|e| e.to_string())
}

/// 原子写入（temp + rename，仅当前用户可读写，复用共享目录规范）
pub fn save(doc: &ToolchainDocument) -> Result<(), String> {
    let path = toolchain_path()?;
    save_to(&path, doc)
}

pub fn save_to(path: &Path, doc: &ToolchainDocument) -> Result<(), String> {
    let mut persisted = doc.clone();
    persisted.version = TOOLCHAIN_SCHEMA_VERSION;
    let content = serde_json::to_vec_pretty(&persisted).map_err(|e| e.to_string())?;
    crate::storage::write_private_file(path, content)
}

#[cfg(test)]
mod tests {
    use super::super::model::ToolchainMode;
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "ailight-store-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ))
    }

    #[test]
    fn rfc3339_formats_known_epochs() {
        assert_eq!(rfc3339_from_unix(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339_from_unix(1_756_526_400), "2025-08-30T04:00:00Z");
        // 闰年边界：2024-02-29T12:00:00Z
        assert_eq!(rfc3339_from_unix(1_709_208_000), "2024-02-29T12:00:00Z");
    }

    #[test]
    fn load_missing_file_returns_default_without_warning() {
        let dir = temp_dir("missing");
        let (doc, warning) = load_from(&dir.join("toolchain.json")).unwrap();
        assert_eq!(doc, ToolchainDocument::new());
        assert!(warning.is_none());
    }

    #[test]
    fn load_corrupt_file_keeps_file_and_warns() {
        let dir = temp_dir("corrupt");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("toolchain.json"), "{broken").unwrap();
        let (doc, warning) = load_from(&dir.join("toolchain.json")).unwrap();
        assert_eq!(doc, ToolchainDocument::new());
        assert!(warning.unwrap().contains("无法解析"));
        assert_eq!(
            std::fs::read_to_string(dir.join("toolchain.json")).unwrap(),
            "{broken"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_relative_override_is_rejected_with_warning() {
        let dir = temp_dir("relative");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("toolchain.json"),
            r#"{"version":1,"mode":"manual","overrides":{"node":"node.exe"}}"#,
        )
        .unwrap();
        let (doc, warning) = load_from(&dir.join("toolchain.json")).unwrap();
        assert!(doc.overrides.node.is_none());
        assert!(warning.unwrap().contains("不是绝对路径"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_writes_schema_version_atomically() {
        let dir = temp_dir("save");
        let mut doc = ToolchainDocument::new();
        doc.mode = ToolchainMode::Manual;
        save_to(&dir.join("toolchain.json"), &doc).unwrap();
        let raw = std::fs::read_to_string(dir.join("toolchain.json")).unwrap();
        assert!(raw.contains("\"version\": 1"));
        assert!(raw.contains("\"mode\": \"manual\""));
        let (back, warning) = load_from(&dir.join("toolchain.json")).unwrap();
        assert_eq!(back, doc);
        assert!(warning.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
