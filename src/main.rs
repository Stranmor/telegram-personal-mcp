use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use rmcp::ServiceExt;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::io::{self, IsTerminal, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};
use std::sync::Arc;
use std::time::Duration;
use telegram_personal_mcp::{
    ApprovalBot, ApprovalPolicy, BotApiApprovalBot, Config, DEFAULT_MAX_DOWNLOAD_BYTES,
    DEFAULT_MAX_UPLOAD_BYTES, FakeTelegramBackend, FanoutApprovalBot, LoginCodeOutcome,
    LoginRequestOutcome, MAX_MESSAGE_CACHE_AGE_DAYS, McpServer, McpServerSettings, TelegramBackend,
    TelegramClient,
    daemon::{
        DaemonConfigIdentity, DaemonPaths, connect_existing_matching, install_proxy_signal_ignores,
        prepare_mcp_daemon, proxy_stdio, run_mcp_daemon, wait_for_matching_socket,
    },
};
use tracing::info;
use zeroize::Zeroizing;

#[derive(Parser)]
#[command(author, version, about)]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,
    /// Run the MCP server against a deterministic local fixture without Telegram credentials.
    #[arg(long, env = "TELEGRAM_MCP_DEMO")]
    demo: bool,
    #[arg(long, env = "TELEGRAM_MCP_SESSION_PATH", global = true)]
    session_path: Option<PathBuf>,
    #[arg(long, env = "TELEGRAM_MCP_SESSION_CACHE_DIR", global = true)]
    session_cache_dir: Option<PathBuf>,
    #[arg(long, env = "TELEGRAM_MCP_ATTACHMENT_CACHE_DIR")]
    attachment_cache_dir: Option<PathBuf>,
    #[arg(
        long,
        env = "TELEGRAM_MCP_MAX_DOWNLOAD_BYTES",
        default_value_t = DEFAULT_MAX_DOWNLOAD_BYTES
    )]
    max_download_bytes: u64,
    #[arg(
        long,
        env = "TELEGRAM_MCP_MAX_UPLOAD_BYTES",
        default_value_t = DEFAULT_MAX_UPLOAD_BYTES
    )]
    max_upload_bytes: u64,
    #[arg(long, env = "TELEGRAM_MCP_STATE_DB")]
    state_db_path: Option<PathBuf>,
    #[arg(long, env = "TELEGRAM_MCP_MESSAGE_CACHE_DB")]
    message_cache_db_path: Option<PathBuf>,
    #[arg(
        long,
        env = "TELEGRAM_MCP_MESSAGE_CACHE_MAX_AGE_DAYS",
        value_parser = parse_message_cache_max_age_days
    )]
    message_cache_max_age_days: Option<u32>,
    #[arg(long, env = "TELEGRAM_API_ID", global = true)]
    api_id: Option<i32>,
    /// Exact compatible daemon executable used by stdio proxy mode.
    ///
    /// This permits a newer transport/frontend generation to keep using one
    /// already-running, byte-identified daemon generation without opening the
    /// same Telegram session from a second process.
    #[arg(long, env = "TELEGRAM_MCP_DAEMON_EXECUTABLE", global = true, hide = true)]
    daemon_executable: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Connect stdio to the persistent per-user Telegram MCP daemon.
    Serve,
    /// Run the singleton persistent Telegram MCP daemon.
    Daemon,
    /// Run one recovery-only MCP server directly on stdio without daemon persistence.
    #[command(hide = true)]
    Direct,
    /// Validate session selection and Telegram account readiness.
    Doctor,
    /// Inspect configured Telegram sessions without opening them.
    Session {
        #[command(subcommand)]
        command: SessionCommand,
    },
    /// Create or refresh an authenticated Telegram user session.
    Auth {
        #[command(subcommand)]
        command: AuthCommand,
    },
}

#[derive(Debug, Subcommand)]
enum SessionCommand {
    /// List session files visible to this configuration.
    List,
}

#[derive(Debug, Subcommand)]
enum AuthCommand {
    /// Interactively request a code, handle optional 2FA, and save one session.
    Login,
}

#[derive(Debug, Serialize)]
struct DoctorReport {
    status: &'static str,
    session_path: String,
    user_id: i64,
    first_name: String,
    username: Option<String>,
}

