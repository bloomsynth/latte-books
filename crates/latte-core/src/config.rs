//! The app's config: `config.json` in its config folder, with the recent files, the
//! language and the company AI assistants use.
//!
//! Books keeps recent files in its Electron config (`files`, newest first, with the company
//! name); Latte keeps them in `config.json` in the app's config folder.

use std::path::{Path, PathBuf};

/// The name this computer's writes are recorded under in the change log.
pub fn device_id() -> String {
    let host = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "latte".to_owned());
    format!("latte-{host}")
}

/// The name an AI assistant's writes are recorded under, so the app can tell them from its
/// own and show them.
pub fn assistant_device_id() -> String {
    format!("{}-assistant", device_id())
}

/// The app's config folder: `%APPDATA%\Latte Books`, `~/Library/Application Support/Latte
/// Books` or `$XDG_CONFIG_HOME/latte-books`.
pub fn config_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("APPDATA").map(|dir| PathBuf::from(dir).join("Latte Books"))
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|home| {
            PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("Latte Books")
        })
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .map(|dir| dir.join("latte-books"))
    }
}

/// A recently opened file.
#[derive(Debug, Clone, PartialEq)]
pub struct RecentFile {
    pub path: PathBuf,
    pub company_name: String,
}

fn config_path() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join("config.json"))
}

fn read_config() -> serde_json::Value {
    config_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_else(|| serde_json::json!({}))
}

fn write_config(config: &serde_json::Value) {
    let Some(path) = config_path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string_pretty(config) {
        let _ = std::fs::write(path, text);
    }
}

/// A value in the config, such as `language`.
pub fn config_value(key: &str) -> Option<String> {
    read_config()
        .get(key)
        .and_then(|value| value.as_str())
        .map(str::to_owned)
}

pub fn set_config_value(key: &str, value: &str) {
    let mut config = read_config();
    config[key] = serde_json::Value::String(value.to_owned());
    write_config(&config);
}

/// The recent files that still exist, newest first.
pub fn recent_files() -> Vec<RecentFile> {
    read_config()
        .get("files")
        .and_then(|files| files.as_array())
        .into_iter()
        .flatten()
        .filter_map(|file| {
            let path = PathBuf::from(file.get("dbPath")?.as_str()?);
            let company_name = file
                .get("companyName")
                .and_then(|name| name.as_str())
                .unwrap_or_default()
                .to_owned();
            path.exists().then_some(RecentFile { path, company_name })
        })
        .collect()
}

/// Puts a file at the top of the recent files. The company the app last opened is also
/// the one AI assistants use, unless one is chosen for them.
pub fn remember_file(path: &Path, company_name: &str) {
    let mut config = read_config();
    let mut files: Vec<serde_json::Value> = config
        .get("files")
        .and_then(|files| files.as_array())
        .cloned()
        .unwrap_or_default();
    let path_text = path.to_string_lossy().to_string();
    files.retain(|file| file.get("dbPath").and_then(|p| p.as_str()) != Some(&path_text));
    files.insert(
        0,
        serde_json::json!({ "dbPath": path_text, "companyName": company_name }),
    );
    config["files"] = serde_json::Value::Array(files);
    config["lastSelectedFilePath"] = serde_json::Value::String(path_text);
    write_config(&config);
}

/// Takes a file off the recent files (Books' "Remove" on the selector), leaving the file.
pub fn forget_file(path: &Path) {
    let mut config = read_config();
    let path_text = path.to_string_lossy().to_string();
    if let Some(files) = config.get_mut("files").and_then(|f| f.as_array_mut()) {
        files.retain(|file| file.get("dbPath").and_then(|p| p.as_str()) != Some(&path_text));
    }
    write_config(&config);
}

/// The company file AI assistants work in: the one chosen for them, or else the one the
/// app last opened.
pub fn assistant_file() -> Option<PathBuf> {
    let config = read_config();
    ["assistantFilePath", "lastSelectedFilePath"]
        .iter()
        .filter_map(|key| config.get(*key).and_then(|v| v.as_str()))
        .map(PathBuf::from)
        .find(|path| path.exists())
}
