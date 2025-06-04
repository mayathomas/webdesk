mod types;
mod server;

use anyhow::Result;
use std::net::SocketAddr;

#[tokio::main]
async fn main() -> Result<()> {
    println!("🖥️ 启动远程控制服务端...");
    
    // 使用固定配置 - 只需要一个端口
    let host = "127.0.0.1";
    let port = 8080;
    
    println!("📡 服务器地址: http://{}:{}", host, port);
    println!("💡 打开浏览器访问上面的地址即可开始远程控制");
    
    // 启动合并的HTTP和WebSocket服务器
    let addr: SocketAddr = format!("{}:{}", host, port).parse()?;
    server::start_server(addr).await?;
    
    Ok(())
} 