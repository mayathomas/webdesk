use std::net::SocketAddr;
use tokio::net::UdpSocket;
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    println!("🧪 Simple STUN Test");
    
    // 手动构建一个最简单的STUN Binding Request消息
    let mut stun_request = Vec::new();
    
    // STUN消息头 (20字节)
    // 消息类型: 0x0001 (Binding Request)
    stun_request.extend_from_slice(&0x0001u16.to_be_bytes());
    
    // 消息长度: 0 (没有属性)
    stun_request.extend_from_slice(&0x0000u16.to_be_bytes());
    
    // Magic Cookie: 0x2112A442
    stun_request.extend_from_slice(&0x2112A442u32.to_be_bytes());
    
    // Transaction ID: 12字节随机数
    let transaction_id: [u8; 12] = [
        0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC,
        0xDE, 0xF0, 0x11, 0x22, 0x33, 0x44
    ];
    stun_request.extend_from_slice(&transaction_id);
    
    println!("📦 Built STUN request: {} bytes", stun_request.len());
    println!("📦 Raw data: {:02x?}", stun_request);
    
    // 创建UDP socket
    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    let local_addr = socket.local_addr()?;
    println!("📍 Local address: {}", local_addr);
    
    // 服务器地址
    let server_addr: SocketAddr = "127.0.0.1:3477".parse()?;
    println!("🎯 Target server: {}", server_addr);
    
    // 发送请求
    println!("📤 Sending request...");
    socket.send_to(&stun_request, server_addr).await?;
    
    // 接收响应
    let mut buffer = vec![0u8; 1500];
    match tokio::time::timeout(
        std::time::Duration::from_secs(5),
        socket.recv_from(&mut buffer)
    ).await {
        Ok(Ok((size, from_addr))) => {
            println!("📥 Received {} bytes from {}", size, from_addr);
            println!("📥 Raw response: {:02x?}", &buffer[..size]);
            
            // 简单解析响应
            if size >= 20 {
                let msg_type = u16::from_be_bytes([buffer[0], buffer[1]]);
                let msg_length = u16::from_be_bytes([buffer[2], buffer[3]]);
                let magic_cookie = u32::from_be_bytes([buffer[4], buffer[5], buffer[6], buffer[7]]);
                let response_transaction_id = &buffer[8..20];
                
                println!("✅ Response parsed:");
                println!("   Message type: 0x{:04x}", msg_type);
                println!("   Message length: {}", msg_length);
                println!("   Magic cookie: 0x{:08x}", magic_cookie);
                println!("   Transaction ID matches: {}", response_transaction_id == transaction_id);
                
                if msg_type == 0x0101 {
                    println!("🎉 Received Binding Success Response!");
                    
                    // 解析属性
                    if msg_length > 0 {
                        let mut offset = 20;
                        while offset + 4 <= size && offset < 20 + msg_length as usize {
                            let attr_type = u16::from_be_bytes([buffer[offset], buffer[offset + 1]]);
                            let attr_length = u16::from_be_bytes([buffer[offset + 2], buffer[offset + 3]]) as usize;
                            
                            println!("   Attribute: type=0x{:04x}, length={}", attr_type, attr_length);
                            
                            if attr_type == 0x0020 && attr_length >= 8 { // XOR-MAPPED-ADDRESS
                                let family = u16::from_be_bytes([buffer[offset + 5], buffer[offset + 6]]);
                                let xor_port = u16::from_be_bytes([buffer[offset + 6], buffer[offset + 7]]);
                                let port = xor_port ^ 0x2112;
                                
                                if family == 0x01 && attr_length == 8 { // IPv4
                                    let xor_ip = u32::from_be_bytes([
                                        buffer[offset + 8], buffer[offset + 9],
                                        buffer[offset + 10], buffer[offset + 11]
                                    ]);
                                    let ip = xor_ip ^ 0x2112A442;
                                    let ip_bytes = ip.to_be_bytes();
                                    
                                    println!("🌍 XOR-MAPPED-ADDRESS: {}.{}.{}.{}:{}", 
                                             ip_bytes[0], ip_bytes[1], ip_bytes[2], ip_bytes[3], port);
                                }
                            }
                            
                            // 移动到下一个属性（考虑padding）
                            let padded_length = (attr_length + 3) & !3;
                            offset += 4 + padded_length;
                        }
                    }
                }
            }
        }
        Ok(Err(e)) => {
            println!("❌ Socket error: {}", e);
        }
        Err(_) => {
            println!("⏰ Timeout waiting for response");
        }
    }
    
    Ok(())
} 