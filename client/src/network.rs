use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use std::time::Duration;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use url::Url;
use tauri::{AppHandle, Emitter};

use crate::state::AppState;
use crate::types::*;
use crate::input::InputController;
use crate::threads;

/// 运行远程控制服务
pub async fn run_remote_service(state: AppState, app: AppHandle) -> Result<()> {
    println!("💻 启动远程控制客户端 (Tauri 2.0 + RustDesk架构)...");
    
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
    println!("   📡 服务器地址11111: {}", config.server_url);
    println!("   🏠 MAC地址: {}", mac_address);
    println!("   🔑 验证码: {}", config.auth_code);
    if let Some(client_id) = &config.client_id {
        println!("   🆔 客户端ID: {}", client_id);
    }
    
    // 连接到服务器
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
    
    println!("🔌 正在连接到服务器: {}", url);
    
    let (ws_stream, _) = match connect_async(url).await {
        Ok(result) => result,
        Err(e) => {
            println!("❌ 连接服务器失败: {}", e);
            let _ = app.emit("status-update", serde_json::json!({
                "state": "连接服务器失败",
                "running": false
            }));
            return Err(e.into());
        }
    };
    
    println!("✅ WebSocket连接已建立");
    
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
    
    // RustDesk架构：通道系统在每次连接时创建
    let mut screen_control_tx: Option<tokio::sync::mpsc::UnboundedSender<ThreadControlSignal>> = None;
    let mut screen_data_rx: Option<tokio::sync::mpsc::UnboundedReceiver<WebSocketMessage>> = None;
    let mut input_control_tx: Option<tokio::sync::mpsc::UnboundedSender<ThreadControlSignal>> = None;
    let mut input_event_tx: Option<tokio::sync::mpsc::UnboundedSender<WebSocketMessage>> = None;
    
    println!("🏗️ RustDesk架构已初始化：主线程专注WebSocket通信...");
    
    // 主线程：仅处理WebSocket连接和消息路由
    loop {
        tokio::select! {
            // 处理WebSocket消息
            msg = ws_receiver.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        println!("📥 收到服务器消息: {}", text);
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
                                    println!("🌐 浏览器已连接: {}", message);
                                    client_state = ClientState::BrowserConnected;
                                    
                                    // RustDesk架构：连接时创建完整的通道系统和线程
                                    if screen_thread_handle.is_none() {
                                        // 创建屏幕捕获通道系统
                                        let (s_ctrl_tx, s_ctrl_rx) = tokio::sync::mpsc::unbounded_channel::<ThreadControlSignal>();
                                        let (s_data_tx, s_data_rx) = tokio::sync::mpsc::unbounded_channel::<WebSocketMessage>();
                                        
                                        screen_control_tx = Some(s_ctrl_tx);
                                        screen_data_rx = Some(s_data_rx);
                                        
                                        println!("📷 启动屏幕捕获线程...");
                                        screen_thread_handle = Some(tokio::task::spawn_blocking(move || {
                                            if let Err(e) = threads::screen_capture_thread(s_ctrl_rx, s_data_tx) {
                                                eprintln!("❌ 屏幕捕获线程错误: {}", e);
                                            }
                                        }));
                                    }
                                    
                                    if input_thread_handle.is_none() {
                                        // 创建输入事件处理通道系统
                                        let (i_ctrl_tx, i_ctrl_rx) = tokio::sync::mpsc::unbounded_channel::<ThreadControlSignal>();
                                        let (i_event_tx, i_event_rx) = tokio::sync::mpsc::unbounded_channel::<WebSocketMessage>();
                                        
                                        input_control_tx = Some(i_ctrl_tx);
                                        input_event_tx = Some(i_event_tx);
                                        
                                        println!("🎮 启动输入事件处理线程...");
                                        let input_controller_clone = input_controller.clone();
                                        input_thread_handle = Some(tokio::spawn(async move {
                                            threads::input_event_thread(
                                                input_controller_clone,
                                                i_event_rx,
                                                i_ctrl_rx,
                                            ).await;
                                        }));
                                    }
                                    
                                    // 发送启动信号
                                    if let Some(tx) = &screen_control_tx {
                                        let _ = tx.send(ThreadControlSignal::Start);
                                    }
                                    if let Some(tx) = &input_control_tx {
                                        let _ = tx.send(ThreadControlSignal::Start);
                                    }
                                }
                                
                                WebSocketMessage::BrowserDisconnected { message } => {
                                    println!("🌐 浏览器已断开: {}", message);
                                    client_state = ClientState::WaitingForBrowser;
                                    
                                    // RustDesk架构：断开时销毁线程和通道
                                    println!("🛑 销毁工作线程...");
                                    if let Some(tx) = &screen_control_tx {
                                        let _ = tx.send(ThreadControlSignal::Stop);
                                    }
                                    if let Some(tx) = &input_control_tx {
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
                                    screen_control_tx = None;
                                    screen_data_rx = None;
                                    input_control_tx = None;
                                    input_event_tx = None;
                                }
                                
                                // 转发输入事件到输入处理线程
                                WebSocketMessage::MouseEvent(_) | WebSocketMessage::KeyboardEvent(_) => {
                                    if let Some(tx) = &input_event_tx {
                                        let _ = tx.send(ws_msg);
                                    }
                                }
                                
                                WebSocketMessage::Error { message } => {
                                    println!("⚠️ 服务器错误: {}", message);
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
                            println!("❌ 无法解析服务器消息: {}", text);
                        }
                    }
                    Some(Ok(Message::Close(_))) => {
                        println!("📡 服务器关闭了连接");
                        break;
                    }
                    Some(Err(e)) => {
                        eprintln!("❌ WebSocket错误: {}", e);
                        break;
                    }
                    None => {
                        println!("📡 WebSocket连接已断开");
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
            
            // 接收来自屏幕捕获线程的数据并转发
            Some(screen_data) = async {
                if let Some(ref mut rx) = screen_data_rx {
                    rx.recv().await
                } else {
                    std::future::pending().await
                }
            } => {
                if let Ok(msg) = serde_json::to_string(&screen_data) {
                    if ws_sender.send(Message::Text(msg)).await.is_err() {
                        println!("❌ 发送屏幕数据失败，连接可能已断开");
                        break;
                    }
                }
            }
        }
    }
    
    // 清理资源
    println!("🧹 清理资源...");
    if let Some(tx) = &screen_control_tx {
        let _ = tx.send(ThreadControlSignal::Stop);
    }
    if let Some(tx) = &input_control_tx {
        let _ = tx.send(ThreadControlSignal::Stop);
    }
    
    if let Some(handle) = screen_thread_handle.take() {
        handle.abort();
    }
    if let Some(handle) = input_thread_handle.take() {
        handle.abort();
    }
    
    println!("👋 远程控制服务已退出");
    Ok(())
} 