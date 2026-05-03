use std::{
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

pub const SERVICE_LABEL: &str = "com.iashc.latch";
pub const DEFAULT_PORT: u16 = 52_525;
pub const DEFAULT_LOG_LEVEL: &str = "info";
pub const DEFAULT_RELEASE_REPO: &str = "iashc/latch";

pub fn home_dir() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn config_path() -> PathBuf {
    if let Some(xdg_config_home) = env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(xdg_config_home).join("latch/config.toml");
    }

    home_dir().join(".config/latch/config.toml")
}

pub fn latch_home() -> PathBuf {
    env::var_os("LATCH_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".latch"))
}

pub fn local_data_file() -> PathBuf {
    latch_home().join("data/latch.jsonl")
}

pub fn icloud_data_file() -> PathBuf {
    home_dir().join("Library/Mobile Documents/com~apple~CloudDocs/latch/latch.jsonl")
}

pub fn cache_dir() -> PathBuf {
    latch_home().join("cache")
}

pub fn release_cache_dir() -> PathBuf {
    cache_dir().join("releases")
}

pub fn downloads_dir() -> PathBuf {
    cache_dir().join("downloads")
}

pub fn clients_dir() -> PathBuf {
    latch_home().join("clients")
}

pub fn client_dir(kind: &str) -> PathBuf {
    clients_dir().join(kind)
}

pub fn client_versions_dir(kind: &str) -> PathBuf {
    client_dir(kind).join("versions")
}

pub fn client_current_link(kind: &str) -> PathBuf {
    client_dir(kind).join("current")
}

pub fn logs_dir() -> PathBuf {
    latch_home().join("logs")
}

pub fn server_log_file() -> PathBuf {
    logs_dir().join("server.log")
}

pub fn cli_log_file() -> PathBuf {
    logs_dir().join("cli.log")
}

pub fn install_log_file() -> PathBuf {
    logs_dir().join("install.log")
}

pub fn launchd_dir() -> PathBuf {
    latch_home().join("launchd")
}

pub fn launchd_plist_file() -> PathBuf {
    launchd_dir().join(format!("{SERVICE_LABEL}.plist"))
}

pub fn user_launch_agents_dir() -> PathBuf {
    home_dir().join("Library/LaunchAgents")
}

pub fn user_launch_agent_plist_file() -> PathBuf {
    user_launch_agents_dir().join(format!("{SERVICE_LABEL}.plist"))
}

pub fn state_dir() -> PathBuf {
    latch_home().join("state")
}

pub fn clients_state_file() -> PathBuf {
    state_dir().join("clients.json")
}

pub fn tmp_dir() -> PathBuf {
    latch_home().join("tmp")
}

pub fn ensure_runtime_dirs() -> Result<()> {
    for path in [
        latch_home(),
        release_cache_dir(),
        downloads_dir(),
        clients_dir(),
        logs_dir(),
        launchd_dir(),
        state_dir(),
        tmp_dir(),
    ] {
        fs::create_dir_all(&path)
            .with_context(|| format!("Failed to create directory {}", path.display()))?;
    }

    Ok(())
}

pub fn path_exists(path: &Path) -> bool {
    path.try_exists().unwrap_or(false)
}
