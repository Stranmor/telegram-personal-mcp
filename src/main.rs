use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use rmcp::ServiceExt;
use serde::Serialize;
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use std::sync::Arc;
use telegram_personal_mcp::{
    Config, FakeTelegramBackend, LoginCodeOutcome, McpServer, McpServerSettings, TelegramBackend,
    TelegramClient,
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
        default_value_t = 64 * 1024 * 1024
    )]
    max_download_bytes: u64,
    #[arg(
        long,
        env = "TELEGRAM_MCP_MAX_UPLOAD_BYTES",
        default_value_t = 64 * 1024 * 1024
    )]
    max_upload_bytes: u64,
    #[arg(long, env = "TELEGRAM_MCP_STATE_DB")]
    state_db_path: Option<PathBuf>,
    #[arg(long, env = "TELEGRAM_API_ID", global = true)]
    api_id: Option<i32>,
}

#[derive(Debug, Subcommand)]
enum Command {
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
    if args.demo && args.command.is_some() {
        bail!("--demo starts the MCP server and cannot be combined with a CLI subcommand");
    }
    match &args.command {
        Some(Command::Doctor) => return doctor(telegram_config(&args)?).await,
        Some(Command::Session { command: SessionCommand::List }) => {
            return list_sessions(&session_selection_config(&args));
        },
        Some(Command::Auth { command: AuthCommand::Login }) => {
            return auth_login(telegram_config(&args)?).await;
        },
        None => {},
    }

    let backend: Arc<dyn TelegramBackend> = if args.demo {
        info!("using deterministic demo backend; Telegram will not be contacted");
        Arc::new(FakeTelegramBackend::fixture())
    } else {
        Arc::new(TelegramClient::new(telegram_config(&args)?))
    };
    let default_settings = McpServerSettings::default();
    let server = McpServer::with_settings(
        backend,
        McpServerSettings {
            attachment_cache_dir: args
                .attachment_cache_dir
                .clone()
                .unwrap_or(default_settings.attachment_cache_dir),
            max_download_bytes: args.max_download_bytes,
            max_upload_bytes: args.max_upload_bytes,
            state_db_path: args.state_db_path.clone().unwrap_or(default_settings.state_db_path),
        },
    );

    info!("starting MCP 2025-11-25 server on stdio");
    server.serve(rmcp::transport::stdio()).await?.waiting().await?;

    Ok(())
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
        let challenge = login.request_login_code(&phone).await?;
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
}
