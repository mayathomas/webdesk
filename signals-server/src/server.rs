use crate::types::*;
use anyhow::Result;
use dashmap::DashMap;
use futures_util::{SinkExt, StreamExt};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio_tungstenite::tungstenite::Message;

/// 信令服务器状态
#[derive(Clone)]
pub struct SignalsServerState {
    /// 已注册的客户端 client_id -> ClientState
    pub clients: Arc<DashMap<String, ClientState>>,
    /// 客户端WebSocket连接 client_id -> sender
    pub client_connections: Arc<DashMap<String, tokio::sync::mpsc::UnboundedSender<Message>>>,
    /// 浏览器WebSocket连接 client_id -> sender
    pub browser_connections: Arc<DashMap<String, tokio::sync::mpsc::UnboundedSender<Message>>>,
}

impl SignalsServerState {
    pub fn new() -> Self {
        Self {
            clients: Arc::new(DashMap::new()),
            client_connections: Arc::new(DashMap::new()),
            browser_connections: Arc::new(DashMap::new()),
        }
    }
}

/// 启动纯WebRTC信令服务器
pub async fn start_signals_server(addr: SocketAddr) -> Result<()> {
    let state = SignalsServerState::new();
    
    log::info!("🚀 启动WebRTC信令服务器...");
    log::info!("📡 WebSocket信令端点: ws://{}/ws", addr);
    
    let listener = tokio::net::TcpListener::bind(addr).await?;
    
    while let Ok((stream, _)) = listener.accept().await {
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, state).await {
                log::error!("处理连接时出错: {}", e);
            }
        });
    }
    
    Ok(())
}

/// 处理TCP连接 - 尝试升级为WebSocket
async fn handle_connection(
    stream: tokio::net::TcpStream,
    state: SignalsServerState,
) -> Result<()> {
    let ws_stream = tokio_tungstenite::accept_async(stream).await?;
    handle_websocket(ws_stream, state).await;
    Ok(())
}