#[derive(Debug, Serialize)]
struct SessionListReport {
    candidate_count: usize,
    candidates: Vec<String>,
}

#[derive(Debug, Serialize)]
struct AuthLoginReport {
    status: &'static str,
    session_path: String,
    user_id: i64,
    first_name: String,
    username: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "telegram_personal_mcp=info,warn".to_string()),
        )
        .with_writer(std::io::stderr)
        .init();

    let args = Args::parse();
    if args.demo
        && matches!(
            args.command,
            Some(Command::Doctor | Command::Session { .. } | Command::Auth { .. })
        )
    {
        bail!("--demo can be combined only with serve, daemon, or direct MCP modes");
    }
    match &args.command {
        Some(Command::Doctor) => return doctor(telegram_config(&args)?).await,
        Some(Command::Session { command: SessionCommand::List }) => {
            return list_sessions(&session_selection_config(&args));
        },
        Some(Command::Auth { command: AuthCommand::Login }) => {
            return auth_login(telegram_config(&args)?).await;
        },
        Some(Command::Daemon) => return daemon(&args).await,
        Some(Command::Direct) => return direct(&args).await,
        Some(Command::Serve) | None => return serve_proxy(&args).await,
    }
}

fn build_server(args: &Args) -> Result<McpServer> {
    let backend: Arc<dyn TelegramBackend> = if args.demo {
        info!("using deterministic demo backend; Telegram will not be contacted");
        Arc::new(FakeTelegramBackend::fixture())
    } else {
        Arc::new(TelegramClient::new(telegram_config(args)?))
    };
    let settings = effective_server_settings(args);
    let message_cache_path = effective_message_cache_path(args, &settings);
    let mut server = if args.demo {
        McpServer::with_settings(backend, settings)
    } else if let Some((policy, bot)) = approval_from_env()? {
        McpServer::with_approval(backend, settings, policy, bot).map_err(anyhow::Error::msg)?
    } else {
        McpServer::with_settings(backend, settings)
    };
    server = server.with_message_cache_path(message_cache_path);
    if let Some(max_age_days) = args.message_cache_max_age_days {
        server =
            server.with_message_cache_max_age_days(max_age_days).map_err(anyhow::Error::new)?;
    }
    Ok(server.with_compact_tool_discovery(true))
}

async fn daemon(args: &Args) -> Result<()> {
    let executable = std::env::current_exe()
        .context("failed to resolve the running daemon executable identity")?;
    let identity = daemon_config_identity(args, &executable)?;
    let paths = DaemonPaths::resolve_default();
    let prepared = prepare_mcp_daemon(paths.clone(), identity).map_err(anyhow::Error::new)?;
    info!(socket = %paths.socket_path.display(), "starting persistent MCP daemon");
    let server = build_server(args)?;
    server.enforce_message_cache_retention_now().await.map_err(|error| {
        anyhow::anyhow!(
            "message-cache startup retention failed ({}): {}",
            error.code,
            error.message
        )
    })?;
    let approval_worker = server
        .start_approval_worker()
        .context("failed to start the Telegram approval callback worker")?;
    let cache_worker = server.start_message_cache_worker();
    let result = run_mcp_daemon(server, prepared).await.map_err(anyhow::Error::new);
    cache_worker.shutdown().await;
    if let Some(worker) = approval_worker {
        worker.abort();
        let _ = worker.await;
    }
    result
}

async fn direct(args: &Args) -> Result<()> {
    let server = build_server(args)?;
    let approval_worker = server
        .start_approval_worker()
        .context("failed to start the Telegram approval callback worker")?;
    info!("starting recovery-only MCP 2025-11-25 server directly on stdio");
    server.serve(rmcp::transport::stdio()).await?.waiting().await?;
    if let Some(worker) = approval_worker {
        worker.abort();
        let _ = worker.await;
    }
    Ok(())
}

