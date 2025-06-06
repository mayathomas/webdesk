use anyhow::Result;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::APIBuilder;
use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::data_channel::data_channel_state::RTCDataChannelState;
use webrtc::data_channel::RTCDataChannel;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::ice_transport::ice_credential_type::RTCIceCredentialType;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::interceptor::registry::Registry;
use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;

use crate::types::*;
use serde_json;

/// WebRTC客户端状态
#[derive(Clone)]
pub struct WebRTCClient {
    pub peer_connection: Arc<RTCPeerConnection>,
    pub data_channel: Arc<Mutex<Option<Arc<RTCDataChannel>>>>,
    pub signaling_tx: mpsc::UnboundedSender<WebSocketMessage>,
    pub client_id: String,
    pub data_channel_ready_tx: Option<mpsc::UnboundedSender<()>>,
}

impl WebRTCClient {
    /// 创建新的WebRTC客户端
    pub async fn new(
        client_id: String,
        signaling_tx: mpsc::UnboundedSender<WebSocketMessage>,
        data_channel_ready_tx: Option<mpsc::UnboundedSender<()>>,
    ) -> Result<Self> {
        println!("🌐 初始化WebRTC客户端...");
        
        // 创建媒体引擎
        let mut media_engine = MediaEngine::default();
        
        // 注册默认编解码器
        media_engine.register_default_codecs()?;
        
        // 创建拦截器注册表
        let mut registry = Registry::new();
        registry = register_default_interceptors(registry, &mut media_engine)?;
        
        // 创建API
        let api = APIBuilder::new()
            .with_media_engine(media_engine)
            .with_interceptor_registry(registry)
            .build();
        
        // ICE服务器配置：包含STUN和TURN服务器
        let ice_servers = vec![
            // Google公共STUN服务器
            RTCIceServer {
                urls: vec!["stun:stun.l.google.com:19302".to_owned()],
                username: "".to_owned(),
                credential: "".to_owned(),
                credential_type: RTCIceCredentialType::Unspecified,
            },
            // Cloudflare公共STUN服务器  
            RTCIceServer {
                urls: vec!["stun:stun.cloudflare.com:3478".to_owned()],
                username: "".to_owned(),
                credential: "".to_owned(),
                credential_type: RTCIceCredentialType::Unspecified,
            },
            // 我们自己的TURN服务器（优先使用）
            RTCIceServer {
                urls: vec!["turn:127.0.0.1:3478".to_owned()],
                username: "maya".to_owned(),
                credential: "sorrow2713".to_owned(),
                credential_type: RTCIceCredentialType::Password,
            },
        ];
        
        println!("📋 ICE服务器配置完成: {} 个STUN服务器", ice_servers.len());
        
        // 创建PeerConnection
        let peer_connection = Arc::new(api.new_peer_connection(RTCConfiguration {
            ice_servers,
            ..Default::default()
        }).await?);
        
        println!("✅ WebRTC PeerConnection 已创建");
        
        Ok(Self {
            peer_connection,
            data_channel: Arc::new(Mutex::new(None)),
            signaling_tx,
            client_id,
            data_channel_ready_tx,
        })
    }
    
