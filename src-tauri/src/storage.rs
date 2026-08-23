use std::path::{Path, PathBuf};

use rand::distributions::{Alphanumeric, DistString};
use serde::Serialize;

pub fn ai_light_home() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("AILIGHT_HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    dirs::home_dir()
        .map(|home| home.join(".ailight"))
        .ok_or_else(|| "无法确定用户家目录".to_string())
}

pub fn ensure_home() -> Result<PathBuf, String> {
    let home = ai_light_home()?;
    std::fs::create_dir_all(&home).map_err(|e| e.to_string())?;
    set_private_dir_permissions(&home)?;
    Ok(home)
}

pub fn config_path() -> Result<PathBuf, String> {
    Ok(ensure_home()?.join("config.json"))
}

pub fn themes_dir() -> Result<PathBuf, String> {
    Ok(ensure_home()?.join("themes"))
}

pub fn logs_dir() -> Result<PathBuf, String> {
    let path = ensure_home()?.join("logs").join("desktop");
    std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    set_private_dir_permissions(&path)?;
    Ok(path)
}

pub fn migrate_legacy_config(legacy_dir: &Path) -> Result<(), String> {
    let destination = config_path()?;
    if destination.exists() {
        return Ok(());
    }
    let source = legacy_dir.join("config.json");
    if source.exists() {
        std::fs::copy(&source, &destination).map_err(|e| e.to_string())?;
        set_private_file_permissions(&destination)?;
    }
    let legacy_themes = legacy_dir.join("themes");
    let destination_themes = themes_dir()?;
    if legacy_themes.is_dir() && !destination_themes.exists() {
        copy_dir(&legacy_themes, &destination_themes)?;
    }
    Ok(())
}

pub fn runtime_token() -> String {
    Alphanumeric.sample_string(&mut rand::thread_rng(), 48)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeDescriptor<'a> {
    schema_version: u8,
    transport: RuntimeTransport,
    pid: u32,
    auth_token: &'a str,
    desktop_version: &'a str,
    protocol: RuntimeProtocol,
    started_at: u64,
}

#[derive(Serialize)]
struct RuntimeTransport {
    #[serde(rename = "type")]
    kind: &'static str,
    host: &'static str,
    port: u16,
}

#[derive(Serialize)]
struct RuntimeProtocol {
    min: u8,
    max: u8,
}

pub fn write_runtime(port: u16, token: &str, version: &str, started_at: u64) -> Result<(), String> {
    let home = ensure_home()?;
    let path = home.join("runtime.json");
    let temporary = home.join(format!("runtime.json.{}.tmp", std::process::id()));
    let descriptor = RuntimeDescriptor {
        schema_version: 1,
        transport: RuntimeTransport {
            kind: "http",
            host: "127.0.0.1",
            port,
        },
        pid: std::process::id(),
        auth_token: token,
        desktop_version: version,
        protocol: RuntimeProtocol { min: 1, max: 1 },
        started_at,
    };
    let content = serde_json::to_vec_pretty(&descriptor).map_err(|e| e.to_string())?;
    std::fs::write(&temporary, content).map_err(|e| e.to_string())?;
    set_private_file_permissions(&temporary)?;
    std::fs::rename(&temporary, &path).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn remove_runtime() {
    if let Ok(home) = ai_light_home() {
        let _ = std::fs::remove_file(home.join("runtime.json"));
    }
}

pub fn write_private_file(path: &Path, content: impl AsRef<[u8]>) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        set_private_dir_permissions(parent)?;
    }
    std::fs::write(path, content).map_err(|e| e.to_string())?;
    set_private_file_permissions(path)
}

fn copy_dir(source: &Path, destination: &Path) -> Result<(), String> {
    std::fs::create_dir_all(destination).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(source).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.file_type().map_err(|e| e.to_string())?.is_file() {
            std::fs::copy(entry.path(), destination.join(entry.file_name()))
                .map_err(|e| e.to_string())?;
        }
    }
    set_private_dir_permissions(destination)
}

#[cfg(unix)]
fn set_private_dir_permissions(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
        .map_err(|e| e.to_string())
}

#[cfg(not(unix))]
fn set_private_dir_permissions(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(unix)]
fn set_private_file_permissions(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| e.to_string())
}

#[cfg(not(unix))]
fn set_private_file_permissions(_path: &Path) -> Result<(), String> {
    Ok(())
}