async fn serve_proxy(args: &Args) -> Result<()> {
    install_proxy_signal_ignores().context("failed to install stdio proxy signal handling")?;
    let paths = DaemonPaths::resolve_default();
    let daemon_executable = selected_daemon_executable(args)?;
    let expected_identity = daemon_config_identity(args, &daemon_executable)?;
    let stream = match connect_existing_matching(&paths, &expected_identity).await {
        Ok(stream) => stream,
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) =>
        {
            spawn_daemon(args, &paths, &daemon_executable)?;
            wait_for_matching_socket(&paths, &expected_identity, Duration::from_secs(30))
                .await
                .context("compatible persistent Telegram MCP daemon did not become ready")?
        },
        Err(error) => return Err(error).context("failed to connect to Telegram MCP daemon"),
    };
    proxy_stdio(stream).await.context("Telegram MCP stdio proxy failed")
}

fn effective_server_settings(args: &Args) -> McpServerSettings {
    let defaults = McpServerSettings::default();
    McpServerSettings {
        attachment_cache_dir: args
            .attachment_cache_dir
            .clone()
            .unwrap_or(defaults.attachment_cache_dir),
        max_download_bytes: args.max_download_bytes,
        max_upload_bytes: args.max_upload_bytes,
        state_db_path: args.state_db_path.clone().unwrap_or(defaults.state_db_path),
    }
}

fn effective_message_cache_path(args: &Args, settings: &McpServerSettings) -> PathBuf {
    args.message_cache_db_path.clone().unwrap_or_else(|| {
        settings
            .state_db_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("message-cache.sqlite")
    })
}

fn selected_daemon_executable(args: &Args) -> Result<PathBuf> {
    let executable = match &args.daemon_executable {
        Some(path) => {
            if !path.is_absolute() {
                bail!("TELEGRAM_MCP_DAEMON_EXECUTABLE must be an absolute path");
            }
            path.clone()
        },
        None => std::env::current_exe()
            .context("failed to resolve the executable for daemon selection")?,
    };
    let metadata = executable.metadata().with_context(|| {
        format!("failed to inspect selected daemon executable {}", executable.display())
    })?;
    if !metadata.is_file() {
        bail!("selected daemon executable is not a regular file: {}", executable.display());
    }
    Ok(executable)
}

fn daemon_config_identity(args: &Args, executable: &Path) -> Result<DaemonConfigIdentity> {
    let settings = effective_server_settings(args);
    let message_cache_path = effective_message_cache_path(args, &settings);
    let session_cache_dir = session_selection_config(args).session_cache_dir();
    let working_directory = std::env::current_dir()
        .context("failed to resolve the working directory for daemon configuration identity")?;
    let mut hasher = Sha256::new();
    hash_daemon_identity_field(
        &mut hasher,
        "format",
        Some(b"telegram-personal-mcp-effective-daemon-config-v2"),
    );
    hash_daemon_identity_file(&mut hasher, "executable_sha256", executable)?;
    hash_daemon_identity_field(
        &mut hasher,
        "package_version",
        Some(env!("CARGO_PKG_VERSION").as_bytes()),
    );
    hash_daemon_identity_field(&mut hasher, "demo", Some(if args.demo { b"1" } else { b"0" }));
    hash_daemon_identity_path(
        &mut hasher,
        "session_path",
        args.session_path.as_deref(),
        &working_directory,
    );
    hash_daemon_identity_path(
        &mut hasher,
        "session_cache_dir",
        Some(session_cache_dir.as_path()),
        &working_directory,
    );
    hash_daemon_identity_path(
        &mut hasher,
        "attachment_cache_dir",
        Some(settings.attachment_cache_dir.as_path()),
        &working_directory,
    );
    hash_daemon_identity_field(
        &mut hasher,
        "max_download_bytes",
        Some(args.max_download_bytes.to_string().as_bytes()),
    );
    hash_daemon_identity_field(
        &mut hasher,
        "max_upload_bytes",
        Some(args.max_upload_bytes.to_string().as_bytes()),
    );
    hash_daemon_identity_path(
        &mut hasher,
        "state_db_path",
        Some(settings.state_db_path.as_path()),
        &working_directory,
    );
    hash_daemon_identity_path(
        &mut hasher,
        "message_cache_db_path",
        Some(message_cache_path.as_path()),
        &working_directory,
    );
    let max_age_days = args.message_cache_max_age_days.map(|value| value.to_string());
    hash_daemon_identity_field(
        &mut hasher,
        "message_cache_max_age_days",
        max_age_days.as_deref().map(str::as_bytes),
    );
    let api_id = args.api_id.map(|value| value.to_string());
    hash_daemon_identity_field(&mut hasher, "api_id", api_id.as_deref().map(str::as_bytes));

    // These values may contain credentials. They are consumed only in memory
    // by this digest and never written to the daemon identity sidecar.
    for name in [
        "TELEGRAM_API_HASH",
        "TELEGRAM_MCP_PROXY",
        "TELEGRAM_MCP_APPROVAL_BOT_TOKEN",
        "TELEGRAM_MCP_APPROVAL_CHAT_ID",
        "TELEGRAM_MCP_APPROVAL_USER_ID",
        "TELEGRAM_MCP_APPROVAL_TTL_SECONDS",
        "TELEGRAM_MCP_APPROVAL_POLL_SECONDS",
        "TELEGRAM_MCP_APPROVAL_API_BASE",
        "TELEGRAM_MCP_APPROVAL_FANOUT_DB",
    ] {
        let value = nonempty_env(name);
        hash_daemon_identity_field(&mut hasher, name, value.as_deref().map(str::as_bytes));
    }

    DaemonConfigIdentity::from_sha256(format!("{:x}", hasher.finalize()))
        .map_err(anyhow::Error::new)
}

