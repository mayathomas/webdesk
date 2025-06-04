mod config;
mod input;
mod screen;

use anyhow::Result;
use config::ClientConfig;
use futures_util::{SinkExt, StreamExt};
use input::InputController;
use screen::ScreenCaptureService;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time::interval;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use url::Url;
use std::sync::Arc;

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

#[derive(Debug, Clone, PartialEq)]
enum ClientState {
    WaitingForRegistration,
    WaitingForBrowser,
    BrowserConnected,
}

// RustDesk架构：线程控制信号
#[derive(Debug, Clone)]
enum ThreadControlSignal {
    Start,
    Stop,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("💻 启动远程控制客户端 (RustDesk架构)...");
    
    // 加载配置
    let mut config = ClientConfig::load()?;
    
    // 运行时获取MAC地址
    let mac_address = config.get_mac_address()?;
    
    println!("⚙️ 客户端配置:");
    println!("   📡 服务器地址: {}", config.server_url);
    println!("   🏠 MAC地址: {}", mac_address);
    println!("   🔑 验证码: {}", config.auth_code);
    if let Some(client_id) = &config.client_id {
        println!("   🆔 客户端ID: {}", client_id);
    }
    
    // 连接到服务器
    let url = Url::parse(&config.server_url)?;
    println!("🔌 正在连接到服务器: {}", url);
    
    let (ws_stream, _) = connect_async(url).await?;
    println!("✅ WebSocket连接已建立");
    
    let (mut ws_sender, mut ws_receiver) = ws_stream.split();
    
    // 发送注册请求
    let register_request = WebSocketMessage::Register(RegisterRequest {
        mac_address: mac_address.clone(),
        auth_code: config.auth_code.clone(),
    });
    
    let register_msg = serde_json::to_string(&register_request)?;
    ws_sender.send(Message::Text(register_msg)).await?;
    
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
                        if let Ok(ws_msg) = serde_json::from_str::<WebSocketMessage>(&text) {
                            match ws_msg {
                                WebSocketMessage::RegisterResponse(response) => {
                                    if response.success {
                                        println!("🎉 注册成功，客户端ID: {}", response.client_id);
                                        config.update_client_id(response.client_id)?;
                                        client_state = ClientState::WaitingForBrowser;
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
        
        // 状态显示
        if client_state != ClientState::BrowserConnected {
            display_status(&client_state).await;
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
    
    println!("👋 客户端已退出");
    Ok(())
}

// RustDesk架构：屏幕捕获线程（阻塞线程）
fn screen_capture_thread(
    mut control_rx: tokio::sync::mpsc::UnboundedReceiver<ThreadControlSignal>,
    screen_tx: tokio::sync::mpsc::UnboundedSender<WebSocketMessage>,
) -> Result<()> {
    println!("📷 屏幕捕获线程已启动 (RustDesk架构)");
    
    let mut capturer = ScreenCaptureService::create_capturer()?;
    let mut active = false;
    let mut last_capture = std::time::Instant::now();
    let capture_interval = Duration::from_millis(100); // 10 FPS
    
    loop {
        // 检查控制信号（非阻塞）
        if let Ok(signal) = control_rx.try_recv() {
            match signal {
                ThreadControlSignal::Start => {
                    println!("📷 屏幕捕获开始");
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
            match ScreenCaptureService::capture_screen(&mut capturer) {
                Ok((image_data, width, height)) => {
                    let screen_data = WebSocketMessage::ScreenData(ScreenData {
                        image_data,
                        width,
                        height,
                    });
                    
                    if screen_tx.send(screen_data).is_err() {
                        println!("❌ 发送屏幕数据到主线程失败，主线程可能已断开");
                        break;
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

async fn display_status(state: &ClientState) {
    match state {
        ClientState::WaitingForRegistration => {
            println!("⏳ 等待注册完成...");
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        ClientState::WaitingForBrowser => {
            println!("⏳ 等待浏览器连接...");
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        ClientState::BrowserConnected => {
            // 连接状态下不需要额外延时
        }
    }
}