    /// 设置WebRTC事件监听器
    pub async fn setup_handlers(&mut self) -> Result<()> {
        let client_id = self.client_id.clone();
        let signaling_tx = self.signaling_tx.clone();
        
        // 监听连接状态变化
        self.peer_connection.on_peer_connection_state_change(Box::new(move |state: RTCPeerConnectionState| {
            println!("🔗 WebRTC连接状态变化: {:?}", state);
            match state {
                RTCPeerConnectionState::Connected => {
                    println!("🎉 WebRTC P2P连接已建立！数据通道应该可用了！");
                }
                RTCPeerConnectionState::Connecting => {
                    println!("🔄 WebRTC正在连接...");
                }
                RTCPeerConnectionState::Disconnected => {
                    println!("⚠️ WebRTC连接已断开");
                }
                RTCPeerConnectionState::Failed => {
                    println!("❌ WebRTC连接失败");
                }
                RTCPeerConnectionState::Closed => {
                    println!("🔐 WebRTC连接已关闭");
                }
                _ => {
                    println!("🔍 WebRTC连接状态: {:?}", state);
                }
            }
            Box::pin(async {})
        }));
        
        // 监听ICE连接状态变化
        self.peer_connection.on_ice_connection_state_change(Box::new(move |state| {
            println!("🧊 ICE连接状态变化: {:?}", state);
            match state {
                webrtc::ice_transport::ice_connection_state::RTCIceConnectionState::Connected => {
                    println!("🎉 ICE连接已建立！P2P通道打开！");
                }
                webrtc::ice_transport::ice_connection_state::RTCIceConnectionState::Completed => {
                    println!("✅ ICE连接完成！最佳路径已选择！");
                }
                webrtc::ice_transport::ice_connection_state::RTCIceConnectionState::Failed => {
                    println!("❌ ICE连接失败！P2P无法建立！");
                    println!("💡 可能原因：");
                    println!("   1. 严格的NAT/防火墙阻止P2P连接");
                    println!("   2. 需要TURN服务器中继");
                    println!("   3. 网络策略限制");
                }
                webrtc::ice_transport::ice_connection_state::RTCIceConnectionState::Disconnected => {
                    println!("⚠️ ICE连接断开");
                }
                webrtc::ice_transport::ice_connection_state::RTCIceConnectionState::Checking => {
                    println!("🔍 ICE正在检查连通性...");
                }
                _ => {
                    println!("🔍 ICE连接状态: {:?}", state);
                }
            }
            Box::pin(async {})
        }));
        
        // 监听ICE收集状态变化
        self.peer_connection.on_ice_gathering_state_change(Box::new(move |state| {
            println!("📡 ICE收集状态变化: {:?}", state);
            Box::pin(async {})
        }));
        
        // 监听ICE候选
        let signaling_tx_clone = signaling_tx.clone();
        let client_id_clone = client_id.clone();
        self.peer_connection.on_ice_candidate(Box::new(move |candidate| {
            let signaling_tx = signaling_tx_clone.clone();
            let client_id = client_id_clone.clone();
            
            Box::pin(async move {
                if let Some(candidate) = candidate {
                    // webrtc-rs的candidate.to_string()返回格式如："udp host 192.168.1.3:49796"
                    let candidate_string = candidate.to_string();
                    println!("🧊 收集到ICE候选: {}", candidate_string);
                    
                    // 解析并重新构造标准的SDP候选格式
                    let formatted_candidate = parse_and_format_candidate(&candidate_string);
                    println!("🧊 发送标准SDP候选: {}", formatted_candidate);
                    
                    let ice_msg = WebSocketMessage::WebRTCIceCandidate {
                        target_id: client_id,
                        ice_candidate: IceCandidate {
                            candidate: formatted_candidate,
                            // 数据通道通常使用这些默认值
                            sdp_mid: Some("0".to_string()),
                            sdp_mline_index: Some(0),
                        },
                    };
                    
                    if let Err(e) = signaling_tx.send(ice_msg) {
                        println!("❌ 发送ICE候选失败: {}", e);
                    }
                } else {
                    println!("🏁 ICE候选收集完成");
                }
            })
        }));
        
                // 监听数据通道（浏览器端创建的数据通道）
        let data_channel_ref = self.data_channel.clone();
        let ready_tx_clone = self.data_channel_ready_tx.clone();
        
        self.peer_connection.on_data_channel(Box::new(move |data_channel| {
            let data_channel = Arc::clone(&data_channel);
            println!("🎉 收到来自浏览器的数据通道: {} (状态: {:?})", 
                data_channel.label(), data_channel.ready_state());
            
            // 保存数据通道引用
            if let Ok(mut dc_ref) = data_channel_ref.lock() {
                *dc_ref = Some(Arc::clone(&data_channel));
                println!("✅ 数据通道引用已保存到客户端");
            } else {
                println!("❌ 无法保存数据通道引用");
            }
            
            // 设置数据通道监听器
            let dc_clone_for_open = Arc::clone(&data_channel);
            let ready_tx = ready_tx_clone.clone();
            data_channel.on_open(Box::new(move || {
                println!("🚀 数据通道已打开，可以开始双向数据传输！状态: {:?}", 
                    dc_clone_for_open.ready_state());
                println!("🎉 现在可以开始屏幕捕获和传输了！");
                
                // 通知主线程数据通道已就绪
                if let Some(ref tx) = ready_tx {
                    let _ = tx.send(());
                }
                
                Box::pin(async {})
            }));
            
            let dc_clone_for_close = Arc::clone(&data_channel);
            data_channel.on_close(Box::new(move || {
                println!("🔒 数据通道已关闭，状态: {:?}", 
                    dc_clone_for_close.ready_state());
                Box::pin(async {})
            }));
            
            let dc_clone_for_error = Arc::clone(&data_channel);
            data_channel.on_error(Box::new(move |err| {
                println!("❌ 数据通道错误: {:?}, 状态: {:?}", 
                    err, dc_clone_for_error.ready_state());
                Box::pin(async {})
            }));
            
            let dc_clone = Arc::clone(&data_channel);
            data_channel.on_message(Box::new(move |msg| {
                let dc = Arc::clone(&dc_clone);
                Box::pin(async move {
                    println!("📨 收到浏览器数据: {} bytes", msg.data.len());
                    handle_data_channel_message(dc, msg).await;
                })
            }));
            
            Box::pin(async {})
        }));
        
        println!("✅ 数据通道监听器已设置");
        
        Ok(())
    }
    