fn hash_daemon_identity_path(
    hasher: &mut Sha256,
    name: &str,
    value: Option<&Path>,
    working_directory: &Path,
) {
    let absolute = value.map(|path| {
        if path.is_absolute() { path.to_path_buf() } else { working_directory.join(path) }
    });
    hash_daemon_identity_field(
        hasher,
        name,
        absolute.as_deref().map(|path| path.as_os_str().as_bytes()),
    );
}

fn hash_daemon_identity_field(hasher: &mut Sha256, name: &str, value: Option<&[u8]>) {
    let name = name.as_bytes();
    hasher.update((name.len() as u64).to_le_bytes());
    hasher.update(name);
    match value {
        Some(value) => {
            hasher.update([1]);
            hasher.update((value.len() as u64).to_le_bytes());
            hasher.update(value);
        },
        None => hasher.update([0]),
    }
}

fn hash_daemon_identity_file(hasher: &mut Sha256, name: &str, path: &Path) -> Result<()> {
    let mut file = File::open(path)
        .with_context(|| format!("failed to open daemon identity executable {}", path.display()))?;
    let mut executable_hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).with_context(|| {
            format!("failed to hash daemon identity executable {}", path.display())
        })?;
        if read == 0 {
            break;
        }
        let chunk = buffer
            .get(..read)
            .ok_or_else(|| anyhow::anyhow!("daemon identity read exceeded the buffer boundary"))?;
        executable_hasher.update(chunk);
    }
    let digest = executable_hasher.finalize();
    hash_daemon_identity_field(hasher, name, Some(digest.as_slice()));
    Ok(())
}

fn spawn_daemon(args: &Args, paths: &DaemonPaths, executable: &Path) -> Result<()> {
    paths.ensure_private_base_dir().context("failed to prepare private daemon directory")?;
    let log = paths.open_log().context("failed to open private daemon log")?;
    let stderr = log.try_clone().context("failed to duplicate daemon log handle")?;
    let mut command = ProcessCommand::new(executable);
    for argument in daemon_child_arguments(args) {
        command.arg(argument);
    }
    command
        .arg("daemon")
        .env("TELEGRAM_MCP_DAEMON_DIR", &paths.base_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(stderr));
    command.process_group(0);
    command.spawn().context("failed to spawn persistent Telegram MCP daemon")?;
    Ok(())
}

