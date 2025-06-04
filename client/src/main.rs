mod config;
mod input;
mod screen;

use anyhow::Result;
use config::ClientConfig;
use futures_util::{SinkExt, StreamExt};
use input::InputController;
use screen::ScreenCaptureService;
use screen::ChangedRegion;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use url::Url;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, State};

// 应用状态管理
#[derive(Debug)]
struct AppState {
    client_config: Arc<Mutex<ClientConfig>>,
    service_running: Arc<Mutex<bool>>,
    client_state: Arc<Mutex<ClientState>>,
    service_handle: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

// 重用服务端的消息类型定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterRequest {
    pub mac_address: String,
    pub auth_code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterResponse {
    pub client_id: String,
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenData {
    pub image_data: String,
    pub width: u32,
    pub height: u32,
    pub format: String, // "png", "jpeg", "diff"
    pub full_frame: bool, // 是否为完整帧
    pub changed_regions: Option<Vec<ChangedRegion>>, // 变化区域
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MouseEvent {
    pub x: f64,
    pub y: f64,
    pub button: String,
    pub event_type: String,
    pub scroll_delta: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyboardEvent {
    pub key: String,
    pub event_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WebSocketMessage {
    Register(RegisterRequest),
    ScreenData(ScreenData),
    MouseEvent(MouseEvent),
    KeyboardEvent(KeyboardEvent),
    RegisterResponse(RegisterResponse),
    BrowserConnected { message: String },
    BrowserDisconnected { message: String },
    Error { message: String },
    Ping,
    Pong,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
enum ClientState {
    WaitingForRegistration,
    WaitingForBrowser,
    BrowserConnected,
    Idle,
}

// RustDesk架构：线程控制信号
#[derive(Debug, Clone)]
enum ThreadControlSignal {
    Start,
    Stop,
}

// Tauri命令：获取客户端信息
#[tauri::command]
async fn get_client_info(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let config = state.client_config.lock().map_err(|e| e.to_string())?;
    
    let mac_address = config.get_mac_address().map_err(|e| e.to_string())?;
    
    // 如果没有客户端ID，尝试生成一个基于MAC地址的ID
    let client_id = if let Some(id) = &config.client_id {
        id.clone()
    } else {
        // 临时显示基于MAC地址的ID，直到服务器分配正式ID
        format!("temp-{}", &mac_address.replace(":", "")[0..8])
    };
    
    Ok(serde_json::json!({
        "clientId": client_id,
        "authCode": config.auth_code.clone(),
        "macAddress": mac_address,
        "version": "1.0.0"
    }))
}

// Tauri命令：获取服务状态
#[tauri::command]
async fn get_service_status(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let config = state.client_config.lock().map_err(|e| e.to_string())?;
    let client_state = state.client_state.lock().map_err(|e| e.to_string())?;
    let service_running = state.service_running.lock().map_err(|e| e.to_string())?;
    
    Ok(serde_json::json!({
        "client_id": config.client_id.clone(),
        "state": format!("{:?}", *client_state),
        "running": *service_running
    }))
}

// Tauri命令：启动远程控制服务
#[tauri::command]
async fn start_remote_service(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut service_running = state.service_running.lock().map_err(|e| e.to_string())?;
    
    if *service_running {
        return Err("服务已在运行中".to_string());
    }
    
    // 启动服务
    let app_state = AppState {
        client_config: state.client_config.clone(),
        service_running: state.service_running.clone(),
        client_state: state.client_state.clone(),
        service_handle: state.service_handle.clone(),
    };
    let service_app = app.clone();
    
    let handle = tokio::spawn(async move {
        if let Err(e) = run_remote_service(app_state, service_app).await {
            eprintln!("远程控制服务错误: {}", e);
        }
    });
    
    *state.service_handle.lock().map_err(|e| e.to_string())? = Some(handle);
    *service_running = true;
    
    Ok(())
}

// Tauri命令：停止远程控制服务
#[tauri::command]
async fn stop_remote_service(state: State<'_, AppState>) -> Result<(), String> {
    let mut service_running = state.service_running.lock().map_err(|e| e.to_string())?;
    let mut service_handle = state.service_handle.lock().map_err(|e| e.to_string())?;
    
    if let Some(handle) = service_handle.take() {
        handle.abort();
    }
    
    *service_running = false;
    *state.client_state.lock().map_err(|e| e.to_string())? = ClientState::Idle;
    
    Ok(())
}

// 运行远程控制服务（保持原有逻辑，添加GUI事件支持）
async fn run_remote_service(state: AppState, app: AppHandle) -> Result<()> {
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
                                            if let Err(e) = screen_capture_thread(s_ctrl_rx, s_data_tx) {
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
                                            input_event_thread(
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

// RustDesk架构：屏幕捕获线程（阻塞线程）
fn screen_capture_thread(
    mut control_rx: tokio::sync::mpsc::UnboundedReceiver<ThreadControlSignal>,
    screen_tx: tokio::sync::mpsc::UnboundedSender<WebSocketMessage>,
) -> Result<()> {
    println!("📷 屏幕捕获线程已启动 (RustDesk架构 + JPEG压缩 + 差分编码)");
    
    let mut capturer = ScreenCaptureService::create_capturer()?;
    let mut screen_service = ScreenCaptureService::new();
    let mut active = false;
    let mut last_capture = std::time::Instant::now();
    let capture_interval = Duration::from_millis(100); // 10 FPS
    
    loop {
        // 检查控制信号（非阻塞）
        if let Ok(signal) = control_rx.try_recv() {
            match signal {
                ThreadControlSignal::Start => {
                    println!("📷 屏幕捕获开始 (优化模式)");
                    active = true;
                }
                ThreadControlSignal::Stop => {
                    println!("📷 屏幕捕获停止");
                    break;
                }
            }
        }
        
        // 如果激活且达到捕获间隔
        if active && last_capture.elapsed() >= capture_interval {
            match screen_service.capture_screen_optimized(&mut capturer) {
                Ok((image_data, width, height, format, full_frame, changed_regions)) => {
                    // 保存用于日志的值
                    let log_format = format.clone();
                    let log_regions_count = changed_regions.as_ref().map(|r| r.len()).unwrap_or(0);
                    
                    let screen_data = WebSocketMessage::ScreenData(ScreenData {
                        image_data,
                        width,
                        height,
                        format,
                        full_frame,
                        changed_regions,
                    });
                    
                    if screen_tx.send(screen_data).is_err() {
                        println!("❌ 发送屏幕数据到主线程失败，主线程可能已断开");
                        break;
                    }
                    
                    // 只在有数据时打印日志（避免无变化时的日志垃圾）
                    if full_frame {
                        println!("📷 发送完整帧: {}x{} ({})", width, height, log_format);
                    } else if log_regions_count > 0 {
                        println!("📷 发送差分数据: {} 个变化区域", log_regions_count);
                    }
                    
                    last_capture = std::time::Instant::now();
                }
                Err(e) => {
                    eprintln!("📷 屏幕捕获失败: {}", e);
                }
            }
        }
        
        // 短暂休眠避免忙等待
        std::thread::sleep(Duration::from_millis(10));
    }
    
    println!("📷 屏幕捕获线程已停止");
    Ok(())
}

// RustDesk架构：输入事件处理线程（异步线程）
async fn input_event_thread(
    input_controller: Arc<InputController>,
    mut input_rx: tokio::sync::mpsc::UnboundedReceiver<WebSocketMessage>,
    mut control_rx: tokio::sync::mpsc::UnboundedReceiver<ThreadControlSignal>,
) {
    println!("🎮 输入事件处理线程已启动 (RustDesk架构)");
    
    let mut active = false;
    
    loop {
        tokio::select! {
            // 检查控制信号
            Some(signal) = control_rx.recv() => {
                match signal {
                    ThreadControlSignal::Start => {
                        println!("🎮 输入事件处理开始");
                        active = true;
                    }
                    ThreadControlSignal::Stop => {
                        println!("🎮 输入事件处理停止");
                        break;
                    }
                }
            }
            
            // 处理输入事件
            Some(input_msg) = input_rx.recv() => {
                if !active {
                    continue;
                }
                
                match input_msg {
                    WebSocketMessage::MouseEvent(mouse_event) => {
                        handle_mouse_event(mouse_event, &input_controller);
                    }
                    WebSocketMessage::KeyboardEvent(keyboard_event) => {
                        handle_keyboard_event(keyboard_event, &input_controller);
                    }
                    _ => {}
                }
            }
        }
    }
    
    println!("🎮 输入事件处理线程已停止");
}

fn handle_mouse_event(mouse_event: MouseEvent, input_controller: &InputController) {
    match mouse_event.event_type.as_str() {
        "click" => {
            if let Err(e) =
                input_controller.click_mouse(mouse_event.x, mouse_event.y, &mouse_event.button)
            {
                eprintln!("❌ 鼠标点击失败: {}", e);
            }
        }
        "move" => {
            if let Err(e) = input_controller.move_mouse(mouse_event.x, mouse_event.y) {
                eprintln!("❌ 鼠标移动失败: {}", e);
            }
        }
        "scroll" => {
            if let Some(delta) = mouse_event.scroll_delta {
                if let Err(e) = input_controller.scroll_mouse(mouse_event.x, mouse_event.y, delta) {
                    eprintln!("❌ 鼠标滚轮失败: {}", e);
                }
            }
        }
        _ => {}
    }
}

fn handle_keyboard_event(keyboard_event: KeyboardEvent, input_controller: &InputController) {
    match keyboard_event.event_type.as_str() {
        "press" => {
            if let Err(e) = input_controller.press_key(&keyboard_event.key) {
                eprintln!("❌ 按键按下失败: {}", e);
            }
        }
        "release" => {
            if let Err(e) = input_controller.release_key(&keyboard_event.key) {
                eprintln!("❌ 按键释放失败: {}", e);
            }
        }
        _ => {}
    }
}

fn main() -> Result<()> {
    // 初始化配置
    let client_config = ClientConfig::load()?;
    println!("{:?}", client_config);
    
    // 确保MAC地址能获取到
    if let Ok(mac) = client_config.get_mac_address() {
        println!("🏠 MAC地址: {}", mac);
    }
    
    // 确保验证码已生成
    println!("🔑 验证码: {}", client_config.auth_code);
    
    // 创建应用状态
    let app_state = AppState {
        client_config: Arc::new(Mutex::new(client_config)),
        service_running: Arc::new(Mutex::new(false)),
        client_state: Arc::new(Mutex::new(ClientState::Idle)),
        service_handle: Arc::new(Mutex::new(None)),
    };
    
    // 启动Tauri 2.0应用
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            get_client_info,
            get_service_status,
            start_remote_service,
            stop_remote_service
        ])
        .run(tauri::generate_context!())
        .expect("启动Tauri应用失败");
    
    Ok(())
}