    /// 处理WebRTC Offer
    pub async fn handle_offer(&mut self, session_description: SessionDescription) -> Result<()> {
        println!("📥 处理WebRTC Offer");
        
        let offer = RTCSessionDescription::offer(session_description.sdp)?;
        self.peer_connection.set_remote_description(offer).await?;
        
        // 注意：数据通道由offer方（浏览器）创建，我们在on_data_channel监听器中接收
        // 不需要在这里创建数据通道
        
        // 创建Answer
        let answer = self.peer_connection.create_answer(None).await?;
        self.peer_connection.set_local_description(answer.clone()).await?;
        
        // 发送Answer
        let answer_msg = WebSocketMessage::WebRTCAnswer {
            target_id: self.client_id.clone(),
            session_description: SessionDescription {
                sdp_type: "answer".to_string(),
                sdp: answer.sdp,
            },
        };
        
        self.signaling_tx.send(answer_msg)?;
        
        println!("📤 发送WebRTC Answer");
        Ok(())
    }
    
    /// 处理ICE候选
    pub async fn handle_ice_candidate(&self, ice_candidate: IceCandidate) -> Result<()> {
        println!("🧊 添加ICE候选: {}", ice_candidate.candidate);
        
        let candidate = RTCIceCandidateInit {
            candidate: ice_candidate.candidate,
            sdp_mid: ice_candidate.sdp_mid,
            sdp_mline_index: ice_candidate.sdp_mline_index,
            username_fragment: None,
        };
        
        self.peer_connection.add_ice_candidate(candidate).await?;
        Ok(())
    }
    
