use crate::types::*;
use crate::config::WebRtcConfig;
use anyhow::Result;
use dashmap::DashMap;
use futures_util::{SinkExt, StreamExt};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio_tungstenite::tungstenite::Message;
use warp::Filter;

/// 服务端状态
#[derive(Clone)]
pub struct ServerState {
    /// 已注册的客户端 client_id -> ClientState
    pub clients: Arc<DashMap<String, ClientState>>,
    /// 客户端WebSocket连接 client_id -> sender
    pub client_connections: Arc<DashMap<String, tokio::sync::mpsc::UnboundedSender<Message>>>,
    /// 浏览器WebSocket连接 client_id -> sender
    pub browser_connections: Arc<DashMap<String, tokio::sync::mpsc::UnboundedSender<Message>>>,
}

impl ServerState {
    pub fn new() -> Self {
        Self {
            clients: Arc::new(DashMap::new()),
            client_connections: Arc::new(DashMap::new()),
            browser_connections: Arc::new(DashMap::new()),
        }
    }
}

/// 启动HTTP和WebSocket服务器（共用端口）
pub async fn start_server(addr: SocketAddr) -> Result<()> {
    let state = ServerState::new();
    
    log::info!("🚀 启动WebRTC远程控制服务器...");
    log::info!("   📡 支持WebRTC P2P连接");
    log::info!("   🔗 WebSocket信令服务器");
    
    // WebSocket路由 - /ws
    let state_filter = warp::any().map(move || state.clone());
    let websocket = warp::path("ws")
        .and(warp::ws())
        .and(state_filter)
        .map(|ws: warp::ws::Ws, state: ServerState| {
            ws.on_upgrade(move |socket| handle_websocket(socket, state))
        });
    
    // WebRTC配置API路由
    let webrtc_config = warp::path("api")
        .and(warp::path("webrtc-config"))
        .and(warp::get())
        .map(|| {
            match WebRtcConfig::load_from_file("webrtc-config.yaml") {
                Ok(config) => {
                    warp::reply::with_status(
                        warp::reply::json(&config.to_frontend_format()),
                        warp::http::StatusCode::OK
                    )
                }
                Err(e) => {
                    log::error!("❌ 加载WebRTC配置失败: {}", e);
                    let error = serde_json::json!({
                        "error": format!("Failed to load config: {}", e)
                    });
                    warp::reply::with_status(
                        warp::reply::json(&error),
                        warp::http::StatusCode::INTERNAL_SERVER_ERROR
                    )
                }
            }
        });
    
    // 静态文件路由 - 直接提供静态文件
    let static_files = warp::fs::dir("static");
    
    // 首页路由 - 直接重定向到index.html
    let index = warp::path::end().and(warp::get()).map(|| {
        warp::redirect::found(warp::http::Uri::from_static("/index.html"))
    });
    
    // 合并所有路由 - 注意顺序：WebSocket优先，API，然后首页，最后静态文件
    let routes = websocket.or(webrtc_config).or(index).or(static_files);
    
    log::info!("🌐 HTTP和WebSocket服务器正在监听: {}", addr);
    log::info!("   - 网页界面: http://{}", addr);
    log::info!("   - WebSocket信令: ws://{}/ws", addr);
    
    warp::serve(routes).run(addr).await;
    
    Ok(())
}

