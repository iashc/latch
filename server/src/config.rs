use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use toml::Value;

use crate::paths;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub config_path: PathBuf,
    pub data_file: PathBuf,
    pub port: u16,
    pub log_level: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct PartialConfig {
    data_file: Option<PathBuf>,
    port: Option<u16>,
    log_level: Option<String>,
}

pub fn load_config() -> Result<AppConfig> {
    let config_path = paths::config_path();
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create config directory {}", parent.display()))?;
    }

    let default_data_file = paths::local_data_file();
    let mut warnings = Vec::new();
    let partial = if config_path.exists() {
        parse_config_file(&config_path, &mut warnings)?
    } else {
        PartialConfig::default()
    };

    let log_level = partial
        .log_level
        .unwrap_or_else(|| paths::DEFAULT_LOG_LEVEL.to_owned());
    let allowed_levels = ["error", "warn", "info", "debug", "trace"];
    let final_log_level = if allowed_levels.iter().any(|level| *level == log_level) {
        log_level
    } else {
        warnings.push(format!(
            "Unknown log level `{log_level}` in config; falling back to `info`"
        ));
        paths::DEFAULT_LOG_LEVEL.to_owned()
    };

    let port = partial.port.unwrap_or(paths::DEFAULT_PORT);
    if port == 0 {
        bail!("Config field `port` must be greater than 0");
    }

    Ok(AppConfig {
        config_path,
        data_file: partial.data_file.unwrap_or(default_data_file),
        port,
        log_level: final_log_level,
        warnings,
    })
}

pub fn write_config(data_file: PathBuf, port: u16, log_level: String) -> Result<()> {
    if port == 0 {
        bail!("Config field `port` must be greater than 0");
    }

    let config_path = paths::config_path();
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create config directory {}", parent.display()))?;
    }

    let raw = toml::to_string_pretty(&PartialConfig {
        data_file: Some(data_file),
        port: Some(port),
        log_level: Some(log_level),
    })?;

    fs::write(&config_path, raw)
        .with_context(|| format!("Failed to write config file {}", config_path.display()))?;

    Ok(())
}

fn parse_config_file(path: &Path, warnings: &mut Vec<String>) -> Result<PartialConfig> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file {}", path.display()))?;

    if raw.trim().is_empty() {
        return Ok(PartialConfig::default());
    }

    let value = toml::from_str::<Value>(&raw)
        .with_context(|| format!("Failed to parse TOML from {}", path.display()))?;

    let table = value
        .as_table()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Config root must be a TOML table"))?;

    let known_fields = HashSet::from(["data_file", "port", "log_level"]);
    for key in table.keys() {
        if !known_fields.contains(key.as_str()) {
            warnings.push(format!(
                "Ignoring unknown config field `{key}` in {}",
                path.display()
            ));
        }
    }

    Ok(PartialConfig::deserialize(Value::Table(table))?)
}
