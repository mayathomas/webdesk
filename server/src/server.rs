use crate::types::*;
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
    
    // WebSocket路由 - /ws
    let state_filter = warp::any().map(move || state.clone());
    let websocket = warp::path("ws")
        .and(warp::ws())
        .and(state_filter)
        .map(|ws: warp::ws::Ws, state: ServerState| {
            ws.on_upgrade(move |socket| handle_websocket(socket, state))
        });
    
    // 静态文件路由
    let static_files = warp::path("static").and(warp::fs::dir("static"));
    
    // 首页路由
    let index = warp::path::end().and(warp::get()).map(|| {
        warp::reply::html(include_str!("../static/index.html"))
    });
    
    // 合并所有路由
    let routes = websocket.or(static_files).or(index);
    
    println!("🌐 HTTP和WebSocket服务器正在监听: {}", addr);
    println!("   - 网页界面: http://{}", addr);
    println!("   - WebSocket: ws://{}/ws", addr);
    
    warp::serve(routes).run(addr).await;
    
    Ok(())
}

/// 处理WebSocket连接
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
                                
                                println!("📱 客户端已注册: {}", id);
                                
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
                                // 浏览器连接
                                if let Some(mut client_state) = state.clients.get_mut(&req.client_id) {
                                    if client_state.auth_code == req.auth_code {
                                        client_state.browser_connected = true;
                                        state.browser_connections.insert(req.client_id.clone(), tx.clone());
                                        connection_type = Some("browser".to_string());
                                        client_id = Some(req.client_id.clone());
                                        
                                        println!("🌐 浏览器已连接到客户端: {}", req.client_id);
                                        
                                        // 向浏览器发送连接成功响应
                                        let response = WebSocketMessage::Connected {
                                            success: true,
                                            message: "连接成功".to_string(),
                                        };
                                        
                                        if let Ok(msg) = serde_json::to_string(&response) {
                                            let _ = tx.send(Message::text(msg));
                                        }
                                        
                                        // 通知对应的客户端有浏览器连接
                                        if let Some(client_tx) = state.client_connections.get(&req.client_id) {
                                            let browser_connected_msg = WebSocketMessage::BrowserConnected {
                                                message: "浏览器已连接".to_string(),
                                            };
                                            
                                            if let Ok(msg) = serde_json::to_string(&browser_connected_msg) {
                                                let _ = client_tx.send(Message::text(msg));
                                            }
                                        }
                                    } else {
                                        println!("❌ 浏览器连接失败: 验证码错误");
                                        let response = WebSocketMessage::Error {
                                            message: "验证码错误".to_string(),
                                        };
                                        
                                        if let Ok(msg) = serde_json::to_string(&response) {
                                            let _ = tx.send(Message::text(msg));
                                        }
                                    }
                                } else {
                                    println!("❌ 浏览器连接失败: 客户端ID不存在");
                                    let response = WebSocketMessage::Error {
                                        message: "客户端ID不存在".to_string(),
                                    };
                                    
                                    if let Ok(msg) = serde_json::to_string(&response) {
                                        let _ = tx.send(Message::text(msg));
                                    }
                                }
                            }
                            
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
                                    println!("🌐 浏览器已断开连接: {}", id);
                                    if let Some(mut client_state) = state.clients.get_mut(id) {
                                        client_state.browser_connected = false;
                                    }
                                    
                                    // 通知客户端浏览器已断开连接
                                    if let Some(client_tx) = state.client_connections.get(id) {
                                        let browser_disconnected_msg = WebSocketMessage::BrowserDisconnected {
                                            message: "浏览器连接已断开".to_string(),
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
                println!("📱 客户端已断开连接: {}", id);
                state.client_connections.remove(&id);
                if let Some(mut client_state) = state.clients.get_mut(&id) {
                    client_state.is_connected = false;
                }
            }
            Some("browser") => {
                println!("🌐 浏览器连接已断开: {}", id);
                state.browser_connections.remove(&id);
                if let Some(mut client_state) = state.clients.get_mut(&id) {
                    client_state.browser_connected = false;
                }
                
                // 通知客户端浏览器已断开连接
                if let Some(client_tx) = state.client_connections.get(&id) {
                    let browser_disconnected_msg = WebSocketMessage::BrowserDisconnected {
                        message: "浏览器连接已断开".to_string(),
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