fn daemon_child_arguments(args: &Args) -> Vec<std::ffi::OsString> {
    let mut arguments = Vec::new();
    if args.demo {
        arguments.push("--demo".into());
    }
    push_path_argument(&mut arguments, "--session-path", args.session_path.as_ref());
    push_path_argument(&mut arguments, "--session-cache-dir", args.session_cache_dir.as_ref());
    push_path_argument(
        &mut arguments,
        "--attachment-cache-dir",
        args.attachment_cache_dir.as_ref(),
    );
    arguments.push("--max-download-bytes".into());
    arguments.push(args.max_download_bytes.to_string().into());
    arguments.push("--max-upload-bytes".into());
    arguments.push(args.max_upload_bytes.to_string().into());
    push_path_argument(&mut arguments, "--state-db-path", args.state_db_path.as_ref());
    push_path_argument(
        &mut arguments,
        "--message-cache-db-path",
        args.message_cache_db_path.as_ref(),
    );
    if let Some(max_age_days) = args.message_cache_max_age_days {
        arguments.push("--message-cache-max-age-days".into());
        arguments.push(max_age_days.to_string().into());
    }
    if let Some(api_id) = args.api_id {
        arguments.push("--api-id".into());
        arguments.push(api_id.to_string().into());
    }
    arguments
}

fn push_path_argument(
    arguments: &mut Vec<std::ffi::OsString>,
    name: &str,
    value: Option<&PathBuf>,
) {
    if let Some(value) = value {
        arguments.push(name.into());
        arguments.push(value.as_os_str().to_owned());
    }
}

fn parse_message_cache_max_age_days(value: &str) -> Result<u32, String> {
    let days = value
        .parse::<u32>()
        .map_err(|_| "message-cache max age must be a whole number of days".to_string())?;
    if days == 0 || days > MAX_MESSAGE_CACHE_AGE_DAYS {
        return Err(format!(
            "message-cache max age must be between 1 and {MAX_MESSAGE_CACHE_AGE_DAYS} days"
        ));
    }
    Ok(days)
}

fn approval_from_env() -> Result<Option<(ApprovalPolicy, Arc<dyn ApprovalBot>)>> {
    let token = nonempty_env("TELEGRAM_MCP_APPROVAL_BOT_TOKEN");
    let chat_id = nonempty_env("TELEGRAM_MCP_APPROVAL_CHAT_ID");
    let user_id = nonempty_env("TELEGRAM_MCP_APPROVAL_USER_ID");
    if token.is_none() && chat_id.is_none() && user_id.is_none() {
        return Ok(None);
    }
    let token = token.context("TELEGRAM_MCP_APPROVAL_BOT_TOKEN is required")?;
    let approval_chat_id = parse_env_i64(
        "TELEGRAM_MCP_APPROVAL_CHAT_ID",
        chat_id.context("TELEGRAM_MCP_APPROVAL_CHAT_ID is required")?,
    )?;
    let approval_user_id = parse_env_i64(
        "TELEGRAM_MCP_APPROVAL_USER_ID",
        user_id.context("TELEGRAM_MCP_APPROVAL_USER_ID is required")?,
    )?;
    let ttl_seconds =
        optional_env_u64("TELEGRAM_MCP_APPROVAL_TTL_SECONDS")?.unwrap_or(24 * 60 * 60);
    let poll_timeout_seconds = optional_env_u64("TELEGRAM_MCP_APPROVAL_POLL_SECONDS")?.unwrap_or(5);
    let api_base = nonempty_env("TELEGRAM_MCP_APPROVAL_API_BASE")
        .unwrap_or_else(|| "https://api.telegram.org".to_string());
    let proxy = nonempty_env("TELEGRAM_MCP_PROXY");
    let policy =
        ApprovalPolicy { approval_chat_id, approval_user_id, ttl_seconds, poll_timeout_seconds };
    policy.validate().map_err(anyhow::Error::msg)?;
    let bot: Arc<dyn ApprovalBot> =
        if let Some(path) = nonempty_env("TELEGRAM_MCP_APPROVAL_FANOUT_DB").map(PathBuf::from) {
            Arc::new(
                FanoutApprovalBot::new(token, api_base, proxy, path)
                    .map_err(|error| anyhow::anyhow!(error.to_string()))?,
            )
        } else {
            Arc::new(
                BotApiApprovalBot::new(token, api_base, proxy)
                    .map_err(|error| anyhow::anyhow!(error.to_string()))?,
            )
        };
    Ok(Some((policy, bot)))
}

fn nonempty_env(name: &str) -> Option<String> {
    std::env::var(name).ok().map(|value| value.trim().to_string()).filter(|value| !value.is_empty())
}

