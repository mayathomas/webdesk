use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use url::Url;
use tauri::{AppHandle, Emitter};

use crate::state::AppState;
use crate::types::*;
use crate::input::InputController;
use crate::webrtc::WebRTCClient;
use crate::video_encoder::NetworkQuality;

/// 运行H.264远程控制服务
pub async fn run_remote_service(state: AppState, app: AppHandle) -> Result<()> {
    log::info!("💻 启动H.264远程控制客户端 (WebRTC视频流架构)...");
    
    // 发送启动状态
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
    
    log::info!("⚙️ H.264客户端配置:");
    log::info!("   📡 信令服务器地址: {}", config.server_url);
    log::info!("   🏠 MAC地址: {}", mac_address);
    log::info!("   🔑 验证码: {}", config.auth_code);
    if let Some(client_id) = &config.client_id {
        log::info!("   🆔 客户端ID: {}", client_id);
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
    
    log::info!("🔌 正在连接到信令服务器: {}", url);
    
    let (ws_stream, _) = match connect_async(url).await {
        Ok(result) => result,
        Err(e) => {
            log::error!("❌ 连接信令服务器失败: {}", e);
            let _ = app.emit("status-update", serde_json::json!({
                "state": "连接服务器失败",
                "running": false
            }));
            return Err(e.into());
        }
    };
    
    log::debug!("✅ 信令WebSocket连接已建立");
    
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
    log::debug!("📤 发送注册请求: {}", register_msg);
    ws_sender.send(Message::Text(register_msg)).await?;
    
    let _ = app.emit("status-update", serde_json::json!({
        "state": "WaitingForRegistration",
        "running": true
    }));
    
    // 初始化输入控制器
    let input_controller = Arc::new(InputController::new());
    
    // 状态管理
    let mut client_state = ClientState::WaitingForRegistration;
    let mut input_thread_handle: Option<tokio::task::JoinHandle<()>> = None;
    
    // WebRTC H.264视频流客户端
    let mut webrtc_client: Option<WebRTCClient> = None;
    
    // 输入事件处理通道
    let mut input_control_tx: Option<tokio::sync::mpsc::UnboundedSender<ThreadControlSignal>> = None;
    let mut input_event_tx: Option<tokio::sync::mpsc::UnboundedSender<WebSocketMessage>> = None;
    
    // 创建信令通道
    let (signaling_tx, mut signaling_rx) = tokio::sync::mpsc::unbounded_channel::<WebSocketMessage>();
    
    // 数据通道就绪通知通道
    let (data_channel_ready_tx, mut data_channel_ready_rx) = tokio::sync::mpsc::unbounded_channel::<()>();
    
    log::debug!("🏗️ H.264 WebRTC架构已初始化：纯视频流传输...");
    
    // 主线程：处理信令WebSocket连接和WebRTC协商
    loop {
        tokio::select! {
            // 处理信令WebSocket消息
            msg = ws_receiver.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        log::debug!("📥 收到信令服务器消息: {}", text);
                        if let Ok(ws_msg) = serde_json::from_str::<WebSocketMessage>(&text) {
                            log::debug!("📦 解析消息类型: {:?}", std::mem::discriminant(&ws_msg));
                            match ws_msg {
                                WebSocketMessage::RegisterResponse(response) => {
                                    if response.success {
                                        log::debug!("🎉 注册成功，客户端ID: {}", response.client_id);
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
                                        log::error!("❌ 注册失败: {}", response.message);
                                    }
                                }
                                
                                WebSocketMessage::BrowserConnected { message } => {
                                    log::info!("🌐 浏览器开始WebRTC连接: {}", message);
                                    client_state = ClientState::WebRTCConnecting;
                                    
                                    // 初始化H.264 WebRTC客户端
                                    if webrtc_client.is_none() {
                                        if let Some(client_id) = &config.client_id {
                                            match WebRTCClient::new(client_id.clone(), signaling_tx.clone(), Some(data_channel_ready_tx.clone())).await {
                                                Ok(mut client) => {
                                                    // 设置事件处理器
                                                    if let Err(e) = client.setup_handlers().await {
                                                        log::error!("❌ 设置WebRTC处理器失败: {}", e);
                                                    } else {
                                                        // 初始化H.264视频流 (默认标准质量)
                                                        if let Err(e) = client.initialize_video_stream(NetworkQuality::Good).await {
                                                            log::error!("❌ 初始化H.264视频流失败: {}", e);
                                                    } else {
                                                        webrtc_client = Some(client);
                                                            log::info!("✅ H.264 WebRTC客户端已初始化并启动视频流");
                                                        }
                                                    }
                                                }
                                                Err(e) => {
                                                    log::error!("❌ 创建WebRTC客户端失败: {}", e);
                                                }
                                            }
                                        }
                                    }
                                }
                                
                                // WebRTC信令处理
                                WebSocketMessage::WebRTCOffer { session_description, .. } => {
                                    log::info!("📡 收到WebRTC Offer，准备建立H.264视频连接");
                                    if let Some(ref mut client) = webrtc_client {
                                        if let Err(e) = client.handle_offer(session_description).await {
                                            log::error!("❌ 处理WebRTC Offer失败: {}", e);
                                        } else {
                                            // 不要在这里设置为Connected，要等实际的WebRTC连接建立
                                            log::info!("🎯 WebRTC SDP协商完成，等待ICE连接建立...");
                                        }
                                    }
                                }
                                
                                WebSocketMessage::WebRTCIceCandidate { ice_candidate, .. } => {
                                    log::debug!("🧊 收到ICE候选");
                                    if let Some(ref client) = webrtc_client {
                                        if let Err(e) = client.handle_ice_candidate(ice_candidate).await {
                                            log::error!("❌ 处理ICE候选失败: {}", e);
                                        }
                                    }
                                }
                                
                                WebSocketMessage::BrowserDisconnected { message } => {
                                    log::debug!("🌐 浏览器已断开WebRTC连接: {}", message);
                                    client_state = ClientState::WaitingForBrowser;
                                    
                                    // 关闭WebRTC连接
                                    if let Some(client) = webrtc_client.take() {
                                        let _ = client.close().await;
                                    }
                                    
                                    // 停止工作线程
                                    stop_worker_threads(
                                        &mut input_thread_handle,
                                        &mut input_control_tx,
                                        &mut input_event_tx,
                                    );
                                }
                                
                                // H.264视频流配置处理
                                WebSocketMessage::VideoStreamConfig { config, .. } => {
                                    log::info!("🎬 收到H.264视频流配置: {}x{}@{:.1}fps, {}kbps", 
                                        config.width, config.height, config.fps, config.bitrate / 1000);
                                        
                                    if let Some(ref mut client) = webrtc_client {
                                        // 应用新的视频配置
                                        let quality = match config.bitrate {
                                            rate if rate <= 2000000 => NetworkQuality::Poor,
                                            rate if rate <= 3000000 => NetworkQuality::Good,
                                            _ => NetworkQuality::Excellent,
                                        };
                                        
                                        if let Err(e) = client.update_video_quality(quality).await {
                                            log::error!("❌ 更新H.264视频质量失败: {}", e);
                                        } else {
                                            log::info!("✅ H.264视频质量已更新: {:?}", quality);
                                        }
                                    }
                                }
                                
                                // 强制关键帧处理
                                WebSocketMessage::ForceKeyframe { .. } => {
                                    log::info!("🔑 收到强制关键帧请求");
                                    
                                    if let Some(ref client) = webrtc_client {
                                        if let Err(e) = client.force_keyframe().await {
                                            log::error!("❌ 强制生成关键帧失败: {}", e);
                                        } else {
                                            log::info!("✅ 已强制生成H.264关键帧");
                                        }
                                    }
                                }
                                
                                // 在H.264模式下，输入事件直接通过WebRTC数据通道传输
                                // 这里的处理仅用于向后兼容或调试
                                WebSocketMessage::MouseEvent(_) | WebSocketMessage::KeyboardEvent(_) => {
                                    log::debug!("🔍 收到输入事件 (H.264模式下应通过WebRTC数据通道传输)");
                                    // 在纯H.264模式下，这些事件应该已经通过WebRTC数据通道处理了
                                }
                                
                                WebSocketMessage::Error { message } => {
                                    log::debug!("⚠️ 信令服务器错误: {}", message);
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
                            log::error!("❌ 无法解析信令服务器消息: {}", text);
                        }
                    }
                    Some(Ok(Message::Close(_))) => {
                        log::debug!("📡 信令服务器关闭了连接");
                        break;
                    }
                    Some(Err(e)) => {
                        log::error!("❌ 信令WebSocket错误: {}", e);
                        break;
                    }
                    None => {
                        log::debug!("📡 信令WebSocket连接已断开");
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
                        log::error!("❌ 发送信令消息失败，连接可能已断开");
                        break;
                    }
                }
            }
            
            // 等待数据通道就绪
            Some(_) = data_channel_ready_rx.recv() => {
                log::debug!("🎉 数据通道已就绪，启动输入处理！");
                
                // 现在才启动输入处理线程
                start_worker_threads(
                    &mut input_thread_handle,
                    &mut input_control_tx,
                    &mut input_event_tx,
                    input_controller.clone(),
                );
            }
        }
    }
    
    // 清理资源
    log::debug!("🧹 清理WebRTC和线程资源...");
    
    // 关闭WebRTC连接
    if let Some(client) = webrtc_client.take() {
        let _ = client.close().await;
    }
    
    // 停止工作线程
    stop_worker_threads(
        &mut input_thread_handle,
        &mut input_control_tx,
        &mut input_event_tx,
    );
    
    log::debug!("👋 WebRTC远程控制服务已退出");
    Ok(())
}

