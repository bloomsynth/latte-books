//! Connecting AI assistants to Latte Books, as DaVinci Resolve 21.1's "Setup AI
//! Assistants" does: an entry for `latte-mcp` goes into each assistant's own config, and
//! disconnecting takes out only that entry.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

/// The name of Latte's entry in an assistant's config.
pub const ENTRY: &str = "latte-books";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    ClaudeDesktop,
    ClaudeCode,
    Codex,
    Cursor,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Self::ClaudeDesktop => "Claude Desktop",
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex",
            Self::Cursor => "Cursor",
        }
    }

    /// What to do after connecting for the assistant to see Latte.
    pub fn after_connecting(self) -> &'static str {
        match self {
            Self::ClaudeDesktop => "Quit and reopen Claude Desktop.",
            Self::Cursor => "Reopen Cursor.",
            Self::ClaudeCode | Self::Codex => "Start a new session.",
        }
    }
}

/// An assistant this computer may have, and where its config is.
#[derive(Debug, Clone, PartialEq)]
pub struct Assistant {
    pub kind: Kind,
    pub config: PathBuf,
    /// Whether the assistant looks to be on this computer.
    pub found: bool,
}

fn home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

/// The assistants Latte can connect, found or not.
pub fn all() -> Vec<Assistant> {
    let Some(home) = home() else {
        return Vec::new();
    };
    let desktop_dir = if cfg!(windows) {
        std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("Claude"))
    } else if cfg!(target_os = "macos") {
        Some(
            home.join("Library")
                .join("Application Support")
                .join("Claude"),
        )
    } else {
        Some(home.join(".config").join("Claude"))
    };
    let codex_dir = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".codex"));
    let mut list = Vec::new();
    if let Some(dir) = desktop_dir {
        list.push(Assistant {
            kind: Kind::ClaudeDesktop,
            found: dir.is_dir(),
            config: dir.join("claude_desktop_config.json"),
        });
    }
    let claude_json = home.join(".claude.json");
    list.push(Assistant {
        kind: Kind::ClaudeCode,
        found: claude_json.is_file() || home.join(".claude").is_dir(),
        config: claude_json,
    });
    list.push(Assistant {
        kind: Kind::Codex,
        found: codex_dir.is_dir(),
        config: codex_dir.join("config.toml"),
    });
    let cursor_dir = home.join(".cursor");
    list.push(Assistant {
        kind: Kind::Cursor,
        found: cursor_dir.is_dir(),
        config: cursor_dir.join("mcp.json"),
    });
    list
}

/// `latte-mcp` beside the running app, if it is there.
pub fn server_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let name = if cfg!(windows) {
        "latte-mcp.exe"
    } else {
        "latte-mcp"
    };
    let path = exe.with_file_name(name);
    path.is_file().then_some(path)
}

fn read(path: &Path) -> Result<Option<String>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("Couldn't read {}: {e}", path.display())),
    }
}

/// Writes through a file beside it, so the assistant never reads half a config.
fn write(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut temp = path.as_os_str().to_owned();
    temp.push(".latte-tmp");
    let temp = PathBuf::from(temp);
    std::fs::write(&temp, text)
        .and_then(|()| std::fs::rename(&temp, path))
        .map_err(|e| format!("Couldn't write {}: {e}", path.display()))
}

fn read_json(path: &Path) -> Result<Value, String> {
    match read(path)? {
        Some(text) if !text.trim().is_empty() => serde_json::from_str(&text).map_err(|e| {
            format!(
                "{} isn't valid JSON ({e}), so it was left as it is",
                path.display()
            )
        }),
        _ => Ok(json!({})),
    }
}

fn read_toml(path: &Path) -> Result<toml_edit::DocumentMut, String> {
    read(path)?.unwrap_or_default().parse().map_err(|e| {
        format!(
            "{} isn't valid TOML ({e}), so it was left as it is",
            path.display()
        )
    })
}

impl Assistant {
    /// The `latte-mcp` the assistant's entry runs, if it has one.
    pub fn connected_to(&self) -> Option<PathBuf> {
        match self.kind {
            Kind::Codex => {
                let doc = read_toml(&self.config).ok()?;
                let command = doc
                    .get("mcp_servers")?
                    .get(ENTRY)?
                    .get("command")?
                    .as_str()?;
                Some(PathBuf::from(command))
            }
            _ => {
                let config = read_json(&self.config).ok()?;
                let command = config
                    .get("mcpServers")?
                    .get(ENTRY)?
                    .get("command")?
                    .as_str()?;
                Some(PathBuf::from(command))
            }
        }
    }

    pub fn is_connected(&self) -> bool {
        self.connected_to().is_some()
    }

