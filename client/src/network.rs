use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use url::Url;
use tauri::{AppHandle, Emitter};

use crate::state::AppState;
use crate::types::*;
use crate::input::InputController;
use crate::threads;
use crate::webrtc::WebRTCClient;

/// 运行远程控制服务
pub async fn run_remote_service(state: AppState, app: AppHandle) -> Result<()> {
    println!("💻 启动远程控制客户端 (WebRTC + 信令服务器架构)...");
    
    // 发送启动中状态
    let _ = app.emit("status-update", serde_json::json!({
        "state": "正在连接服务器...",
        "running": true
    }));
    
    // 加载配置
    let mut config = {
        let config_lock = state.client_config.lock().unwrap();
        config_lock.clone()
    };
    
    // 运行时获取MAC地址  
    let mac_address = match config.get_mac_address() {
        Ok(mac) => mac,
        Err(e) => {
            let _ = app.emit("status-update", serde_json::json!({
                "state": "获取MAC地址失败",
                "running": false
            }));
            return Err(e);
        }
    };
    
    println!("⚙️ 客户端配置:");
    println!("   📡 信令服务器地址: {}", config.server_url);
    println!("   🏠 MAC地址: {}", mac_address);
    println!("   🔑 验证码: {}", config.auth_code);
    if let Some(client_id) = &config.client_id {
        println!("   🆔 客户端ID: {}", client_id);
    }
    
    // 连接到信令服务器
    let url = match Url::parse(&config.server_url) {
        Ok(url) => url,
        Err(e) => {
            let _ = app.emit("status-update", serde_json::json!({
                "state": "服务器URL格式错误",
                "running": false
            }));
            return Err(e.into());
        }
    };
    
    println!("🔌 正在连接到信令服务器: {}", url);
    
    let (ws_stream, _) = match connect_async(url).await {
        Ok(result) => result,
        Err(e) => {
            println!("❌ 连接信令服务器失败: {}", e);
            let _ = app.emit("status-update", serde_json::json!({
                "state": "连接服务器失败",
                "running": false
            }));
            return Err(e.into());
        }
    };
    
    println!("✅ 信令WebSocket连接已建立");
    
    // 立即发送连接成功状态到前端
    let _ = app.emit("status-update", serde_json::json!({
        "state": "WaitingForRegistration",
        "running": true
    }));
    
    let (mut ws_sender, mut ws_receiver) = ws_stream.split();
    
    // 发送注册请求
    let register_request = WebSocketMessage::Register(RegisterRequest {
        mac_address: mac_address.clone(),
        auth_code: config.auth_code.clone(),
    });
    
    let register_msg = serde_json::to_string(&register_request)?;
    println!("📤 发送注册请求: {}", register_msg);
    ws_sender.send(Message::Text(register_msg)).await?;
    
    // 发送注册中状态到前端
    let _ = app.emit("status-update", serde_json::json!({
        "state": "WaitingForRegistration",
        "running": true
    }));
    
    // 初始化组件
    let input_controller = Arc::new(InputController::new());
    
    // 状态管理
    let mut client_state = ClientState::WaitingForRegistration;
    let mut screen_thread_handle: Option<tokio::task::JoinHandle<()>> = None;
    let mut input_thread_handle: Option<tokio::task::JoinHandle<()>> = None;
    
    // WebRTC相关状态
    let mut webrtc_client: Option<WebRTCClient> = None;
    
    // RustDesk架构：通道系统在每次连接时创建
    let mut screen_control_tx: Option<tokio::sync::mpsc::UnboundedSender<ThreadControlSignal>> = None;
    let mut screen_data_rx: Option<tokio::sync::mpsc::UnboundedReceiver<WebSocketMessage>> = None;
    let mut input_control_tx: Option<tokio::sync::mpsc::UnboundedSender<ThreadControlSignal>> = None;
    let mut input_event_tx: Option<tokio::sync::mpsc::UnboundedSender<WebSocketMessage>> = None;
    
    // 创建信令通道
    let (signaling_tx, mut signaling_rx) = tokio::sync::mpsc::unbounded_channel::<WebSocketMessage>();
    
    // 数据通道就绪通知通道
    let (data_channel_ready_tx, mut data_channel_ready_rx) = tokio::sync::mpsc::unbounded_channel::<()>();
    
    println!("🏗️ WebRTC架构已初始化：主线程专注信令通信...");
    
    // 主线程：处理信令WebSocket连接和WebRTC协商
    loop {
        tokio::select! {
            // 处理信令WebSocket消息
            msg = ws_receiver.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        println!("📥 收到信令服务器消息: {}", text);
                        if let Ok(ws_msg) = serde_json::from_str::<WebSocketMessage>(&text) {
                            println!("📦 解析消息类型: {:?}", std::mem::discriminant(&ws_msg));
                            match ws_msg {
                                WebSocketMessage::RegisterResponse(response) => {
                                    if response.success {
                                        println!("🎉 注册成功，客户端ID: {}", response.client_id);
                                        config.update_client_id(response.client_id.clone())?;
                                        
                                        // 更新状态
                                        {
                                            let mut config_lock = state.client_config.lock().unwrap();
                                            *config_lock = config.clone();
                                        }
                                        
                                        client_state = ClientState::WaitingForBrowser;
                                        
                                        // 发送状态更新到前端
                                        let _ = app.emit("client-info-update", serde_json::json!({
                                            "clientId": response.client_id
                                        }));
                                    } else {
                                        println!("❌ 注册失败: {}", response.message);
                                    }
                                }
                                
                                WebSocketMessage::BrowserConnected { message } => {
                                    println!("🌐 浏览器开始WebRTC连接: {}", message);
                                    client_state = ClientState::WebRTCConnecting;
                                    
                                    // 初始化WebRTC客户端
                                    if webrtc_client.is_none() {
                                        if let Some(client_id) = &config.client_id {
                                            match WebRTCClient::new(client_id.clone(), signaling_tx.clone(), Some(data_channel_ready_tx.clone())).await {
                                                Ok(mut client) => {
                                                    if let Err(e) = client.setup_handlers().await {
                                                        println!("❌ 设置WebRTC处理器失败: {}", e);
                                                    } else {
                                                        webrtc_client = Some(client);
                                                        println!("✅ WebRTC客户端已初始化");
                                                    }
                                                }
                                                Err(e) => {
                                                    println!("❌ 创建WebRTC客户端失败: {}", e);
                                                }
                                            }
                                        }
                                    }
                                }
                                
                                // WebRTC信令处理
                                WebSocketMessage::WebRTCOffer { session_description, .. } => {
                                    println!("📡 收到WebRTC Offer");
                                    if let Some(ref mut client) = webrtc_client {
                                        if let Err(e) = client.handle_offer(session_description).await {
                                            println!("❌ 处理WebRTC Offer失败: {}", e);
                                        } else {
                                            client_state = ClientState::WebRTCConnected;
                                            println!("🎯 WebRTC连接协商完成，等待数据通道建立...");
                                        }
                                    }
                                }
                                
                                WebSocketMessage::WebRTCIceCandidate { ice_candidate, .. } => {
                                    println!("🧊 收到ICE候选");
                                    if let Some(ref client) = webrtc_client {
                                        if let Err(e) = client.handle_ice_candidate(ice_candidate).await {
                                            println!("❌ 处理ICE候选失败: {}", e);
                                        }
                                    }
                                }
                                
                                WebSocketMessage::BrowserDisconnected { message } => {
                                    println!("🌐 浏览器已断开WebRTC连接: {}", message);
                                    client_state = ClientState::WaitingForBrowser;
                                    
                                    // 关闭WebRTC连接
                                    if let Some(client) = webrtc_client.take() {
                                        let _ = client.close().await;
                                    }
                                    
                                    // 停止工作线程
                                    stop_worker_threads(
                                        &mut screen_thread_handle,
                                        &mut input_thread_handle,
                                        &mut screen_control_tx,
                                        &mut screen_data_rx,
                                        &mut input_control_tx,
                                        &mut input_event_tx,
                                    );
                                }
                                
                                // 转发输入事件到输入处理线程
                                WebSocketMessage::MouseEvent(_) | WebSocketMessage::KeyboardEvent(_) => {
                                    if let Some(tx) = &input_event_tx {
                                        let _ = tx.send(ws_msg);
                                    }
                                }
                                
                                WebSocketMessage::Error { message } => {
                                    println!("⚠️ 信令服务器错误: {}", message);
                                }
                                
                                WebSocketMessage::Ping => {
                                    let pong = WebSocketMessage::Pong;
                                    if let Ok(msg) = serde_json::to_string(&pong) {
                                        let _ = ws_sender.send(Message::Text(msg)).await;
                                    }
                                }
                                
                                _ => {}
                            }
                        } else {
                            println!("❌ 无法解析信令服务器消息: {}", text);
                        }
                    }
                    Some(Ok(Message::Close(_))) => {
                        println!("📡 信令服务器关闭了连接");
                        break;
                    }
                    Some(Err(e)) => {
                        eprintln!("❌ 信令WebSocket错误: {}", e);
                        break;
                    }
                    None => {
                        println!("📡 信令WebSocket连接已断开");
                        break;
                    }
                    _ => {}
                }
                
                // 更新状态到应用状态管理器
                {
                    let mut state_lock = state.client_state.lock().unwrap();
                    *state_lock = client_state.clone();
                }
                
                // 发送状态更新到前端
                let _ = app.emit("status-update", serde_json::json!({
                    "state": format!("{:?}", client_state),
                    "client_id": config.client_id.clone()
                }));
            }
            
            // 处理WebRTC信令消息发送
            Some(signaling_msg) = signaling_rx.recv() => {
                if let Ok(msg) = serde_json::to_string(&signaling_msg) {
                    if ws_sender.send(Message::Text(msg)).await.is_err() {
                        println!("❌ 发送信令消息失败，连接可能已断开");
                        break;
                    }
                }
            }
            
            // 等待数据通道就绪
            Some(_) = data_channel_ready_rx.recv() => {
                println!("🎉 数据通道已就绪，启动屏幕捕获和输入处理！");
                
                // 现在才启动屏幕捕获和输入处理线程
                start_worker_threads(
                    &mut screen_thread_handle,
                    &mut input_thread_handle,
                    &mut screen_control_tx,
                    &mut screen_data_rx,
                    &mut input_control_tx,
                    &mut input_event_tx,
                    input_controller.clone(),
                );
            }
            
            // 接收来自屏幕捕获线程的数据并通过WebRTC发送
            Some(screen_data) = async {
                if let Some(ref mut rx) = screen_data_rx {
                    rx.recv().await
                } else {
                    std::future::pending().await
                }
            } => {
                // 通过WebRTC数据通道发送屏幕数据
                if let Some(ref client) = webrtc_client {
                    if let WebSocketMessage::ScreenData(screen_data) = screen_data {
                        println!("🎬 准备通过WebRTC发送屏幕数据: {}x{}", screen_data.width, screen_data.height);
                        if let Err(e) = client.send_screen_data(&screen_data).await {
                            println!("❌ 通过WebRTC发送屏幕数据失败: {}", e);
                        }
                    } else {
                        println!("⚠️ 收到非屏幕数据消息: {:?}", std::mem::discriminant(&screen_data));
                    }
                } else {
                    println!("⚠️ WebRTC客户端未初始化，无法发送屏幕数据");
                }
            }
        }
    }
    
    // 清理资源
    println!("🧹 清理WebRTC和线程资源...");
    
    // 关闭WebRTC连接
    if let Some(client) = webrtc_client.take() {
        let _ = client.close().await;
    }
    
    // 停止工作线程
    stop_worker_threads(
        &mut screen_thread_handle,
        &mut input_thread_handle,
        &mut screen_control_tx,
        &mut screen_data_rx,
        &mut input_control_tx,
        &mut input_event_tx,
    );
    
    println!("👋 WebRTC远程控制服务已退出");
    Ok(())
}