/// 启动工作线程
fn start_worker_threads(
    input_thread_handle: &mut Option<tokio::task::JoinHandle<()>>,
    input_control_tx: &mut Option<tokio::sync::mpsc::UnboundedSender<ThreadControlSignal>>,
    input_event_tx: &mut Option<tokio::sync::mpsc::UnboundedSender<WebSocketMessage>>,
    input_controller: Arc<InputController>,
) {
    if input_thread_handle.is_none() {
        // 创建输入事件处理通道系统
        let (i_ctrl_tx, i_ctrl_rx) = tokio::sync::mpsc::unbounded_channel::<ThreadControlSignal>();
        let (i_event_tx, i_event_rx) = tokio::sync::mpsc::unbounded_channel::<WebSocketMessage>();
        
        *input_control_tx = Some(i_ctrl_tx);
        *input_event_tx = Some(i_event_tx);
        
        log::debug!("🎮 启动输入事件处理线程...");
        *input_thread_handle = Some(tokio::spawn(async move {
            crate::threads::input_event_thread(
                input_controller,
                i_event_rx,
                i_ctrl_rx,
            ).await;
        }));
    }
    
    // 发送启动信号
    if let Some(tx) = input_control_tx {
        let _ = tx.send(ThreadControlSignal::Start);
    }
}

/// 停止工作线程
fn stop_worker_threads(
    input_thread_handle: &mut Option<tokio::task::JoinHandle<()>>,
    input_control_tx: &mut Option<tokio::sync::mpsc::UnboundedSender<ThreadControlSignal>>,
    input_event_tx: &mut Option<tokio::sync::mpsc::UnboundedSender<WebSocketMessage>>,
) {
    log::debug!("🛑 停止工作线程...");
    
    if let Some(tx) = input_control_tx {
        let _ = tx.send(ThreadControlSignal::Stop);
    }
    
    // 等待线程结束
    if let Some(handle) = input_thread_handle.take() {
        handle.abort();
    }
    
    // 清理通道
    *input_control_tx = None;
    *input_event_tx = None;
} 