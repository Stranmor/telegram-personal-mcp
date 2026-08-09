use std::io::{self, Read};

use anyhow::{Context, Result, bail};
use clap::Parser;
use telegram_personal_mcp::{ApprovalBot, BotApiApprovalBot};

#[derive(Debug, Parser)]
#[command(about = "Send one native Telegram Rich Message through the configured operator bot")]
struct Args {
    #[arg(long, env = "TELEGRAM_MCP_APPROVAL_CHAT_ID")]
    chat_id: i64,
    #[arg(
        long,
        env = "TELEGRAM_MCP_APPROVAL_API_BASE",
        default_value = "https://api.telegram.org"
    )]
    api_base: String,
    #[arg(long, env = "TELEGRAM_MCP_PROXY")]
    proxy: Option<String>,
    #[arg(long)]
    current_session_approval: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let approval = args.current_session_approval.trim();
    if approval.encode_utf16().count() < 16 || approval.encode_utf16().count() > 512 {
        bail!("current-session approval must concretely describe this exact delivery");
    }
    let token = std::env::var("TELEGRAM_MCP_APPROVAL_BOT_TOKEN")
        .context("TELEGRAM_MCP_APPROVAL_BOT_TOKEN is required")?;
    let mut html = String::new();
    io::stdin().read_to_string(&mut html).context("failed to read Rich Message HTML from stdin")?;
    if html.trim().is_empty() {
        bail!("Rich Message HTML on stdin must not be empty");
    }
    if html.encode_utf16().count() > 32_768 {
        bail!("Rich Message HTML exceeds 32768 UTF-16 code units");
    }
    let bot = BotApiApprovalBot::new_outbound(token, args.api_base, args.proxy)
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    let receipt = bot
        .send_operator_rich_message(args.chat_id, &html)
        .await
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    println!("{}", serde_json::to_string(&receipt)?);
    Ok(())
}