/// 处理WebSocket连接（现在作为WebRTC信令服务器）
async fn handle_websocket(ws: warp::ws::WebSocket, state: ServerState) {
    let (mut ws_sender, mut ws_receiver) = ws.split();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Message>();
    
    // 处理发送队列
    let send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if let Ok(text) = msg.to_text() {
                if ws_sender.send(warp::ws::Message::text(text)).await.is_err() {
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
                if let Ok(text) = msg.to_str() {
                    if let Ok(ws_msg) = serde_json::from_str::<WebSocketMessage>(text) {
                        match ws_msg {
                            WebSocketMessage::Register(req) => {
                                // 客户端注册
                                let id = generate_client_id(&req.mac_address);
                                let client_state = ClientState {
                                    client_id: id.clone(),
                                    mac_address: req.mac_address,
                                    auth_code: req.auth_code,
                                    is_connected: true,
                                    browser_connected: false,
                                };
                                
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
                                // 浏览器连接（现在是WebRTC信令）
                                if let Some(mut client_state) = state.clients.get_mut(&req.client_id) {
                                    if client_state.auth_code == req.auth_code {
                                        client_state.browser_connected = true;
                                        state.browser_connections.insert(req.client_id.clone(), tx.clone());
                                        connection_type = Some("browser".to_string());
                                        client_id = Some(req.client_id.clone());
                                        
                                        log::info!("🌐 浏览器开始WebRTC信令交换: {}", req.client_id);
                                        
                                        // 向浏览器发送连接成功响应
                                        let response = WebSocketMessage::Connected {
                                            success: true,
                                            message: "信令连接成功，开始WebRTC协商".to_string(),
                                        };
                                        
                                        if let Ok(msg) = serde_json::to_string(&response) {
                                            let _ = tx.send(Message::text(msg));
                                        }
                                        
                                        // 通知对应的客户端有浏览器连接
                                        if let Some(client_tx) = state.client_connections.get(&req.client_id) {
                                            let browser_connected_msg = WebSocketMessage::BrowserConnected {
                                                message: "浏览器开始WebRTC连接".to_string(),
                                            };
                                            
                                            if let Ok(msg) = serde_json::to_string(&browser_connected_msg) {
                                                let _ = client_tx.send(Message::text(msg));
                                            }
                                        }
                                    } else {
                                        log::warn!("❌ 浏览器连接失败: 验证码错误");
                                        let response = WebSocketMessage::Error {
                                            message: "验证码错误".to_string(),
                                        };
                                        
                                        if let Ok(msg) = serde_json::to_string(&response) {
                                            let _ = tx.send(Message::text(msg));
                                        }
                                    }
                                } else {
                                    log::warn!("❌ 浏览器连接失败: 客户端ID不存在");
                                    let response = WebSocketMessage::Error {
                                        message: "客户端ID不存在".to_string(),
                                    };
                                    
                                    if let Ok(msg) = serde_json::to_string(&response) {
                                        let _ = tx.send(Message::text(msg));
                                    }
                                }
                            }
                            
                            // WebRTC 信令消息转发
                            WebSocketMessage::WebRTCOffer { target_id, session_description } => {
                                log::debug!("📡 转发WebRTC Offer到客户端: {}", target_id);
                                if let Some(client_tx) = state.client_connections.get(&target_id) {
                                    let msg = WebSocketMessage::WebRTCOffer { 
                                        target_id: target_id.clone(), 
                                        session_description 
                                    };
                                    if let Ok(json) = serde_json::to_string(&msg) {
                                        let _ = client_tx.send(Message::text(json));
                                    }
                                }
                            }
                            
                            WebSocketMessage::WebRTCAnswer { target_id, session_description } => {
                                log::debug!("📡 转发WebRTC Answer到浏览器: {}", target_id);
                                if let Some(browser_tx) = state.browser_connections.get(&target_id) {
                                    let msg = WebSocketMessage::WebRTCAnswer { 
                                        target_id: target_id.clone(), 
                                        session_description 
                                    };
                                    if let Ok(json) = serde_json::to_string(&msg) {
                                        let _ = browser_tx.send(Message::text(json));
                                    }
                                }
                            }
                            
                            WebSocketMessage::WebRTCIceCandidate { target_id, ice_candidate } => {
                                let candidate_string = ice_candidate.candidate.clone();
                                log::debug!("🧊 转发ICE候选，target_id: {}, 候选: {}", target_id, candidate_string);
                                
                                // 判断发送方和接收方
                                if target_id == "browser" {
                                    // 来自客户端，发送给浏览器
                                    if let Some(current_client_id) = &client_id {
                                        if let Some(browser_tx) = state.browser_connections.get(current_client_id) {
                                            let msg = WebSocketMessage::WebRTCIceCandidate { 
                                                target_id: current_client_id.clone(), 
                                                ice_candidate
                                            };
                                            if let Ok(json) = serde_json::to_string(&msg) {
                                                let _ = browser_tx.send(Message::text(json));
                                                log::debug!("✅ ICE候选已转发到浏览器: {}, 候选: {}", current_client_id, candidate_string);
                                            }
                                        } else {
                                            log::debug!("⚠️ 未找到对应的浏览器连接: {}, 候选: {}", current_client_id, candidate_string);
                                        }
                                    }
                                } else {
                                    // 来自浏览器，发送给客户端
                                    if let Some(client_tx) = state.client_connections.get(&target_id) {
                                        let msg = WebSocketMessage::WebRTCIceCandidate { 
                                            target_id: target_id.clone(), 
                                            ice_candidate 
                                        };
                                        if let Ok(json) = serde_json::to_string(&msg) {
                                            let _ = client_tx.send(Message::text(json));
                                            log::debug!("✅ ICE候选已转发到客户端: {}, 候选: {}", target_id, candidate_string);
                                        }
                                    } else {
                                        log::debug!("⚠️ 未找到对应的客户端连接: {}, 候选: {}", target_id, candidate_string);
                                    }
                                }
                            }
                            
                            // 传统WebSocket消息（向后兼容）
                            WebSocketMessage::ScreenData(screen_data) => {
                                // 转发屏幕数据到浏览器（静默处理，不打印日志）
                                if let Some(id) = &client_id {
                                    if let Some(browser_tx) = state.browser_connections.get(id) {
                                        let msg = WebSocketMessage::ScreenData(screen_data);
                                        if let Ok(json) = serde_json::to_string(&msg) {
                                            let _ = browser_tx.send(Message::text(json));
                                        }
                                    }
                                }
                            }
                            
                            WebSocketMessage::MouseEvent(mouse_event) => {
                                // 转发鼠标事件到客户端（静默处理）
                                if let Some(id) = &client_id {
                                    if let Some(client_tx) = state.client_connections.get(id) {
                                        let msg = WebSocketMessage::MouseEvent(mouse_event);
                                        if let Ok(json) = serde_json::to_string(&msg) {
                                            let _ = client_tx.send(Message::text(json));
                                        }
                                    }
                                }
                            }
                            
                            WebSocketMessage::KeyboardEvent(keyboard_event) => {
                                // 转发键盘事件到客户端（静默处理）
                                if let Some(id) = &client_id {
                                    if let Some(client_tx) = state.client_connections.get(id) {
                                        let msg = WebSocketMessage::KeyboardEvent(keyboard_event);
                                        if let Ok(json) = serde_json::to_string(&msg) {
                                            let _ = client_tx.send(Message::text(json));
                                        }
                                    }
                                }
                            }
                            
                            WebSocketMessage::Disconnect => {
                                // 断开连接
                                if let Some(id) = &client_id {
                                    log::info!("🌐 连接已断开: {}", id);
                                    if let Some(mut client_state) = state.clients.get_mut(id) {
                                        client_state.browser_connected = false;
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
                                break;
                            }
                            
                            WebSocketMessage::Ping => {
                                let response = WebSocketMessage::Pong;
                                if let Ok(msg) = serde_json::to_string(&response) {
                                    let _ = tx.send(Message::text(msg));
                                }
                            }
                            
                            _ => {}
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
        match connection_type.as_deref() {
            Some("client") => {
                log::info!("📱 客户端已断开连接: {}", id);
                state.client_connections.remove(&id);
                if let Some(mut client_state) = state.clients.get_mut(&id) {
                    client_state.is_connected = false;
                }
            }
            Some("browser") => {
                log::info!("🌐 浏览器信令连接已断开: {}", id);
                state.browser_connections.remove(&id);
                if let Some(mut client_state) = state.clients.get_mut(&id) {
                    client_state.browser_connected = false;
                }
                
                // 通知客户端浏览器已断开连接
                if let Some(client_tx) = state.client_connections.get(&id) {
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
    
    send_task.abort();
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