fn parse_env_i64(name: &str, value: String) -> Result<i64> {
    value.parse::<i64>().with_context(|| format!("{name} must be a signed integer"))
}

fn optional_env_u64(name: &str) -> Result<Option<u64>> {
    nonempty_env(name)
        .map(|value| value.parse::<u64>().with_context(|| format!("{name} must be an integer")))
        .transpose()
}

fn telegram_config(args: &Args) -> Result<Config> {
    let api_id = args
        .api_id
        .filter(|value| *value > 0)
        .context("--api-id or TELEGRAM_API_ID must be a positive integer")?;
    let api_hash = std::env::var("TELEGRAM_API_HASH")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .context("TELEGRAM_API_HASH is required")?
        .to_string();
    let proxy = std::env::var("TELEGRAM_MCP_PROXY")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    Ok(Config {
        session_path: args.session_path.clone(),
        session_cache_dir: args.session_cache_dir.clone(),
        proxy,
        api_id,
        api_hash,
    })
}

fn session_selection_config(args: &Args) -> Config {
    Config {
        session_path: args.session_path.clone(),
        session_cache_dir: args.session_cache_dir.clone(),
        proxy: None,
        api_id: 0,
        api_hash: String::new(),
    }
}

async fn doctor(config: Config) -> Result<()> {
    let client = TelegramClient::new(config);
    let (account, session_path) = client.get_me(None).await?;
    let report = DoctorReport {
        status: "ready",
        session_path: session_path.display().to_string(),
        user_id: account.user_id,
        first_name: account.first_name,
        username: account.username,
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn list_sessions(config: &Config) -> Result<()> {
    let candidates = config
        .session_candidates()?
        .into_iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();
    let report = SessionListReport { candidate_count: candidates.len(), candidates };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

async fn auth_login(config: Config) -> Result<()> {
    ensure_interactive_terminal()?;
    let client = TelegramClient::new(config);
    let login = client.begin_login().await?;
    let status = if login.is_authorized().await? {
        "already_authorized"
    } else {
        let phone = normalize_phone(&prompt_line(
            "Phone number in international format (for example +14155550132): ",
        )?)
        .map_err(anyhow::Error::msg)?;
        if let LoginRequestOutcome::CodeRequired(challenge) =
            login.request_login_code(&phone).await?
        {
            eprintln!("Telegram sent a login code. The code will not be echoed.");

            let code = Zeroizing::new(prompt_secret("Login code: ")?);
            if code.is_empty() {
                bail!("login code cannot be empty");
            }
            match login.sign_in(&challenge, code.as_str()).await? {
                LoginCodeOutcome::Authorized => {},
                LoginCodeOutcome::PasswordRequired(challenge) => {
                    eprintln!("Two-step verification is enabled. The password will not be echoed.");
                    let password = Zeroizing::new(prompt_secret("2FA password: ")?);
                    if password.is_empty() {
                        bail!("2FA password cannot be empty");
                    }
                    login.check_password(challenge, password.as_bytes()).await?;
                },
            }
        }
        login.save_session_or_revoke().await?;
        "authenticated"
    };

    let account = login.get_me().await?;
    let report = AuthLoginReport {
        status,
        session_path: login.path().display().to_string(),
        user_id: account.user_id,
        first_name: account.first_name,
        username: account.username,
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn ensure_interactive_terminal() -> Result<()> {
    if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
        bail!(
            "auth login requires an interactive terminal; login codes and 2FA passwords are not accepted through pipes or command arguments"
        );
    }
    Ok(())
}

fn prompt_line(prompt: &str) -> Result<String> {
    eprint!("{prompt}");
    io::stderr().flush()?;
    let mut value = String::new();
    let bytes = io::stdin().read_line(&mut value)?;
    if bytes == 0 {
        bail!("interactive input ended before login completed");
    }
    Ok(value.trim().to_string())
}

fn prompt_secret(prompt: &str) -> Result<String> {
    eprint!("{prompt}");
    io::stderr().flush()?;
    rpassword::read_password().context("failed to read hidden terminal input")
}

fn normalize_phone(raw: &str) -> std::result::Result<String, &'static str> {
    let value = raw.trim();
    if !value.starts_with('+') {
        return Err("phone number must start with + and include the country code");
    }

    let mut digits = String::new();
    for character in value.chars().skip(1) {
        match character {
            '0'..='9' => digits.push(character),
            ' ' | '-' | '(' | ')' => {},
            _ => return Err("phone number contains unsupported characters"),
        }
    }
    if !(7..=15).contains(&digits.len()) {
        return Err("phone number must contain between 7 and 15 digits");
    }
    Ok(format!("+{digits}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn phone_parser_normalizes_common_formatting() {
        assert_eq!(normalize_phone(" +1 (415) 555-0132 "), Ok("+14155550132".to_string()));
    }

    #[test]
    fn phone_parser_requires_international_format() {
        assert_eq!(
            normalize_phone("4155550132"),
            Err("phone number must start with + and include the country code")
        );
    }

    #[test]
    fn phone_parser_rejects_letters_and_impossible_lengths() {
        assert_eq!(
            normalize_phone("+1-800-FLOWERS"),
            Err("phone number contains unsupported characters")
        );
        assert_eq!(
            normalize_phone("+123"),
            Err("phone number must contain between 7 and 15 digits")
        );
    }

    #[test]
    fn message_cache_retention_parser_is_bounded_and_forwarded_to_daemon() {
        let args = Args::try_parse_from([
            "telegram-personal-mcp",
            "--demo",
            "--message-cache-max-age-days",
            "30",
            "serve",
        ])
        .expect("valid retention CLI");
        assert_eq!(args.message_cache_max_age_days, Some(30));
        let child = daemon_child_arguments(&args)
            .into_iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(
            child.windows(2).any(|pair| pair == ["--message-cache-max-age-days", "30"]),
            "proxy startup must preserve the retention policy for the daemon child"
        );

        for invalid in ["0", "36501", "not-a-number"] {
            assert!(
                Args::try_parse_from([
                    "telegram-personal-mcp",
                    "--message-cache-max-age-days",
                    invalid,
                    "serve",
                ])
                .is_err(),
                "{invalid:?} must be rejected"
            );
        }
    }

    #[test]
    fn daemon_identity_binds_the_exact_executable_bytes() {
        let temporary = tempdir().expect("temporary directory");
        let first = temporary.path().join("first");
        let second = temporary.path().join("second");
        std::fs::write(&first, b"generation-a").expect("first executable fixture");
        std::fs::write(&second, b"generation-b").expect("second executable fixture");

        let mut first_identity = Sha256::new();
        hash_daemon_identity_file(&mut first_identity, "executable_sha256", &first)
            .expect("first identity");
        let mut repeated_identity = Sha256::new();
        hash_daemon_identity_file(&mut repeated_identity, "executable_sha256", &first)
            .expect("repeated identity");
        let mut second_identity = Sha256::new();
        hash_daemon_identity_file(&mut second_identity, "executable_sha256", &second)
            .expect("second identity");

        assert_eq!(first_identity.finalize(), repeated_identity.finalize());
        assert_ne!(Sha256::digest(b"generation-a"), Sha256::digest(b"generation-b"));
        assert_ne!(
            {
                let mut value = Sha256::new();
                hash_daemon_identity_file(&mut value, "executable_sha256", &first)
                    .expect("first comparison identity");
                value.finalize()
            },
            second_identity.finalize()
        );
    }

    #[test]
    fn explicit_daemon_executable_is_absolute_and_byte_identified() {
        let temporary = tempdir().expect("temporary directory");
        let executable = temporary.path().join("compatible-daemon");
        std::fs::write(&executable, b"compatible-generation").expect("daemon executable fixture");
        let args = Args::try_parse_from([
            "telegram-personal-mcp",
            "--demo",
            "--daemon-executable",
            executable.to_str().expect("UTF-8 fixture path"),
            "serve",
        ])
        .expect("explicit daemon executable");

        assert_eq!(selected_daemon_executable(&args).expect("selected daemon"), executable);
        assert!(daemon_config_identity(&args, &executable).is_ok());

        let relative = Args::try_parse_from([
            "telegram-personal-mcp",
            "--daemon-executable",
            "relative-daemon",
            "serve",
        ])
        .expect("relative path is parsed before semantic validation");
        assert!(selected_daemon_executable(&relative).is_err());
    }
}
