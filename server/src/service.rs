use std::{
    fs, io,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow, bail};

use crate::paths;

const LEGACY_SERVICE_LABEL: &str = "com.latch.server";

#[derive(Debug, Clone)]
pub struct InstallOptions {
    pub force: bool,
    pub no_start: bool,
    pub program: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ServiceStatus {
    pub loaded: bool,
    pub legacy_loaded: bool,
    pub plist_path: PathBuf,
    pub launch_agent_path: PathBuf,
    pub launch_agent_points_to_plist: bool,
}

pub fn install(options: InstallOptions) -> Result<()> {
    ensure_macos()?;
    paths::ensure_runtime_dirs()?;

    let program = options
        .program
        .map(Ok)
        .unwrap_or_else(std::env::current_exe)
        .context("Failed to resolve current executable path")?;
    let plist_path = paths::launchd_plist_file();
    let launch_agent_path = paths::user_launch_agent_plist_file();
    let plist = launch_agent_plist(&program);

    fs::write(&plist_path, plist)
        .with_context(|| format!("Failed to write {}", plist_path.display()))?;
    ensure_launch_agent_link(&plist_path, &launch_agent_path, options.force)?;
    append_install_log(format!(
        "installed launch agent {} -> {}",
        launch_agent_path.display(),
        plist_path.display()
    ))?;

    if !options.no_start {
        stop_legacy_if_loaded()?;
        start()?;
    }

    Ok(())
}

pub fn uninstall() -> Result<()> {
    ensure_macos()?;
    let _ = stop();

    let launch_agent_path = paths::user_launch_agent_plist_file();
    if path_exists(&launch_agent_path) {
        fs::remove_file(&launch_agent_path)
            .with_context(|| format!("Failed to remove {}", launch_agent_path.display()))?;
    }

    let plist_path = paths::launchd_plist_file();
    if path_exists(&plist_path) {
        fs::remove_file(&plist_path)
            .with_context(|| format!("Failed to remove {}", plist_path.display()))?;
    }

    append_install_log("uninstalled launch agent")?;
    Ok(())
}

pub fn start() -> Result<()> {
    ensure_macos()?;
    stop_legacy_if_loaded()?;

    let status = status()?;
    if !status.loaded {
        let launch_agent_path = paths::user_launch_agent_plist_file();
        if !path_exists(&launch_agent_path) {
            bail!(
                "LaunchAgent is not installed. Run `latch service install` first. Expected {}",
                launch_agent_path.display()
            );
        }

        let domain = launchctl_domain();
        let plist = path_arg(&launch_agent_path);
        run_launchctl(&["bootstrap", &domain, &plist])
            .context("Failed to bootstrap LaunchAgent")?;
        wait_for_label_loaded(paths::SERVICE_LABEL, Duration::from_secs(5))?;
    }

    let target = format!("{}/{}", launchctl_domain(), paths::SERVICE_LABEL);
    run_launchctl(&["kickstart", "-k", &target]).context("Failed to start LaunchAgent")?;
    append_install_log("started launch agent")?;

    Ok(())
}

pub fn stop() -> Result<()> {
    ensure_macos()?;
    if !is_loaded()? {
        return Ok(());
    }

    let target = format!("{}/{}", launchctl_domain(), paths::SERVICE_LABEL);
    run_launchctl(&["bootout", &target]).context("Failed to stop LaunchAgent")?;
    append_install_log("stopped launch agent")?;

    Ok(())
}

pub fn restart() -> Result<()> {
    ensure_macos()?;
    let _ = stop();
    start()
}

pub fn status() -> Result<ServiceStatus> {
    ensure_macos()?;
    let plist_path = paths::launchd_plist_file();
    let launch_agent_path = paths::user_launch_agent_plist_file();
    Ok(ServiceStatus {
        loaded: is_loaded()?,
        legacy_loaded: is_label_loaded(LEGACY_SERVICE_LABEL)?,
        plist_path: plist_path.clone(),
        launch_agent_path: launch_agent_path.clone(),
        launch_agent_points_to_plist: launch_agent_points_to_plist(&launch_agent_path, &plist_path),
    })
}

fn ensure_launch_agent_link(
    plist_path: &Path,
    launch_agent_path: &Path,
    force: bool,
) -> Result<()> {
    if let Some(parent) = launch_agent_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }

    if path_exists(launch_agent_path) {
        if launch_agent_points_to_plist(launch_agent_path, plist_path) {
            return Ok(());
        }

        if !force {
            bail!(
                "{} already exists and does not point to {}. Re-run with --force to replace it.",
                launch_agent_path.display(),
                plist_path.display()
            );
        }

        fs::remove_file(launch_agent_path)
            .with_context(|| format!("Failed to remove {}", launch_agent_path.display()))?;
    }

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(plist_path, launch_agent_path).with_context(|| {
            format!(
                "Failed to create symlink {} -> {}",
                launch_agent_path.display(),
                plist_path.display()
            )
        })?;
    }

    #[cfg(not(unix))]
    {
        fs::copy(plist_path, launch_agent_path).with_context(|| {
            format!(
                "Failed to copy {} to {}",
                plist_path.display(),
                launch_agent_path.display()
            )
        })?;
    }

    Ok(())
}

fn launch_agent_points_to_plist(launch_agent_path: &Path, plist_path: &Path) -> bool {
    match fs::read_link(launch_agent_path) {
        Ok(target) => target == plist_path,
        Err(_) => false,
    }
}

pub fn launch_agent_plist(program: &Path) -> String {
    let working_directory = paths::latch_home();
    let server_log = paths::server_log_file();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{label}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{program}</string>
    <string>serve</string>
  </array>
  <key>WorkingDirectory</key>
  <string>{working_directory}</string>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>{server_log}</string>
  <key>StandardErrorPath</key>
  <string>{server_log}</string>
</dict>
</plist>
"#,
        label = paths::SERVICE_LABEL,
        program = escape_xml(&program.display().to_string()),
        working_directory = escape_xml(&working_directory.display().to_string()),
        server_log = escape_xml(&server_log.display().to_string()),
    )
}

fn is_loaded() -> Result<bool> {
    is_label_loaded(paths::SERVICE_LABEL)
}

fn is_label_loaded(label: &str) -> Result<bool> {
    ensure_macos()?;
    let target = format!("{}/{}", launchctl_domain(), label);
    let output = Command::new("launchctl")
        .args(["print", &target])
        .output()
        .context("Failed to run launchctl print")?;
    Ok(output.status.success())
}

fn wait_for_label_loaded(label: &str, timeout: Duration) -> Result<()> {
    let started_at = Instant::now();
    loop {
        if is_label_loaded(label)? {
            return Ok(());
        }

        if started_at.elapsed() >= timeout {
            bail!("Timed out waiting for LaunchAgent `{label}` to load");
        }

        thread::sleep(Duration::from_millis(100));
    }
}

fn stop_legacy_if_loaded() -> Result<()> {
    if !is_label_loaded(LEGACY_SERVICE_LABEL)? {
        return Ok(());
    }

    let target = format!("{}/{}", launchctl_domain(), LEGACY_SERVICE_LABEL);
    run_launchctl(&["bootout", &target]).context("Failed to stop legacy LaunchAgent")?;
    append_install_log("stopped legacy launch agent com.latch.server")?;

    Ok(())
}

fn run_launchctl(args: &[&str]) -> Result<()> {
    let output = Command::new("launchctl")
        .args(args)
        .output()
        .with_context(|| format!("Failed to run launchctl {}", args.join(" ")))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    Err(anyhow!(
        "launchctl {} failed: {}{}",
        args.join(" "),
        stdout,
        stderr
    ))
}

fn launchctl_domain() -> String {
    format!("gui/{}", current_uid())
}

#[cfg(unix)]
fn current_uid() -> u32 {
    use std::os::unix::fs::MetadataExt;

    fs::metadata(paths::home_dir())
        .map(|metadata| metadata.uid())
        .unwrap_or(0)
}

#[cfg(not(unix))]
fn current_uid() -> u32 {
    0
}

fn append_install_log(message: impl AsRef<str>) -> Result<()> {
    paths::ensure_runtime_dirs()?;
    let now = chrono::Utc::now().to_rfc3339();
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(paths::install_log_file())?
        .write_all(format!("{now} {}\n", message.as_ref()).as_bytes())?;
    Ok(())
}

fn path_arg(path: &Path) -> String {
    path.display().to_string()
}

fn path_exists(path: &Path) -> bool {
    match path.try_exists() {
        Ok(exists) => exists,
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(_) => false,
    }
}

fn escape_xml(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn ensure_macos() -> Result<()> {
    if cfg!(target_os = "macos") {
        Ok(())
    } else {
        bail!("Latch service management is currently only supported on macOS")
    }
}

#[cfg(test)]
mod tests {
    use super::launch_agent_plist;

    #[test]
    fn launch_agent_plist_escapes_xml_paths() {
        let plist = launch_agent_plist(std::path::Path::new("/tmp/latch&bin"));
        assert!(plist.contains("/tmp/latch&amp;bin"));
        assert!(plist.contains("<string>serve</string>"));
    }
}