/// 处理WebSocket连接 (纯WebRTC信令服务器)
async fn handle_websocket(
    ws: tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    state: SignalsServerState,
) {
    let (mut ws_sender, mut ws_receiver) = ws.split();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Message>();
    
    // 处理发送队列
    let send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if let Ok(text) = msg.to_text() {
                if ws_sender.send(tokio_tungstenite::tungstenite::Message::text(text)).await.is_err() {
                    break;
                }
            }
        }
    });
    
    // 处理接收消息
    let mut connection_type: Option<String> = None;
    let mut client_id: Option<String> = None;
    
    while let Some(msg) = ws_receiver.next().await {
        match msg {
            Ok(msg) if msg.is_text() => {
                if let Ok(text) = msg.to_text() {
                    if let Ok(ws_msg) = serde_json::from_str::<WebSocketMessage>(text) {
                        match ws_msg {
                            WebSocketMessage::Register(req) => {
                                // 客户端注册
                                let id = generate_client_id(&req.mac_address);
                                let client_state = ClientState::new(req.auth_code);
                                
                                state.clients.insert(id.clone(), client_state);
                                state.client_connections.insert(id.clone(), tx.clone());
                                connection_type = Some("client".to_string());
                                client_id = Some(id.clone());
                                
                                log::info!("📱 客户端已注册: {}", id);
                                
                                let response = WebSocketMessage::RegisterResponse(RegisterResponse {
                                    client_id: id,
                                    success: true,
                                    message: "注册成功".to_string(),
                                });
                                
                                if let Ok(msg) = serde_json::to_string(&response) {
                                    let _ = tx.send(Message::text(msg));
                                }
                            }
                            
                            WebSocketMessage::BrowserConnect(req) => {
                                // 浏览器连接信令
                                if let Some(mut client_state) = state.clients.get_mut(&req.client_id) {
                                    if client_state.auth_code == req.auth_code {
                                        client_state.browser_connected = true;
                                        state.browser_connections.insert(req.client_id.clone(), tx.clone());
                                        connection_type = Some("browser".to_string());
                                        client_id = Some(req.client_id.clone());
                                        
                                        log::info!("🌐 浏览器开始WebRTC信令交换: {}", req.client_id);
                                        
                                        let response = WebSocketMessage::Connected {
                                            success: true,
                                            message: "信令连接成功，开始WebRTC协商".to_string(),
                                        };
                                        
                                        if let Ok(msg) = serde_json::to_string(&response) {
                                            let _ = tx.send(Message::text(msg));
                                        }
                                        
                                        // 通知客户端有浏览器连接
                                        if let Some(client_tx) = state.client_connections.get(&req.client_id) {
                                            let browser_connected_msg = WebSocketMessage::BrowserConnected {
                                                message: "浏览器开始WebRTC连接".to_string(),
                                            };
                                            
                                            if let Ok(msg) = serde_json::to_string(&browser_connected_msg) {
                                                let _ = client_tx.send(Message::text(msg));
                                            }
                                        }
                                    } else {
                                        send_error(&tx, "验证码错误").await;
                                    }
                                } else {
                                    send_error(&tx, "客户端ID不存在").await;
                                }
                            }
                            
                            // WebRTC 信令消息转发
                            WebSocketMessage::WebRTCOffer { target_id, session_description } => {
                                log::debug!("📡 转发WebRTC Offer到客户端: {}", target_id);
                                forward_to_client(&state, &target_id, WebSocketMessage::WebRTCOffer { 
                                    target_id: target_id.clone(), 
                                    session_description 
                                }).await;
                            }
                            
                            WebSocketMessage::WebRTCAnswer { target_id, session_description } => {
                                log::debug!("📡 转发WebRTC Answer到浏览器: {}", target_id);
                                forward_to_browser(&state, &target_id, WebSocketMessage::WebRTCAnswer { 
                                    target_id: target_id.clone(), 
                                    session_description 
                                }).await;
                            }
                            
                            WebSocketMessage::WebRTCIceCandidate { target_id, ice_candidate } => {
                                let candidate_string = ice_candidate.candidate.clone();
                                log::debug!("🧊 转发ICE候选，target_id: {}, 候选: {}", target_id, candidate_string);
                                
                                // 判断发送方和接收方
                                if target_id == "browser" {
                                    // 来自客户端，发送给浏览器
                                    if let Some(current_client_id) = &client_id {
                                        forward_to_browser(&state, current_client_id, WebSocketMessage::WebRTCIceCandidate { 
                                            target_id: current_client_id.clone(), 
                                            ice_candidate
                                        }).await;
                                    }
                                } else {
                                    // 来自浏览器，发送给客户端
                                    forward_to_client(&state, &target_id, WebSocketMessage::WebRTCIceCandidate { 
                                        target_id: target_id.clone(), 
                                        ice_candidate 
                                    }).await;
                                }
                            }
                            
                            // H.264视频流配置消息
                            WebSocketMessage::VideoStreamConfig { target_id, config } => {
                                log::info!("🎬 转发H.264视频流配置: {}x{}@{:.1}fps, {}kbps", 
                                    config.width, config.height, config.fps, config.bitrate / 1000);
                                    
                                forward_to_client(&state, &target_id, WebSocketMessage::VideoStreamConfig { 
                                    target_id: target_id.clone(), 
                                    config 
                                }).await;
                                
                                // 标记视频流为激活状态
                                if let Some(mut client_state) = state.clients.get_mut(&target_id) {
                                    client_state.video_stream_active = true;
                                }
                            }
                            
                            // 强制关键帧消息
                            WebSocketMessage::ForceKeyframe { target_id } => {
                                log::info!("🔑 转发强制关键帧请求到客户端: {}", &target_id);
                                forward_to_client(&state, &target_id, WebSocketMessage::ForceKeyframe { 
                                    target_id: target_id.clone()
                                }).await;
                            }
                            
                            WebSocketMessage::Disconnect => {
                                if let Some(id) = &client_id {
                                    handle_disconnect(&state, id).await;
                                }
                                break;
                            }
                            
                            WebSocketMessage::Ping => {
                                let response = WebSocketMessage::Pong;
                                if let Ok(msg) = serde_json::to_string(&response) {
                                    let _ = tx.send(Message::text(msg));
                                }
                            }
                            
                            _ => {
                                log::warn!("⚠️ 收到未知消息类型");
                            }
                        }
                    }
                }
            }
            Ok(msg) if msg.is_close() => break,
            Err(_) => break,
            _ => {}
        }
    }
    
    // 清理连接
    if let Some(id) = client_id {
        cleanup_connection(&state, &id, &connection_type).await;
    }
    
    send_task.abort();
}