/// 启动工作线程
fn start_worker_threads(
    screen_thread_handle: &mut Option<tokio::task::JoinHandle<()>>,
    input_thread_handle: &mut Option<tokio::task::JoinHandle<()>>,
    screen_control_tx: &mut Option<tokio::sync::mpsc::UnboundedSender<ThreadControlSignal>>,
    screen_data_rx: &mut Option<tokio::sync::mpsc::UnboundedReceiver<WebSocketMessage>>,
    input_control_tx: &mut Option<tokio::sync::mpsc::UnboundedSender<ThreadControlSignal>>,
    input_event_tx: &mut Option<tokio::sync::mpsc::UnboundedSender<WebSocketMessage>>,
    input_controller: Arc<InputController>,
) {
    if screen_thread_handle.is_none() {
        // 创建屏幕捕获通道系统
        let (s_ctrl_tx, s_ctrl_rx) = tokio::sync::mpsc::unbounded_channel::<ThreadControlSignal>();
        let (s_data_tx, s_data_rx) = tokio::sync::mpsc::unbounded_channel::<WebSocketMessage>();
        
        *screen_control_tx = Some(s_ctrl_tx);
        *screen_data_rx = Some(s_data_rx);
        
        println!("📷 启动屏幕捕获线程...");
        *screen_thread_handle = Some(tokio::task::spawn_blocking(move || {
            if let Err(e) = threads::screen_capture_thread(s_ctrl_rx, s_data_tx) {
                eprintln!("❌ 屏幕捕获线程错误: {}", e);
            }
        }));
    }
    
    if input_thread_handle.is_none() {
        // 创建输入事件处理通道系统
        let (i_ctrl_tx, i_ctrl_rx) = tokio::sync::mpsc::unbounded_channel::<ThreadControlSignal>();
        let (i_event_tx, i_event_rx) = tokio::sync::mpsc::unbounded_channel::<WebSocketMessage>();
        
        *input_control_tx = Some(i_ctrl_tx);
        *input_event_tx = Some(i_event_tx);
        
        println!("🎮 启动输入事件处理线程...");
        *input_thread_handle = Some(tokio::spawn(async move {
            threads::input_event_thread(
                input_controller,
                i_event_rx,
                i_ctrl_rx,
            ).await;
        }));
    }
    
    // 发送启动信号
    if let Some(tx) = screen_control_tx {
        let _ = tx.send(ThreadControlSignal::Start);
    }
    if let Some(tx) = input_control_tx {
        let _ = tx.send(ThreadControlSignal::Start);
    }
}

