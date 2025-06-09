mod types;
mod server;
mod config;

use anyhow::Result;
use std::net::SocketAddr;
use crate::config::WebRtcConfig;

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化日志
    env_logger::init();
    
    log::info!("🖥️ 启动远程控制服务端...");
    
    // 加载配置
    let config = WebRtcConfig::load_from_file("webrtc-config.yaml")?;
    let https_config = &config.https_config;
    
    if https_config.enabled {
        log::info!("🔒 HTTPS模式已启用");
        
        // 使用HTTP端口作为主端口启动
        let addr: SocketAddr = format!("0.0.0.0:{}", https_config.http_port).parse()?;
        server::start_server(addr).await?;
    } else {
        let addr: SocketAddr = "0.0.0.0:3000".parse()?;
        server::start_server(addr).await?;
    }
    
    Ok(())
} 