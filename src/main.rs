mod db;
mod error;
mod models;
mod tools;

use std::sync::Arc;

use anyhow::Result;
use tracing_subscriber::EnvFilter;

use db::Database;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    dotenvy::dotenv().ok();

    let database_url =
        std::env::var("DATABASE_URL").map_err(|_| anyhow::anyhow!("DATABASE_URL is required"))?;

    let tz_name = std::env::var("LOCAL_TIMEZONE")
        .or_else(|_| std::env::var("TZ"))
        .unwrap_or_else(|_| "Asia/Shanghai".to_string());

    let local_timezone: chrono_tz::Tz = tz_name
        .parse()
        .map_err(|_| anyhow::anyhow!("Unknown timezone: {tz_name}"))?;

    let db = Arc::new(Database::new(&database_url, local_timezone).await?);

    let transport = std::env::var("MCP_TRANSPORT")
        .unwrap_or_else(|_| "streamable-http".to_string())
        .to_lowercase();

    tracing::info!("Starting TeslaMate MCP server with transport: {transport}");

    match transport.as_str() {
        "stdio" => tools::serve_stdio(db).await?,
        "streamable-http" => {
            let host = std::env::var("MCP_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
            let port: u16 = std::env::var("MCP_PORT")
                .unwrap_or_else(|_| "8000".to_string())
                .parse()
                .map_err(|_| anyhow::anyhow!("MCP_PORT must be a valid port number"))?;
            tools::serve_http(db, &host, port).await?
        }
        "sse" => {
            let host = std::env::var("MCP_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
            let port: u16 = std::env::var("MCP_PORT")
                .unwrap_or_else(|_| "8000".to_string())
                .parse()
                .map_err(|_| anyhow::anyhow!("MCP_PORT must be a valid port number"))?;
            tools::serve_sse(db, &host, port).await?
        }
        _ => {
            anyhow::bail!("MCP_TRANSPORT must be one of: stdio, streamable-http, sse");
        }
    }

    Ok(())
}