/// 停止工作线程
fn stop_worker_threads(
    screen_thread_handle: &mut Option<tokio::task::JoinHandle<()>>,
    input_thread_handle: &mut Option<tokio::task::JoinHandle<()>>,
    screen_control_tx: &mut Option<tokio::sync::mpsc::UnboundedSender<ThreadControlSignal>>,
    screen_data_rx: &mut Option<tokio::sync::mpsc::UnboundedReceiver<WebSocketMessage>>,
    input_control_tx: &mut Option<tokio::sync::mpsc::UnboundedSender<ThreadControlSignal>>,
    input_event_tx: &mut Option<tokio::sync::mpsc::UnboundedSender<WebSocketMessage>>,
) {
    println!("🛑 停止工作线程...");
    
    if let Some(tx) = screen_control_tx {
        let _ = tx.send(ThreadControlSignal::Stop);
    }
    if let Some(tx) = input_control_tx {
        let _ = tx.send(ThreadControlSignal::Stop);
    }
    
    // 等待线程结束
    if let Some(handle) = screen_thread_handle.take() {
        handle.abort();
    }
    if let Some(handle) = input_thread_handle.take() {
        handle.abort();
    }
    
    // 清理通道
    *screen_control_tx = None;
    *screen_data_rx = None;
    *input_control_tx = None;
    *input_event_tx = None;
} 