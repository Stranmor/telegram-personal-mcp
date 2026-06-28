use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use std::sync::Arc;
use telegram_personal_mcp::{Config, McpServer, TelegramClient};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, error, info};

#[derive(Debug, Parser)]
#[command(author, version, about)]
struct Args {
    #[arg(long, env = "TELEGRAM_MCP_SESSION_PATH")]
    session_path: Option<PathBuf>,
    #[arg(long, env = "TELEGRAM_MCP_SESSION_CACHE_DIR")]
    session_cache_dir: Option<PathBuf>,
    #[arg(long, env = "TELEGRAM_MCP_PROXY")]
    proxy: Option<String>,
    #[arg(long, env = "TELEGRAM_API_ID")]
    api_id: i32,
    #[arg(long, env = "TELEGRAM_API_HASH")]
    api_hash: String,
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
    let config = Config {
        session_path: args.session_path,
        session_cache_dir: args.session_cache_dir,
        proxy: args.proxy,
        api_id: args.api_id,
        api_hash: args.api_hash,
    };
    let client = Arc::new(TelegramClient::new(config));
    let server = Arc::new(McpServer::new(client));

    info!("starting newline-delimited JSON-RPC MCP server on stdio");
    let mut stdin = BufReader::new(tokio::io::stdin());
    let mut stdout = tokio::io::stdout();
    let mut line = String::new();

    loop {
        line.clear();
        let bytes = stdin.read_line(&mut line).await?;
        if bytes == 0 {
            break;
        }
        let request = line.trim();
        if request.is_empty() {
            continue;
        }

        debug!("received JSON-RPC request");
        match server.handle_request(request).await {
            Ok(Some(response)) => {
                stdout.write_all(response.as_bytes()).await?;
                stdout.write_all(b"\n").await?;
                stdout.flush().await?;
            },
            Ok(None) => {},
            Err(error) => error!(%error, "request failed"),
        }
    }

    Ok(())
}
