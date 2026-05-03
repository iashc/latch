mod browser_import;
mod client_packages;
mod config;
mod error;
mod models;
mod paths;
mod routes;
mod search;
mod service;
mod store;
mod sync;

use std::{collections::VecDeque, fs, net::SocketAddr, path::PathBuf, time::Duration};

use anyhow::{Context, Result, anyhow};
use clap::{Args, Parser, Subcommand};
use serde::Deserialize;
use tokio::{net::TcpListener, signal};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use crate::{
    browser_import::parse_browser_bookmarks_html,
    client_packages::ClientKind,
    config::{load_config, write_config},
    models::{ImportBookmarksRequest, normalize_url},
    store::AppStore,
};

#[derive(Debug, Parser)]
#[command(
    name = "latch",
    version,
    about = "Local-first bookmark service and personal clients"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run the local HTTP service in the foreground.
    Serve,
    /// Print service, config, and API status.
    Status,
    /// Check local setup and common runtime problems.
    Doctor,
    /// Print server logs from ~/.latch/logs.
    Logs(LogsArgs),
    /// Shortcut for `latch service start`.
    Start,
    /// Shortcut for `latch service stop`.
    Stop,
    /// Shortcut for `latch service restart`.
    Restart,
    /// Manage the macOS LaunchAgent.
    Service {
        #[command(subcommand)]
        command: ServiceCommand,
    },
    /// View or update ~/.config/latch/config.toml.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Install or inspect the unpacked Chrome extension package.
    #[command(alias = "browser")]
    Chrome {
        #[command(subcommand)]
        command: ClientCommand,
    },
    /// Install or inspect the Raycast extension package.
    Raycast {
        #[command(subcommand)]
        command: ClientCommand,
    },
    /// Import external bookmark data.
    Import {
        #[command(subcommand)]
        command: ImportCommand,
    },
    /// Backward-compatible alias for `latch import browser-html`.
    #[command(name = "import-browser-html", hide = true)]
    ImportBrowserHtml { path: PathBuf },
}

#[derive(Debug, Args)]
struct LogsArgs {
    #[arg(short = 'n', long, default_value_t = 80)]
    lines: usize,
}

#[derive(Debug, Subcommand)]
enum ServiceCommand {
    Install(ServiceInstallArgs),
    Uninstall,
    Start,
    Stop,
    Restart,
    Status,
    /// Print the LaunchAgent plist generated for the current binary.
    PrintPlist(ServicePrintPlistArgs),
}