    /// 发送屏幕数据通过WebRTC数据通道
    pub async fn send_screen_data(&self, screen_data: &ScreenData) -> Result<()> {
        // 先获取数据通道的克隆，避免跨await持有锁
        let data_channel = {
            if let Ok(data_channel_guard) = self.data_channel.lock() {
                data_channel_guard.clone()
            } else {
                return Ok(()); // 静默返回，避免日志spam
            }
        };
        
        if let Some(data_channel) = data_channel {
            if data_channel.ready_state() == RTCDataChannelState::Open {
                // 将ScreenData包装为完整的WebSocketMessage
                let message = WebSocketMessage::ScreenData(screen_data.clone());
                let json_data = serde_json::to_string(&message)?;
                let bytes = json_data.into_bytes();
                let total_size = bytes.len();
                
                if total_size <= 16 * 1024 {
                    // 数据较小，直接发送
                    match data_channel.send(&bytes.into()).await {
                        Ok(_) => {
                            println!("📤 通过WebRTC发送屏幕数据: {}x{}, 数据大小: {} bytes", 
                                screen_data.width, screen_data.height, total_size);
                        }
                        Err(e) => {
                            println!("❌ 通过WebRTC发送屏幕数据失败: {}", e);
                            return Err(e.into());
                        }
                    }
                } else {
                    // 数据太大，需要分片发送
                    println!("📦 数据过大({} bytes)，开始分片发送...", total_size);
                    
                    // 根据RFC 8831第6.6节：没有消息交错支持时，发送方应该将最大消息大小限制为16KB
                    // "As long as message interleaving is not supported, the sender SHOULD limit 
                    // the maximum message size to 16 KB to avoid monopolization."
                    const MAX_SAFE_MESSAGE_SIZE: usize = 16 * 1024; // 16KB，符合RFC 8831标准
                    
                    // 估算协议头大小（保守估计）
                    let sample_header = ChunkHeader {
                        message_id: "1234567890123".to_string(),
                        chunk_index: 999,
                        total_chunks: 999,
                        chunk_size: 15000, // 估算值
                        total_size,
                    };
                    let sample_header_json = serde_json::to_string(&sample_header)?;
                    let header_overhead = 2 + sample_header_json.len(); // 2字节长度 + JSON头
                    
                    // 计算每片的净数据空间（留出安全余量）
                    let safety_margin = 100; // 100字节安全余量
                    let net_data_per_chunk = MAX_SAFE_MESSAGE_SIZE - header_overhead - safety_margin;
                    
                    // 计算需要多少个分片
                    let total_chunks = (total_size + net_data_per_chunk - 1) / net_data_per_chunk; // 向上取整
                    
                    // 计算每片平均数据大小
                    let avg_data_per_chunk = total_size / total_chunks;
                    let remainder = total_size % total_chunks;
                    
                    println!("📏 分片计算 (RFC 8831标准):");
                    println!("   - RFC推荐最大消息大小: {} bytes (16KB)", MAX_SAFE_MESSAGE_SIZE);
                    println!("   - 协议头开销: {} bytes", header_overhead);
                    println!("   - 安全余量: {} bytes", safety_margin);
                    println!("   - 每片净数据空间: {} bytes", net_data_per_chunk);
                    println!("   - 总分片数: {}", total_chunks);
                    println!("   - 平均每片数据: {} bytes", avg_data_per_chunk);
                    
                    // 生成唯一的消息ID
                    let message_id = format!("{}", std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_millis());
                    
                    let mut offset = 0;
                    
                    for chunk_index in 0..total_chunks {
                        // 计算当前分片的数据大小（最后几片可能会大一点，分配剩余数据）
                        let current_chunk_size = if chunk_index < remainder {
                            avg_data_per_chunk + 1
                        } else {
                            avg_data_per_chunk
                        };
                        
                        // 确保不会越界
                        let end_offset = std::cmp::min(offset + current_chunk_size, total_size);
                        let actual_chunk_size = end_offset - offset;
                        
                        let chunk_data = &bytes[offset..end_offset];
                        
                        // 创建分片消息头
                        let chunk_header = ChunkHeader {
                            message_id: message_id.clone(),
                            chunk_index,
                            total_chunks,
                            chunk_size: actual_chunk_size,
                            total_size,
                        };
                        
                        // 序列化分片头
                        let header_json = serde_json::to_string(&chunk_header)?;
                        let header_bytes = header_json.as_bytes();
                        
                        // 构建完整的分片消息：[2字节头长度][头数据][分片数据]
                        let header_len = header_bytes.len() as u16;
                        let mut chunk_message = Vec::with_capacity(2 + header_bytes.len() + chunk_data.len());
                        chunk_message.extend_from_slice(&header_len.to_le_bytes());
                        chunk_message.extend_from_slice(header_bytes);
                        chunk_message.extend_from_slice(chunk_data);
                        
                        let total_chunk_size = chunk_message.len();
                        
                        // 验证分片大小
                        if total_chunk_size > MAX_SAFE_MESSAGE_SIZE {
                            println!("❌ 分片 {}/{} 仍然过大 ({} > {} bytes)，算法需要调整", 
                                chunk_index + 1, total_chunks, total_chunk_size, MAX_SAFE_MESSAGE_SIZE);
                            return Err(anyhow::anyhow!("分片算法错误"));
                        }
                        
                        // 发送分片
                        match data_channel.send(&chunk_message.into()).await {
                            Ok(_) => {
                                println!("📤 分片 {}/{} 发送成功 (总: {} bytes，头: {} bytes，数据: {} bytes)", 
                                    chunk_index + 1, total_chunks, total_chunk_size, 
                                    2 + header_bytes.len(), chunk_data.len());
                            }
                            Err(e) => {
                                println!("❌ 分片 {}/{} 发送失败: {}", chunk_index + 1, total_chunks, e);
                                return Err(e.into());
                            }
                        }
                        
                        offset = end_offset;
                        
                        // 在分片之间添加小延迟，避免网络拥塞
                        if chunk_index < total_chunks - 1 {
                            tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
                        }
                    }
                    
                    println!("✅ 分片发送完成: {} 个分片，总大小: {} bytes", total_chunks, total_size);
                }
            } else {
                // 数据通道存在但未就绪，静默返回
                return Ok(());
            }
        } else {
            // 数据通道未初始化，静默返回  
            return Ok(());
        }
        Ok(())
    }
    
