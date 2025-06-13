mod types;
mod server;

use anyhow::Result;
use clap::Parser;
use std::net::SocketAddr;

#[derive(Parser)]
#[command(name = "signals-server")]
#[command(about = "WebRTC信令服务器")]
struct Args {
    /// 监听地址
    #[arg(short, long, default_value = "0.0.0.0:3476")]
    addr: SocketAddr,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    
    let args = Args::parse();
    
    log::info!("🚀 启动WebRTC信令服务器...");
    log::info!("📡 监听地址: {}", args.addr);
    
    server::start_signals_server(args.addr).await?;
    
    Ok(())
} 