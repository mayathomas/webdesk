use std::net::SocketAddr;
use std::sync::Arc;
use clap::{Command, Arg};
use tokio::net::UdpSocket;
use tokio::signal;
use anyhow::Result;

// 使用stun-rs库
use stun_rs::*;
use stun_rs::methods::BINDING;
use stun_rs::attributes::stun::{XorMappedAddress, Software};

const DEFAULT_PORT: u16 = 3478;
const BUFFER_SIZE: usize = 1500;

#[derive(Debug)]
struct StunServerConfig {
    pub bind_addr: SocketAddr,
    pub software: String,
}

impl Default for StunServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: SocketAddr::from(([0, 0, 0, 0], DEFAULT_PORT)),
            software: "stun-server-rs/0.1.0".to_string(),
        }
    }
}

struct StunServer {
    config: StunServerConfig,
    socket: Arc<UdpSocket>,
    encoder: MessageEncoder,
    decoder: MessageDecoder,
}

impl StunServer {
    pub async fn new(config: StunServerConfig) -> Result<Self> {
        let socket = UdpSocket::bind(config.bind_addr).await?;
        println!("🚀 STUN服务器启动在 {}", config.bind_addr);
        
        // 创建编码器和解码器
        let encoder = MessageEncoderBuilder::default().build();
        let decoder = MessageDecoderBuilder::default().build();

        Ok(Self {
            config,
            socket: Arc::new(socket),
            encoder,
            decoder,
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        let mut buffer = [0u8; BUFFER_SIZE];

        loop {
            let (len, src_addr) = self.socket.recv_from(&mut buffer).await?;
            println!("📨 收到来自 {} 的 {} 字节数据", src_addr, len);
            
            if let Err(e) = self.handle_message(&buffer[..len], src_addr).await {
                println!("❌ 处理消息失败: {}", e);
            }
        }
    }

    async fn handle_message(&self, data: &[u8], src_addr: SocketAddr) -> Result<()> {
        // 解码STUN消息
        match self.decoder.decode(data) {
            Ok((message, _)) => {
                println!("✅ 成功解码STUN消息: method={:?}, class={:?}", 
                    message.method(), message.class());
                
                // 检查是否为绑定请求
                if message.method() == BINDING && message.class() == MessageClass::Request {
                    self.handle_binding_request(&message, src_addr).await?;
                } else {
                    println!("⚠️ 不支持的STUN消息类型");
                }
            }
            Err(e) => {
                println!("❌ 解码STUN消息失败: {}", e);
            }
        }
        Ok(())
    }

    async fn handle_binding_request(&self, request: &StunMessage, src_addr: SocketAddr) -> Result<()> {
        println!("🔗 处理来自 {} 的绑定请求", src_addr);

        // 构建绑定成功响应
        let response = StunMessageBuilder::new(
            BINDING,
            MessageClass::SuccessResponse,
        )
        .with_transaction_id(*request.transaction_id())
        .with_attribute(XorMappedAddress::from(src_addr))
        .with_attribute(Software::try_from(self.config.software.clone())?)
        .build();

        // 编码响应消息
        let mut response_buffer = [0u8; BUFFER_SIZE];
        let response_len = self.encoder.encode(&mut response_buffer, &response)?;
        
        // 发送响应
        self.socket.send_to(&response_buffer[..response_len], src_addr).await?;
        println!("✅ 已发送 {} 字节响应到 {}", response_len, src_addr);
        
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let matches = Command::new("STUN Server")
        .version("0.1.0")
        .author("您的名字 <your@email.com>")
        .about("一个生产级的STUN服务器")
        .arg(
            Arg::new("host")
                .long("host")
                .value_name("HOST")
                .help("绑定的主机地址")
                .default_value("0.0.0.0"),
        )
        .arg(
            Arg::new("port")
                .short('p')
                .long("port")
                .value_name("PORT")
                .help("绑定的端口")
                .default_value("3478"),
        )
        .arg(
            Arg::new("verbose")
                .short('v')
                .long("verbose")
                .help("启用详细日志")
                .action(clap::ArgAction::SetTrue),
        )
        .get_matches();

    // 设置日志级别
    if matches.get_flag("verbose") {
        unsafe { std::env::set_var("RUST_LOG", "debug"); }
    } else if std::env::var("RUST_LOG").is_err() {
        unsafe { std::env::set_var("RUST_LOG", "info"); }
    }
    env_logger::init();

    // 解析命令行参数
    let host: &String = matches.get_one("host").unwrap();
    let port: &String = matches.get_one("port").unwrap();
    let bind_addr: SocketAddr = format!("{}:{}", host, port).parse()?;

    let config = StunServerConfig {
        bind_addr,
        software: "stun-server-rs/0.1.0".to_string(),
    };

    // 启动服务器
    let mut server = StunServer::new(config).await?;
    
    // 设置优雅关闭
    tokio::select! {
        result = server.run() => {
            if let Err(e) = result {
                println!("❌ 服务器错误: {}", e);
            }
        }
        _ = signal::ctrl_c() => {
            println!("🛑 收到停止信号，正在关闭服务器...");
        }
    }

    println!("👋 STUN服务器已关闭");
    Ok(())
}