#[derive(Debug, Args)]
struct ServiceInstallArgs {
    /// Replace an existing ~/Library/LaunchAgents plist/symlink if it points elsewhere.
    #[arg(long)]
    force: bool,
    /// Write the LaunchAgent files without starting the service.
    #[arg(long)]
    no_start: bool,
    /// Program path to use in the LaunchAgent. Defaults to the current executable.
    #[arg(long)]
    program: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct ServicePrintPlistArgs {
    /// Program path to render in the plist. Defaults to the current executable.
    #[arg(long)]
    program: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    Show,
    /// Store bookmarks in ~/.latch/data/latch.jsonl.
    UseLocal,
    /// Store bookmarks in iCloud Drive under com~apple~CloudDocs/latch.
    UseIcloud,
}

#[derive(Debug, Subcommand)]
enum ClientCommand {
    Install(ClientInstallArgs),
    /// Re-download and reinstall the selected client package.
    Update(ClientInstallArgs),
    /// Print the locally installed package path.
    Path,
    /// Open the locally installed package in Finder.
    Open,
    /// Remove the locally installed package and state.
    Uninstall,
}

#[derive(Debug, Args)]
struct ClientInstallArgs {
    /// GitHub repository containing release assets, for example iashc/latch.
    #[arg(long)]
    repo: Option<String>,
    /// Release tag to install. Defaults to the latest GitHub release.
    #[arg(long)]
    version: Option<String>,
    /// Re-download and replace an already installed version.
    #[arg(long)]
    force: bool,
}

#[derive(Debug, Subcommand)]
enum ImportCommand {
    /// Import a Chrome / Edge / Firefox / Safari bookmarks HTML export.
    BrowserHtml { path: PathBuf },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command.unwrap_or(Command::Serve) {
        Command::Serve => run_server().await,
        Command::Status => print_status().await,
        Command::Doctor => print_doctor().await,
        Command::Logs(args) => print_logs(args.lines),
        Command::Start => {
            service::start()?;
            println!("Latch 服务已启动");
            Ok(())
        }
        Command::Stop => {
            service::stop()?;
            println!("Latch 服务已停止");
            Ok(())
        }
        Command::Restart => {
            service::restart()?;
            println!("Latch 服务已重启");
            Ok(())
        }
        Command::Service { command } => handle_service_command(command),
        Command::Config { command } => handle_config_command(command),
        Command::Chrome { command } => handle_client_command(ClientKind::Chrome, command).await,
        Command::Raycast { command } => handle_client_command(ClientKind::Raycast, command).await,
        Command::Import { command } => match command {
            ImportCommand::BrowserHtml { path } => import_browser_html(path).await,
        },
        Command::ImportBrowserHtml { path } => import_browser_html(path).await,
    }
}

async fn run_server() -> Result<()> {
    let config = load_config()?;
    init_tracing(&config.log_level);

    for warning in &config.warnings {
        warn!("{warning}");
    }

    let store = AppStore::load(config.data_file.clone())?;
    let bookmark_count = store.count().await;

    info!(config_path = %config.config_path.display(), "loaded config");
    info!(data_file = %config.data_file.display(), "using data file");
    info!(bookmark_count, "loaded bookmarks");

    let _watcher = sync::spawn_file_watcher(store.clone())?;
    let app = routes::router(store);

    let address = SocketAddr::from(([127, 0, 0, 1], config.port));
    let listener = TcpListener::bind(address).await?;
    info!(listen = %address, "latch server listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn import_browser_html(path: PathBuf) -> Result<()> {
    let config = load_config()?;
    init_tracing(&config.log_level);

    for warning in &config.warnings {
        warn!("{warning}");
    }

    let html = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read browser bookmarks export {}", path.display()))?;
    let parsed_items = parse_browser_bookmarks_html(&html)?;
    let parsed_total = parsed_items.len();

    let mut importable_items = Vec::new();
    let mut skipped_non_http = 0;
    for item in parsed_items {
        if normalize_url(&item.url).is_ok() {
            importable_items.push(item);
        } else {
            skipped_non_http += 1;
        }
    }

    let store = AppStore::load(config.data_file.clone())?;
    let result = store
        .import(ImportBookmarksRequest {
            items: importable_items,
        })
        .await
        .map_err(|error| anyhow!(error.message().to_owned()))?;

    println!("浏览器书签导入完成");
    println!("文件: {}", path.display());
    println!("解析到书签: {parsed_total}");
    println!("跳过非网页书签: {skipped_non_http}");
    println!("新建: {}", result.created);
    println!("恢复: {}", result.restored);
    println!("跳过重复: {}", result.skipped);
    println!("数据文件: {}", config.data_file.display());

    Ok(())
}

fn handle_service_command(command: ServiceCommand) -> Result<()> {
    match command {
        ServiceCommand::Install(args) => {
            service::install(service::InstallOptions {
                force: args.force,
                no_start: args.no_start,
                program: args.program,
            })?;
            println!("LaunchAgent 已安装");
            println!("plist: {}", paths::launchd_plist_file().display());
            println!(
                "系统入口: {}",
                paths::user_launch_agent_plist_file().display()
            );
            Ok(())
        }
        ServiceCommand::Uninstall => {
            service::uninstall()?;
            println!("LaunchAgent 已卸载");
            Ok(())
        }
        ServiceCommand::Start => {
            service::start()?;
            println!("Latch 服务已启动");
            Ok(())
        }
        ServiceCommand::Stop => {
            service::stop()?;
            println!("Latch 服务已停止");
            Ok(())
        }
        ServiceCommand::Restart => {
            service::restart()?;
            println!("Latch 服务已重启");
            Ok(())
        }
        ServiceCommand::Status => {
            let status = service::status()?;
            println!(
                "LaunchAgent: {}",
                if status.loaded {
                    "loaded"
                } else {
                    "not loaded"
                }
            );
            println!(
                "旧 LaunchAgent(com.latch.server): {}",
                if status.legacy_loaded {
                    "loaded"
                } else {
                    "not loaded"
                }
            );
            println!("plist: {}", status.plist_path.display());
            println!("系统入口: {}", status.launch_agent_path.display());
            println!(
                "系统入口指向 plist: {}",
                if status.launch_agent_points_to_plist {
                    "yes"
                } else {
                    "no"
                }
            );
            Ok(())
        }
        ServiceCommand::PrintPlist(args) => {
            let program = args
                .program
                .map(Ok)
                .unwrap_or_else(std::env::current_exe)
                .context("Failed to resolve current executable path")?;
            print!("{}", service::launch_agent_plist(&program));
            Ok(())
        }
    }
}

fn handle_config_command(command: ConfigCommand) -> Result<()> {
    match command {
        ConfigCommand::Show => {
            let config = load_config()?;
            print_config(&config);
            Ok(())
        }
        ConfigCommand::UseLocal => {
            let current = load_config()?;
            let data_file = paths::local_data_file();
            sync::ensure_storage_ready(&data_file)?;
            write_config(data_file.clone(), current.port, current.log_level)?;
            println!("已切换到本地数据文件: {}", data_file.display());
            Ok(())
        }
        ConfigCommand::UseIcloud => {
            let current = load_config()?;
            let data_file = paths::icloud_data_file();
            sync::ensure_storage_ready(&data_file)?;
            write_config(data_file.clone(), current.port, current.log_level)?;
            println!("已切换到 iCloud 数据文件: {}", data_file.display());
            Ok(())
        }
    }
}

async fn handle_client_command(kind: ClientKind, command: ClientCommand) -> Result<()> {
    match command {
        ClientCommand::Install(args) => {
            let installed =
                client_packages::install(kind, client_install_options(args, false)?).await?;
            println!(
                "{} 客户端已安装: {}",
                installed.kind.display_name(),
                installed.path.display()
            );
            println!("版本: {}", installed.version);
            Ok(())
        }
        ClientCommand::Update(args) => {
            let installed =
                client_packages::install(kind, client_install_options(args, true)?).await?;
            println!(
                "{} 客户端已更新: {}",
                installed.kind.display_name(),
                installed.path.display()
            );
            println!("版本: {}", installed.version);
            Ok(())
        }
        ClientCommand::Path => {
            let path = client_packages::installed_path(kind).ok_or_else(|| {
                anyhow!(
                    "{} 客户端尚未安装，请先运行 `latch {} install`",
                    kind.display_name(),
                    kind.id()
                )
            })?;
            println!("{}", path.display());
            Ok(())
        }
        ClientCommand::Open => client_packages::open(kind),
        ClientCommand::Uninstall => {
            client_packages::uninstall(kind)?;
            println!("{} 客户端已移除", kind.display_name());
            Ok(())
        }
    }
}

fn client_install_options(
    args: ClientInstallArgs,
    force_from_command: bool,
) -> Result<client_packages::InstallOptions> {
    let repo = args
        .repo
        .unwrap_or_else(|| paths::DEFAULT_RELEASE_REPO.to_owned());
    if !repo.contains('/') {
        return Err(anyhow!("--repo must use owner/name format, got `{repo}`"));
    }

    Ok(client_packages::InstallOptions {
        repo,
        version: args.version,
        force: args.force || force_from_command,
    })
}

async fn print_status() -> Result<()> {
    let config = load_config()?;
    print_config(&config);
    println!("Latch 主目录: {}", paths::latch_home().display());
    println!("服务日志: {}", paths::server_log_file().display());

    let base_url = format!("http://127.0.0.1:{}", config.port);
    match fetch_status(&base_url).await {
        Ok(status) => {
            println!("API: ok");
            if let Some(total) = status.total {
                println!("书签总数: {total}");
            }
        }
        Err(error) => {
            println!("API: unavailable ({error})");
        }
    }

    if cfg!(target_os = "macos") {
        if let Ok(status) = service::status() {
            println!(
                "LaunchAgent: {}",
                if status.loaded {
                    "loaded"
                } else {
                    "not loaded"
                }
            );
            if status.legacy_loaded {
                println!("旧 LaunchAgent(com.latch.server): loaded");
            }
        }
    }

    Ok(())
}

async fn print_doctor() -> Result<()> {
    let config = load_config()?;
    let mut checks = Vec::new();
    checks.push(check(
        paths::path_exists(&config.config_path),
        "配置文件存在",
        config.config_path.display().to_string(),
    ));
    checks.push(check(
        config.data_file.parent().is_some_and(paths::path_exists),
        "数据目录存在",
        config
            .data_file
            .parent()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<none>".to_owned()),
    ));
    checks.push(check(
        paths::path_exists(&paths::latch_home()),
        "Latch 主目录存在",
        paths::latch_home().display().to_string(),
    ));
    let icloud_data_file = paths::icloud_data_file();
    let icloud_parent = icloud_data_file
        .parent()
        .unwrap_or_else(|| std::path::Path::new("/"));
    checks.push(check(
        paths::path_exists(&icloud_data_file) || paths::path_exists(icloud_parent),
        "iCloud Drive 路径可见",
        icloud_data_file.display().to_string(),
    ));

    let base_url = format!("http://127.0.0.1:{}", config.port);
    checks.push(match fetch_status(&base_url).await {
        Ok(_) => "[ok] API 健康检查通过".to_owned(),
        Err(error) => format!("[warn] API 暂不可用: {error}"),
    });

    if cfg!(target_os = "macos") {
        match service::status() {
            Ok(status) => {
                checks.push(check(
                    status.launch_agent_points_to_plist,
                    "LaunchAgent 系统入口正确",
                    status.launch_agent_path.display().to_string(),
                ));
                checks.push(format!(
                    "[{}] LaunchAgent 加载状态: {}",
                    if status.loaded { "ok" } else { "warn" },
                    if status.loaded {
                        "loaded"
                    } else {
                        "not loaded"
                    }
                ));
                checks.push(format!(
                    "[{}] 旧 LaunchAgent(com.latch.server): {}",
                    if status.legacy_loaded { "warn" } else { "ok" },
                    if status.legacy_loaded {
                        "loaded"
                    } else {
                        "not loaded"
                    }
                ));
            }
            Err(error) => checks.push(format!("[warn] 无法检查 LaunchAgent: {error}")),
        }
    }

    for line in checks {
        println!("{line}");
    }

    Ok(())
}

fn print_config(config: &config::AppConfig) {
    println!("配置文件: {}", config.config_path.display());
    println!("数据文件: {}", config.data_file.display());
    println!("端口: {}", config.port);
    println!("日志级别: {}", config.log_level);
    println!("CLI 日志: {}", paths::cli_log_file().display());
    for warning in &config.warnings {
        println!("配置警告: {warning}");
    }
}

fn print_logs(lines: usize) -> Result<()> {
    let path = paths::server_log_file();
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read server log {}", path.display()))?;
    let lines = lines.max(1);
    let mut tail = VecDeque::with_capacity(lines);

    for line in raw.lines() {
        if tail.len() == lines {
            tail.pop_front();
        }
        tail.push_back(line);
    }

    for line in tail {
        println!("{line}");
    }

    Ok(())
}

fn check(ok: bool, label: &str, detail: String) -> String {
    format!("[{}] {label}: {detail}", if ok { "ok" } else { "warn" })
}

#[derive(Debug, Deserialize)]
struct ApiListStatus {
    total: usize,
}

struct ApiStatus {
    total: Option<usize>,
}

async fn fetch_status(base_url: &str) -> Result<ApiStatus> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()?;
    client
        .get(format!("{base_url}/health"))
        .send()
        .await?
        .error_for_status()?;

    let total = client
        .get(format!("{base_url}/api/bookmarks?limit=1"))
        .send()
        .await?
        .error_for_status()?
        .json::<ApiListStatus>()
        .await
        .ok()
        .map(|response| response.total);

    Ok(ApiStatus { total })
}

fn init_tracing(level: &str) {
    let filter = EnvFilter::try_new(level).unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();
}

async fn shutdown_signal() {
    #[cfg(unix)]
    let terminate = async {
        let mut signal = signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler");
        signal.recv().await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = signal::ctrl_c() => {},
        _ = terminate => {},
    }

    info!("shutdown signal received");
}