    /// 关闭WebRTC连接
    pub async fn close(&self) -> Result<()> {
        println!("🔐 关闭WebRTC连接");
        self.peer_connection.close().await?;
        Ok(())
    }
}

/// 处理数据通道消息（来自浏览器的控制命令）
async fn handle_data_channel_message(_data_channel: Arc<RTCDataChannel>, msg: DataChannelMessage) {
    if let Ok(text) = String::from_utf8(msg.data.to_vec()) {
        if let Ok(message) = serde_json::from_str::<WebSocketMessage>(&text) {
            match message {
                WebSocketMessage::MouseEvent(mouse_event) => {
                    println!("🖱️ 通过WebRTC收到鼠标事件: {:?}", mouse_event);
                    // TODO: 处理鼠标事件
                }
                WebSocketMessage::KeyboardEvent(keyboard_event) => {
                    println!("⌨️ 通过WebRTC收到键盘事件: {:?}", keyboard_event);
                    // TODO: 处理键盘事件
                }
                _ => {}
            }
        }
    }
}

/// 解析webrtc-rs的候选字符串并格式化为标准SDP格式
/// 输入格式: "udp host 192.168.1.3:49796" 或 "udp srflx 121.227.207.147:60352"
/// 输出格式: "candidate:842163049 1 udp 1686052607 192.168.1.3 49796 typ host"
fn parse_and_format_candidate(candidate_str: &str) -> String {
    let parts: Vec<&str> = candidate_str.split_whitespace().collect();
    
    if parts.len() >= 3 {
        let transport = parts[0].to_uppercase(); // UDP/TCP
        let candidate_type = parts[1]; // host/srflx/relay等
        let address_port = parts[2]; // IP:PORT格式
        
        // 解析地址:端口，处理IPv6和webrtc-rs的bug格式
        let (ip, port) = parse_address_port(address_port);
        
        // 如果解析失败（返回空字符串），跳过该候选
        if ip.is_empty() || port.is_empty() {
            println!("⚠️ 跳过无效的ICE候选: {}", candidate_str);
            return String::new(); // 返回空字符串，让上层跳过
        }
        
        // 验证端口是否有效
        if let Ok(port_num) = port.parse::<u16>() {
            // 根据候选类型设置优先级
            let priority = match candidate_type {
                "host" => 2113937151,
                "srflx" => 1677729535, 
                "relay" => 16777215,
                _ => 1000000,
            };
            
            // 构造标准的SDP候选格式
            // 根据RFC 5245和MDN文档，ICE候选的IP地址字段不使用方括号
            format!(
                "candidate:{} {} {} {} {} {} typ {}",
                "foundation", // 基础标识符
                1,           // component-id
                transport,   // UDP/TCP
                priority,    // 根据类型设置的优先级
                ip,          // IP地址（IPv6无需方括号）
                port_num,    // 端口号
                candidate_type // host/srflx/relay
            )
        } else {
            println!("⚠️ 无效的端口号: {}, 跳过候选", port);
            String::new() // 返回空字符串，让上层跳过
        }
    } else {
        println!("⚠️ 候选格式不正确: {}, 跳过", candidate_str);
        format!("candidate:{}", candidate_str)
    }
}