/// 转发消息到客户端
async fn forward_to_client(state: &SignalsServerState, target_id: &str, msg: WebSocketMessage) {
    if let Some(client_tx) = state.client_connections.get(target_id) {
        if let Ok(json) = serde_json::to_string(&msg) {
            let _ = client_tx.send(Message::text(json));
        }
    }
}

/// 转发消息到浏览器
async fn forward_to_browser(state: &SignalsServerState, target_id: &str, msg: WebSocketMessage) {
    if let Some(browser_tx) = state.browser_connections.get(target_id) {
        if let Ok(json) = serde_json::to_string(&msg) {
            let _ = browser_tx.send(Message::text(json));
        }
    }
}

/// 处理断开连接
async fn handle_disconnect(state: &SignalsServerState, id: &str) {
    log::info!("🌐 WebRTC连接已断开: {}", id);
    if let Some(mut client_state) = state.clients.get_mut(id) {
        client_state.browser_connected = false;
        client_state.video_stream_active = false;
    }
    
    // 通知客户端浏览器已断开连接
    if let Some(client_tx) = state.client_connections.get(id) {
        let browser_disconnected_msg = WebSocketMessage::BrowserDisconnected {
            message: "WebRTC连接已断开".to_string(),
        };
        
        if let Ok(msg) = serde_json::to_string(&browser_disconnected_msg) {
            let _ = client_tx.send(Message::text(msg));
        }
    }
    
    state.browser_connections.remove(id);
}

/// 清理连接
async fn cleanup_connection(state: &SignalsServerState, id: &str, connection_type: &Option<String>) {
    match connection_type.as_deref() {
        Some("client") => {
            log::info!("📱 客户端已断开连接: {}", id);
            state.client_connections.remove(id);
            if let Some(mut client_state) = state.clients.get_mut(id) {
                client_state.is_connected = false;
            }
        }
        Some("browser") => {
            log::info!("🌐 浏览器信令连接已断开: {}", id);
            state.browser_connections.remove(id);
            if let Some(mut client_state) = state.clients.get_mut(id) {
                client_state.browser_connected = false;
            }
            
            // 通知客户端浏览器已断开连接
            if let Some(client_tx) = state.client_connections.get(id) {
                let browser_disconnected_msg = WebSocketMessage::BrowserDisconnected {
                    message: "WebRTC信令连接已断开".to_string(),
                };
                
                if let Ok(msg) = serde_json::to_string(&browser_disconnected_msg) {
                    let _ = client_tx.send(Message::text(msg));
                }
            }
        }
        _ => {}
    }
}

/// 发送错误消息
async fn send_error(tx: &tokio::sync::mpsc::UnboundedSender<Message>, message: &str) {
    log::warn!("❌ {}", message);
    let response = WebSocketMessage::Error {
        message: message.to_string(),
    };
    
    if let Ok(msg) = serde_json::to_string(&response) {
        let _ = tx.send(Message::text(msg));
    }
}

/// 根据MAC地址生成客户端ID
fn generate_client_id(mac_address: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    
    let mut hasher = DefaultHasher::new();
    mac_address.hash(&mut hasher);
    let hash = hasher.finish();
    
    format!("{:016x}", hash)
} 