    /// Adds Latte's entry, running `server`, or points it at `server`.
    pub fn connect(&self, server: &Path) -> Result<(), String> {
        let command = server.to_string_lossy().to_string();
        match self.kind {
            Kind::Codex => {
                let mut doc = read_toml(&self.config)?;
                let servers = doc
                    .entry("mcp_servers")
                    .or_insert_with(|| {
                        let mut table = toml_edit::Table::new();
                        table.set_implicit(true);
                        toml_edit::Item::Table(table)
                    })
                    .as_table_mut()
                    .ok_or("mcp_servers in the Codex config isn't a table")?;
                let mut entry = toml_edit::Table::new();
                entry.insert("command", toml_edit::value(command));
                entry.insert("args", toml_edit::value(toml_edit::Array::new()));
                servers.insert(ENTRY, toml_edit::Item::Table(entry));
                write(&self.config, &doc.to_string())
            }
            kind => {
                let mut config = read_json(&self.config)?;
                let entry = if kind == Kind::ClaudeCode {
                    json!({ "type": "stdio", "command": command, "args": [], "env": {} })
                } else {
                    json!({ "command": command, "args": [] })
                };
                let object = config
                    .as_object_mut()
                    .ok_or_else(|| format!("{} isn't a JSON object", self.config.display()))?;
                let servers = object.entry("mcpServers").or_insert_with(|| json!({}));
                servers
                    .as_object_mut()
                    .ok_or("mcpServers isn't an object")?
                    .insert(ENTRY.to_owned(), entry);
                let text = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
                write(&self.config, &text)
            }
        }
    }

    /// Takes out Latte's entry, leaving the rest of the config.
    pub fn disconnect(&self) -> Result<(), String> {
        if !self.is_connected() {
            return Ok(());
        }
        match self.kind {
            Kind::Codex => {
                let mut doc = read_toml(&self.config)?;
                if let Some(servers) = doc
                    .get_mut("mcp_servers")
                    .and_then(|s| s.as_table_like_mut())
                {
                    servers.remove(ENTRY);
                }
                write(&self.config, &doc.to_string())
            }
            _ => {
                let mut config = read_json(&self.config)?;
                if let Some(servers) = config.get_mut("mcpServers").and_then(Value::as_object_mut) {
                    servers.remove(ENTRY);
                }
                let text = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
                write(&self.config, &text)
            }
        }
    }
}

/// Points every connected assistant at `server`, after the app was updated or moved.
pub fn update_entries(server: &Path) {
    for assistant in all() {
        if let Some(current) = assistant.connected_to()
            && current != server
        {
            let _ = assistant.connect(server);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("latte-mcp-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_json_config_keeps_everything_but_latte() {
        let dir = temp_dir("json");
        let config = dir.join("claude_desktop_config.json");
        std::fs::write(
            &config,
            r#"{"theme":"dark","mcpServers":{"other":{"command":"other.exe"}}}"#,
        )
        .unwrap();
        let assistant = Assistant {
            kind: Kind::ClaudeDesktop,
            config: config.clone(),
            found: true,
        };
        let server = dir.join("latte-mcp.exe");
        assistant.connect(&server).unwrap();
        assert_eq!(assistant.connected_to(), Some(server));
        let value: Value =
            serde_json::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();
        assert_eq!(value["theme"], "dark");
        assert_eq!(value["mcpServers"]["other"]["command"], "other.exe");

        assistant.disconnect().unwrap();
        assert!(!assistant.is_connected());
        let value: Value =
            serde_json::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();
        assert_eq!(value["mcpServers"]["other"]["command"], "other.exe");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_codex_config_keeps_its_comments() {
        let dir = temp_dir("toml");
        let config = dir.join("config.toml");
        let before =
            "# my settings\nmodel = \"gpt-5\"\n\n[mcp_servers.other]\ncommand = \"other\"\n";
        std::fs::write(&config, before).unwrap();
        let assistant = Assistant {
            kind: Kind::Codex,
            config: config.clone(),
            found: true,
        };
        let server = dir.join("latte-mcp");
        assistant.connect(&server).unwrap();
        assert_eq!(assistant.connected_to(), Some(server));
        let text = std::fs::read_to_string(&config).unwrap();
        assert!(text.starts_with("# my settings\nmodel = \"gpt-5\""));
        assert!(text.contains("[mcp_servers.latte-books]"));

        assistant.disconnect().unwrap();
        assert_eq!(std::fs::read_to_string(&config).unwrap(), before);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_config_is_made() {
        let dir = temp_dir("new");
        let assistant = Assistant {
            kind: Kind::ClaudeCode,
            config: dir.join(".claude.json"),
            found: true,
        };
        assistant.connect(Path::new("latte-mcp")).unwrap();
        let value: Value =
            serde_json::from_str(&std::fs::read_to_string(&assistant.config).unwrap()).unwrap();
        assert_eq!(value["mcpServers"][ENTRY]["type"], "stdio");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