/// 解析地址:端口字符串，处理各种格式，包括webrtc-rs的bug格式
fn parse_address_port(address_port: &str) -> (String, String) {
    println!("🔍 解析地址:端口: {}", address_port);
    
    // 处理webrtc-rs的IPv4 bug: "121.227.207.147:502480.0.0.0"
    // 正确端口应该是50248，不是502480.0.0.0
    if address_port.contains("0.0.0.0") {
        if let Some(zero_pos) = address_port.find("0.0.0.0") {
            let before_zero = &address_port[..zero_pos];
            if let Some(colon_pos) = before_zero.rfind(':') {
                let ip = &before_zero[..colon_pos];
                let port_with_extra = &before_zero[colon_pos + 1..];
                
                // 从port_with_extra中提取正确的端口号
                // 比如从"50248"中提取50248，这里需要找到正确的端口长度
                if port_with_extra.len() >= 5 {
                    let port = &port_with_extra[..5]; // 大多数端口是5位或更少
                    if port.chars().all(|c| c.is_ascii_digit()) {
                        println!("🔨 修复webrtc-rs IPv4 bug: {} -> {}:{}", address_port, ip, port);
                        return (ip.to_string(), port.to_string());
                    }
                }
            }
        }
    }
    
    // 处理webrtc-rs的IPv6 bug: "240e:3a3:4c35:c9a0:b01f:76db:88b8:7e1f:50254::"
    // 这种格式中最后的数字部分应该是端口
    if address_port.ends_with("::") && address_port.contains(':') {
        let without_double_colon = &address_port[..address_port.len() - 2];
        if let Some(last_colon_pos) = without_double_colon.rfind(':') {
            let potential_port = &without_double_colon[last_colon_pos + 1..];
            if potential_port.chars().all(|c| c.is_ascii_digit()) && potential_port.len() <= 5 {
                let ip_part = &without_double_colon[..last_colon_pos];
                println!("🔨 修复webrtc-rs IPv6 bug: {} -> IPv6 {}:{}", address_port, ip_part, potential_port);
                return (ip_part.to_string(), potential_port.to_string()); // 不加方括号，让webrtc-rs库自己处理
            }
        }
    }
    
    // 处理标准IPv6格式 [ip]:port
    if address_port.starts_with('[') {
        if let Some(bracket_pos) = address_port.find("]:") {
            let ip = address_port[1..bracket_pos].to_string();
            let port = address_port[bracket_pos + 2..].to_string();
            return (ip, port); // IPv6地址不需要方括号给webrtc-rs
        }
    }
    
    // 处理标准IPv4格式 ip:port
    if let Some(colon_pos) = address_port.rfind(':') {
        let potential_ip = &address_port[..colon_pos];
        let potential_port = &address_port[colon_pos + 1..];
        
        // 检查端口部分是否为纯数字且不为空
        if potential_port.chars().all(|c| c.is_ascii_digit()) && !potential_port.is_empty() {
            return (potential_ip.to_string(), potential_port.to_string());
        }
    }
    
    // 如果所有解析都失败，返回空字符串让上层跳过
    println!("⚠️ 无法解析地址:端口 {}, 跳过该候选", address_port);
    ("".to_string(), "".to_string()) // 返回空字符串，让上层跳过
} 