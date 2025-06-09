mod types;
mod server;
mod config;

use anyhow::Result;
use std::net::SocketAddr;
use crate::config::WebRtcConfig;

#[tokio::main]
async fn main() -> Result<()> {
    println!("🖥️ 启动远程控制服务端...");
    
    // 加载配置
    let config = WebRtcConfig::load_from_file("webrtc-config.yaml")?;
    let https_config = &config.https_config;
    
    if https_config.enabled {
        println!("🔒 HTTPS模式已启用");
        println!("📡 HTTP服务器地址: http://127.0.0.1:{}", https_config.http_port);
        println!("📡 HTTPS服务器地址: https://127.0.0.1:{}", https_config.https_port);
        
        // 使用HTTP端口作为主端口启动
        let addr: SocketAddr = format!("127.0.0.1:{}", https_config.http_port).parse()?;
        server::start_server(addr).await?;
    } else {
        println!("🌐 使用HTTP模式");
        println!("📡 服务器地址: http://127.0.0.1:3000");
        
        let addr: SocketAddr = "127.0.0.1:3000".parse()?;
        server::start_server(addr).await?;
    }
    
    Ok(())
} 