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
use crate::video_track::{VideoTrackManager, VideoTrackStats};
use crate::video_encoder::VideoEncoderConfig;
use serde_json;

/// SDP分析结果
#[derive(Debug, Default)]
struct SdpAnalysis {
    /// ICE候选总数
    total_candidates: usize,
    /// Host候选数量
    host_candidates: usize,
    /// Srflx候选数量  
    srflx_candidates: usize,
    /// Relay候选数量
    relay_candidates: usize,
    /// DTLS Setup方式
    dtls_setup: String,
}

/// WebRTC客户端状态
#[derive(Clone)]
pub struct WebRTCClient {
    pub peer_connection: Arc<RTCPeerConnection>,
    pub data_channel: Arc<Mutex<Option<Arc<RTCDataChannel>>>>,
    pub video_track_manager: Arc<Mutex<VideoTrackManager>>,
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
        log::info!("🌐 初始化WebRTC客户端...");
        
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
        
        // 从YAML配置文件加载ICE服务器配置
        let ice_servers = match crate::config::WebRtcConfig::load_from_file("webrtc-config.yaml") {
            Ok(config) => {
                log::info!("📋 已加载WebRTC配置文件");
                config.to_ice_servers()
            }
            Err(e) => {
                log::warn!("⚠️ 加载WebRTC配置失败，使用默认配置: {}", e);
                // 使用默认配置作为备份
                vec![
                    RTCIceServer {
                        urls: vec!["stun:stun.l.google.com:19302".to_owned()],
                        username: "".to_owned(),
                        credential: "".to_owned(),
                        credential_type: RTCIceCredentialType::Unspecified,
                    },
                    RTCIceServer {
                        urls: vec!["stun:stun.cloudflare.com:3478".to_owned()],
                        username: "".to_owned(),
                        credential: "".to_owned(),
                        credential_type: RTCIceCredentialType::Unspecified,
                    },
                ]
            }
        };
        
        log::info!("📋 ICE服务器配置完成: {} 个STUN服务器", ice_servers.len());
        
        // 创建PeerConnection
        let peer_connection = Arc::new(api.new_peer_connection(RTCConfiguration {
            ice_servers,
            ice_candidate_pool_size: 10,
            ice_transport_policy: webrtc::peer_connection::policy::ice_transport_policy::RTCIceTransportPolicy::All,
            ..Default::default()
        }).await?);
        
        log::info!("✅ WebRTC PeerConnection 已创建");
        
        // 设置视频轨道管理器
        let video_track_manager = Arc::new(Mutex::new(VideoTrackManager::new()));
        
        // 初始化默认视频轨道
        let video_track = {
            let mut manager = video_track_manager.lock().unwrap();
            let encoder_config = VideoEncoderConfig::default();
            manager.initialize(encoder_config)?
        };
        
        // 添加视频轨道到PeerConnection
        peer_connection.add_track(video_track).await?;
        log::info!("🎥 视频轨道已添加到PeerConnection");
        
        Ok(Self {
            peer_connection,
            data_channel: Arc::new(Mutex::new(None)),
            video_track_manager,
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
        let peer_connection_clone_state = self.peer_connection.clone();
        self.peer_connection.on_peer_connection_state_change(Box::new(move |state| {
            log::debug!("🔗 WebRTC连接状态变化: {:?}", state);
            match state {
                RTCPeerConnectionState::Connected => {
                    log::info!("🎉 WebRTC P2P连接已建立！数据通道应该可用了！");
                }
                RTCPeerConnectionState::Connecting => {
                    log::info!("🔄 WebRTC正在连接...");
                }
                RTCPeerConnectionState::Disconnected => {
                    log::warn!("⚠️ WebRTC连接已断开");
                }
                RTCPeerConnectionState::Failed => {
                    log::error!("❌ WebRTC连接失败");
                    
                    // 获取详细的连接失败信息
                    let pc = peer_connection_clone_state.clone();
                    tokio::spawn(async move {
                        log::debug!("🔍 尝试获取详细的WebRTC失败信息...");
                        
                        // 检查连接状态
                        log::debug!("🔗 当前连接状态: {:?}", pc.connection_state());
                        log::debug!("🧊 当前ICE连接状态: {:?}", pc.ice_connection_state());
                        log::debug!("📡 当前ICE收集状态: {:?}", pc.ice_gathering_state());
                        
                        // 获取本地和远程描述
                        if let Some(local_desc) = pc.local_description().await {
                            log::debug!("📤 本地描述: {:?}", local_desc.sdp);
                        }
                        if let Some(remote_desc) = pc.remote_description().await {
                            log::debug!("📥 远程描述: {:?}", remote_desc.sdp);
                        }

                    });
                }
                RTCPeerConnectionState::Closed => {
                    log::info!("🔒 WebRTC连接已关闭");
                }
                _ => {
                    log::debug!("🔍 WebRTC连接状态: {:?}", state);
                }
            }
            Box::pin(async {})
        }));
        
        // 监听ICE连接状态变化
        let peer_connection_clone_ice = self.peer_connection.clone();
        self.peer_connection.on_ice_connection_state_change(Box::new(move |state| {
            log::debug!("🧊 ICE连接状态变化: {:?}", state);
            match state {
                webrtc::ice_transport::ice_connection_state::RTCIceConnectionState::Checking => {
                    log::debug!("🔍 ICE正在检查连通性...");
                    
                    // 🔧 关键修复：在强制中继模式下，增加详细的ICE调试信息
                    let pc = peer_connection_clone_ice.clone();
                    tokio::spawn(async move {
                        // 等待一些时间让ICE检查进行
                        tokio::time::sleep(tokio::time::Duration::from_millis(2000)).await;
                        
                        log::debug!("🔍 强制中继模式下的ICE检查状态:");
                        log::debug!("   🔗 连接状态: {:?}", pc.connection_state());
                        log::debug!("   🧊 ICE连接状态: {:?}", pc.ice_connection_state());
                        log::debug!("   📡 ICE收集状态: {:?}", pc.ice_gathering_state());
                        
                        // 检查本地描述中的ICE参数
                        if let Some(local_desc) = pc.local_description().await {
                            let ice_lines: Vec<&str> = local_desc.sdp.lines()
                                .filter(|line| line.contains("ice-") || line.contains("candidate"))
                                .collect();
                            if !ice_lines.is_empty() {
                                log::debug!("   📤 本地ICE参数:");
                                for line in &ice_lines[..std::cmp::min(5, ice_lines.len())] {
                                    log::debug!("      {}", line);
                                }
                            }
                        }
                        
                        // 检查远程描述中的ICE参数
                        if let Some(remote_desc) = pc.remote_description().await {
                            let ice_lines: Vec<&str> = remote_desc.sdp.lines()
                                .filter(|line| line.contains("ice-") || line.contains("candidate"))
                                .collect();
                            if !ice_lines.is_empty() {
                                log::debug!("   📥 远程ICE参数:");
                                for line in &ice_lines[..std::cmp::min(5, ice_lines.len())] {
                                    log::debug!("      {}", line);
                                }
                            }
                        }
                    });
                }
                webrtc::ice_transport::ice_connection_state::RTCIceConnectionState::Connected => {
                    log::info!("🎉 ICE连接已建立！");
                    
                    // 🔧 新增：连接建立后分析连接模式
                    let pc = peer_connection_clone_ice.clone();
                    tokio::spawn(async move {
                        // 等待连接稳定
                        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
                        
                        Self::analyze_connection_mode(&pc).await;
                    });
                }
                webrtc::ice_transport::ice_connection_state::RTCIceConnectionState::Completed => {
                    log::info!("✅ ICE连接完成！");
                    
                    // 🔧 新增：连接完成后进行详细分析
                    let pc = peer_connection_clone_ice.clone();
                    tokio::spawn(async move {
                        // 等待连接完全稳定
                        tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;
                        
                        Self::analyze_connection_mode(&pc).await;
                        Self::display_ice_server_info().await;
                    });
                }
                webrtc::ice_transport::ice_connection_state::RTCIceConnectionState::Failed => {
                    log::error!("❌ ICE连接失败！");
                    
                    // 🔧 增强失败分析：特别针对强制中继模式
                    let pc = peer_connection_clone_ice.clone();
                    tokio::spawn(async move {
                        log::debug!("🔍 强制中继模式ICE失败详细分析:");
                        log::debug!("   🔗 连接状态: {:?}", pc.connection_state());
                        log::debug!("   🧊 ICE连接状态: {:?}", pc.ice_connection_state());
                        log::debug!("   📡 ICE收集状态: {:?}", pc.ice_gathering_state());
                        
                        // 分析本地和远程的relay候选
                        if let Some(local_desc) = pc.local_description().await {
                            let relay_candidates: Vec<&str> = local_desc.sdp.lines()
                                .filter(|line| line.contains("typ relay"))
                                .collect();
                            log::debug!("   📤 本地relay候选数量: {}", relay_candidates.len());
                            for (i, candidate) in relay_candidates.iter().enumerate() {
                                log::debug!("      {}: {}", i+1, candidate);
                            }
                        }
                        
                        if let Some(remote_desc) = pc.remote_description().await {
                            let relay_candidates: Vec<&str> = remote_desc.sdp.lines()
                                .filter(|line| line.contains("typ relay"))
                                .collect();
                            log::debug!("   📥 远程relay候选数量: {}", relay_candidates.len());
                            for (i, candidate) in relay_candidates.iter().enumerate() {
                                log::debug!("      {}: {}", i+1, candidate);
                            }
                        }

                    });
                }
                webrtc::ice_transport::ice_connection_state::RTCIceConnectionState::Disconnected => {
                    log::warn!("⚠️ ICE连接已断开");
                }
                webrtc::ice_transport::ice_connection_state::RTCIceConnectionState::Closed => {
                    log::info!("🔒 ICE连接已关闭");
                }
                _ => {}
            }
            Box::pin(async {})
        }));
        
        // 监听ICE收集状态变化
        self.peer_connection.on_ice_gathering_state_change(Box::new(move |state| {
            log::debug!("📡 ICE收集状态变化: {:?}", state);
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
                    log::debug!("🧊 收集到ICE候选: {}", candidate_string);
                    
                    // 🔧 关键修复：解析并重新构造标准的SDP候选格式
                    let formatted_candidate = parse_and_format_candidate(&candidate_string);
                    
                    // 只发送有效的候选（非空字符串）
                    if !formatted_candidate.is_empty() {
                        log::debug!("🧊 发送标准SDP候选: {}", formatted_candidate);
                        
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
                            log::error!("❌ 发送ICE候选失败: {}", e);
                        }
                    } else {
                        log::debug!("⚠️ 跳过发送无效的ICE候选");
                    }
                } else {
                    log::debug!("🏁 ICE候选收集完成");
                }
            })
        }));
        
                // 监听数据通道（浏览器端创建的数据通道）
        let data_channel_ref = self.data_channel.clone();
        let ready_tx_clone = self.data_channel_ready_tx.clone();
        
        self.peer_connection.on_data_channel(Box::new(move |data_channel| {
            let data_channel = Arc::clone(&data_channel);
            log::info!("🎉 收到来自浏览器的数据通道: {} (状态: {:?})", 
                data_channel.label(), data_channel.ready_state());
            
            // 保存数据通道引用
            if let Ok(mut dc_ref) = data_channel_ref.lock() {
                *dc_ref = Some(Arc::clone(&data_channel));
                log::debug!("✅ 数据通道引用已保存到客户端");
            } else {
                log::error!("❌ 无法保存数据通道引用");
            }
            
            // 设置数据通道监听器
            let dc_clone_for_open = Arc::clone(&data_channel);
            let ready_tx = ready_tx_clone.clone();
            data_channel.on_open(Box::new(move || {
                log::info!("🚀 数据通道已打开，可以开始双向数据传输！状态: {:?}", 
                    dc_clone_for_open.ready_state());
                log::info!("🎉 现在可以开始屏幕捕获和传输了！");
                
                // 通知主线程数据通道已就绪
                if let Some(ref tx) = ready_tx {
                    let _ = tx.send(());
                }
                
                Box::pin(async {})
            }));
            
            let dc_clone_for_close = Arc::clone(&data_channel);
            data_channel.on_close(Box::new(move || {
                log::info!("🔒 数据通道已关闭，状态: {:?}", 
                    dc_clone_for_close.ready_state());
                Box::pin(async {})
            }));
            
            let dc_clone_for_error = Arc::clone(&data_channel);
            data_channel.on_error(Box::new(move |err| {
                log::error!("❌ 数据通道错误: {:?}, 状态: {:?}", 
                    err, dc_clone_for_error.ready_state());
                Box::pin(async {})
            }));
            
            let dc_clone = Arc::clone(&data_channel);
            data_channel.on_message(Box::new(move |msg| {
                let dc = Arc::clone(&dc_clone);
                Box::pin(async move {
                    log::debug!("📨 收到浏览器数据: {} bytes", msg.data.len());
                    handle_data_channel_message(dc, msg).await;
                })
            }));
            
            Box::pin(async {})
        }));
        
        log::debug!("✅ 数据通道监听器已设置");
        
        Ok(())
    }
    
    /// 处理WebRTC Offer
    pub async fn handle_offer(&mut self, session_description: SessionDescription) -> Result<()> {
        log::info!("📥 处理WebRTC Offer");
        
        let offer = RTCSessionDescription::offer(session_description.sdp)?;
        self.peer_connection.set_remote_description(offer).await?;
        
        // 创建Answer
        let answer = self.peer_connection.create_answer(None).await?;
        
        // 🔧 使用正确的事件驱动ICE候选收集
        // 设置ICE候选收集监听器
        let (ice_complete_tx, mut ice_complete_rx) = tokio::sync::mpsc::unbounded_channel::<()>();
        
        let ice_complete_tx_clone = ice_complete_tx.clone();
        self.peer_connection.on_ice_candidate(Box::new(move |candidate| {
            if let Some(candidate) = candidate {
                log::debug!("🧊 收集到ICE候选: {}:{} {}", candidate.address, candidate.port, candidate.typ);
            } else {
                log::debug!("✅ ICE候选收集完成 (收到null候选)");
                // 收到null候选表示收集完成
                if let Err(_) = ice_complete_tx_clone.send(()) {
                    log::debug!("⚠️ ICE完成通知发送失败");
                }
            }
            
            Box::pin(async {})
        }));
        
        // 设置本地描述，这会触发ICE候选收集
        self.peer_connection.set_local_description(answer.clone()).await?;
        
        // 异步等待ICE收集完成并发送Answer
        let peer_connection_clone = self.peer_connection.clone();
        let signaling_tx_clone = self.signaling_tx.clone();
        let client_id_clone = self.client_id.clone();
        
        tokio::spawn(async move {
            log::debug!("⏳ 等待ICE候选收集完成...");
            
            // 设置超时保护
            let timeout_duration = tokio::time::Duration::from_secs(10);
            
            let result = tokio::time::timeout(timeout_duration, async {
                // 等待ICE收集完成信号
                ice_complete_rx.recv().await
            }).await;
            
            match result {
                Ok(Some(())) => {
                    log::debug!("✅ ICE候选收集完成，准备发送Answer");
                }
                Ok(None) => {
                    log::debug!("⚠️ ICE收集通道已关闭");
                }
                Err(_) => {
                    log::debug!("⚠️ ICE候选收集超时(10s)，强制发送Answer");
                }
            }
            
            // 获取包含ICE候选的最终本地描述
            if let Some(local_desc) = peer_connection_clone.local_description().await {
                // 分析SDP内容
                let analysis = Self::analyze_sdp(&local_desc.sdp);
                
                log::debug!("📋 最终Answer SDP分析:");
                log::debug!("   🧊 ICE候选总数: {}", analysis.total_candidates);
                log::debug!("   🏠 Host候选: {}", analysis.host_candidates);
                log::debug!("   🌐 Srflx候选: {}", analysis.srflx_candidates);
                log::debug!("   🔄 Relay候选: {}", analysis.relay_candidates);
                log::debug!("   🔧 DTLS Setup: {}", analysis.dtls_setup);
                
                // 如果没有候选但ICE状态是Complete，可能是webrtc-rs的bug
                if analysis.total_candidates == 0 {
                    log::warn!("⚠️ 警告: SDP中没有ICE候选，这可能是webrtc-rs的已知问题");
                    log::warn!("💡 建议: 检查ICE服务器配置或网络连接");
                }
                
                // 发送包含ICE候选的Answer
                let answer_msg = WebSocketMessage::WebRTCAnswer {
                    target_id: client_id_clone,
                    session_description: SessionDescription {
                        sdp_type: "answer".to_string(),
                        sdp: local_desc.sdp,
                    },
                };
                
                match signaling_tx_clone.send(answer_msg) {
                    Ok(_) => {
                        log::info!("📤 WebRTC Answer发送成功 (包含{}个ICE候选)", analysis.total_candidates);
                    }
                    Err(e) => {
                        log::error!("❌ Answer发送失败: {}", e);
                    }
                }
            } else {
                log::error!("❌ 无法获取本地描述，Answer发送失败");
            }
        });
        
        log::info!("🎯 WebRTC连接协商已启动，等待ICE候选收集...");
        Ok(())
    }
    
    /// 分析SDP内容
    fn analyze_sdp(sdp: &str) -> SdpAnalysis {
        let mut analysis = SdpAnalysis::default();
        
        for line in sdp.lines() {
            if line.contains("a=candidate:") {
                analysis.total_candidates += 1;
                
                if line.contains("typ host") {
                    analysis.host_candidates += 1;
                } else if line.contains("typ srflx") {
                    analysis.srflx_candidates += 1;
                } else if line.contains("typ relay") {
                    analysis.relay_candidates += 1;
                }
            } else if line.contains("a=setup:") {
                if line.contains("active") {
                    analysis.dtls_setup = "active".to_string();
                } else if line.contains("passive") {
                    analysis.dtls_setup = "passive".to_string();
                } else if line.contains("actpass") {
                    analysis.dtls_setup = "actpass".to_string();
                }
            }
        }
        
        analysis
    }
    
    /// 🔧 新增：分析连接模式（P2P vs Relay）
    async fn analyze_connection_mode(peer_connection: &Arc<RTCPeerConnection>) {
        log::info!("🔍 ========== WebRTC连接模式分析 ==========");
        
        // 获取本地和远程描述进行分析
        let local_analysis = if let Some(local_desc) = peer_connection.local_description().await {
            let analysis = Self::analyze_sdp(&local_desc.sdp);
            log::info!("📤 本地SDP分析:");
            log::info!("   🏠 Host候选: {}", analysis.host_candidates);
            log::info!("   🌐 Srflx候选: {}", analysis.srflx_candidates);
            log::info!("   🔄 Relay候选: {}", analysis.relay_candidates);
            log::info!("   📊 总候选数: {}", analysis.total_candidates);
            Some(analysis)
        } else {
            log::warn!("⚠️ 无法获取本地SDP描述");
            None
        };
        
        let remote_analysis = if let Some(remote_desc) = peer_connection.remote_description().await {
            let analysis = Self::analyze_sdp(&remote_desc.sdp);
            log::info!("📥 远程SDP分析:");
            log::info!("   🏠 Host候选: {}", analysis.host_candidates);
            log::info!("   🌐 Srflx候选: {}", analysis.srflx_candidates);
            log::info!("   🔄 Relay候选: {}", analysis.relay_candidates);
            log::info!("   📊 总候选数: {}", analysis.total_candidates);
            Some(analysis)
        } else {
            log::warn!("⚠️ 无法获取远程SDP描述");
            None
        };
        
        // 分析连接模式
        if let (Some(local), Some(remote)) = (local_analysis, remote_analysis) {
            let total_relay = local.relay_candidates + remote.relay_candidates;
            let total_host = local.host_candidates + remote.host_candidates;
            let total_srflx = local.srflx_candidates + remote.srflx_candidates;
            
            log::info!("🎯 连接模式分析结果:");
            
            // 判断连接模式
            if total_relay > 0 && (total_host == 0 && total_srflx == 0) {
                log::info!("   🔄 连接模式: **RELAY模式** (通过TURN服务器中继)");
                log::info!("   📍 说明: 双方都在NAT后面，无法直连，所有流量通过TURN服务器转发");
            } else if total_host > 0 && total_relay == 0 {
                log::info!("   🏠 连接模式: **直连P2P模式** (Host-to-Host)");
                log::info!("   📍 说明: 双方在同一网络或公网，可以直接连接");
            } else if total_srflx > 0 && total_relay == 0 {
                log::info!("   🌐 连接模式: **NAT穿透P2P模式** (通过STUN辅助)");
                log::info!("   📍 说明: 通过STUN服务器辅助NAT穿透，实现P2P直连");
            } else if total_relay > 0 && (total_host > 0 || total_srflx > 0) {
                log::info!("   🔀 连接模式: **混合模式** (TURN备用 + P2P优先)");
                log::info!("   📍 说明: 优先尝试P2P连接，TURN作为备用");
            } else {
                log::info!("   ❓ 连接模式: **未知模式**");
            }
            
            // 显示数据传输路径
            log::info!("📊 ICE候选统计:");
            log::info!("   🏠 Host (本地网络): {} 个", total_host);
            log::info!("   🌐 Srflx (STUN辅助): {} 个", total_srflx);
            log::info!("   🔄 Relay (TURN中继): {} 个", total_relay);
            
            // 性能和延迟预估
            if total_relay > 0 && total_host == 0 && total_srflx == 0 {
                log::info!("⚡ 性能预估: 中等延迟 (通过TURN服务器转发)");
            } else if total_host > 0 || total_srflx > 0 {
                log::info!("⚡ 性能预估: 低延迟 (P2P直连)");
            }
        }
        
        log::info!("=======================================");
    }
    
    /// 🔧 新增：显示当前使用的ICE服务器信息
    async fn display_ice_server_info() {
        log::info!("🌐 ========== ICE服务器配置信息 ==========");
        
        // 从配置文件读取ICE服务器信息
        match crate::config::WebRtcConfig::load_from_file("webrtc-config.yaml") {
            Ok(config) => {
                log::info!("📋 当前ICE服务器配置:");
                
                // 显示STUN服务器
                if !config.stun_servers.is_empty() {
                    log::info!("🎯 STUN服务器 ({} 个):", config.stun_servers.len());
                    for (i, stun) in config.stun_servers.iter().enumerate() {
                        log::info!("   {}. {}", i + 1, stun.url);
                    }
                } else {
                    log::info!("   📍 未配置STUN服务器");
                }
                
                // 显示TURN服务器
                if !config.turn_servers.is_empty() {
                    log::info!("🔄 TURN服务器 ({} 个):", config.turn_servers.len());
                    for (i, turn) in config.turn_servers.iter().enumerate() {
                        log::info!("   {}. {} (用户名: {})", i + 1, turn.url, turn.username);
                    }
                } else {
                    log::info!("   📍 未配置TURN服务器");
                }
                
                // 显示RTC配置
                log::info!("⚙️ RTC配置:");
                log::info!("   🧊 ICE候选池大小: {}", config.rtc_config.ice_candidate_pool_size);
                log::info!("   📦 Bundle策略: {}", config.rtc_config.bundle_policy);
                log::info!("   🔀 RTCP Mux策略: {}", config.rtc_config.rtcp_mux_policy);
                
                // 显示服务器连接配置
                log::info!("🔗 信令服务器配置:");
                log::info!("   🌐 服务器URL: {}", config.server_config.server_url);
                log::info!("   🆔 客户端ID: {}", config.server_config.client_id);
            }
            Err(e) => {
                log::warn!("⚠️ 无法读取配置文件: {}", e);
                log::info!("📋 使用默认STUN服务器:");
                log::info!("   1. stun:stun.l.google.com:19302");
                log::info!("   2. stun:stun.cloudflare.com:3478");
            }
        }
        
        log::info!("=========================================");
    }
    
    /// 处理ICE候选
    pub async fn handle_ice_candidate(&self, ice_candidate: IceCandidate) -> Result<()> {
        // 详细解析ICE候选信息
        let candidate_str = &ice_candidate.candidate;
        
        // 提取ICE候选的类型和地址信息用于调试
        if candidate_str.contains("typ host") {
            log::debug!("🏠 添加Host候选: {}", candidate_str);
        } else if candidate_str.contains("typ srflx") {
            log::debug!("🌐 添加Srflx候选: {}", candidate_str);
        } else if candidate_str.contains("typ relay") {
            log::debug!("🔄 添加Relay候选: {}", candidate_str);
        } else {
            log::debug!("🧊 添加未知类型候选: {}", candidate_str);
        }
        
        let candidate = RTCIceCandidateInit {
            candidate: ice_candidate.candidate.clone(),
            sdp_mid: ice_candidate.sdp_mid.clone(),
            sdp_mline_index: ice_candidate.sdp_mline_index,
            username_fragment: None,
        };
        
        match self.peer_connection.add_ice_candidate(candidate).await {
            Ok(_) => {
                log::debug!("✅ ICE候选添加成功: mid={:?}, mline_index={:?}", 
                    ice_candidate.sdp_mid, ice_candidate.sdp_mline_index);
            }
            Err(e) => {
                log::error!("❌ ICE候选添加失败: {} - 候选: {}", e, candidate_str);
                return Err(e.into());
            }
        }
        
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
                            log::debug!("📤 通过WebRTC发送屏幕数据: {}x{}, 数据大小: {} bytes", 
                                screen_data.width, screen_data.height, total_size);
                        }
                        Err(e) => {
                            log::error!("❌ 通过WebRTC发送屏幕数据失败: {}", e);
                            return Err(e.into());
                        }
                    }
                } else {
                    // 数据太大，需要分片发送
                    log::debug!("📦 数据过大({} bytes)，开始分片发送...", total_size);
                    
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
                    
                    log::debug!("📏 分片计算 (RFC 8831标准):");
                    log::debug!("   - RFC推荐最大消息大小: {} bytes (16KB)", MAX_SAFE_MESSAGE_SIZE);
                    log::debug!("   - 协议头开销: {} bytes", header_overhead);
                    log::debug!("   - 安全余量: {} bytes", safety_margin);
                    log::debug!("   - 每片净数据空间: {} bytes", net_data_per_chunk);
                    log::debug!("   - 总分片数: {}", total_chunks);
                    log::debug!("   - 平均每片数据: {} bytes", avg_data_per_chunk);
                    
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
                        
                        // 🔧 增强调试：详细打印分片信息
                        log::debug!("🔍 分片 {}/{} 详细信息:", chunk_index + 1, total_chunks);
                        log::debug!("   📋 头部JSON: {}", header_json);
                        log::debug!("   📏 头部长度: {} bytes", header_bytes.len());
                        log::debug!("   📦 数据偏移: {} - {}", offset, end_offset);
                        log::debug!("   📊 数据大小: {} bytes", actual_chunk_size);
                        
                        // 构建完整的分片消息：[2字节头长度][头数据][分片数据]
                        let header_len = header_bytes.len() as u16;
                        let mut chunk_message = Vec::with_capacity(2 + header_bytes.len() + chunk_data.len());
                        chunk_message.extend_from_slice(&header_len.to_le_bytes());
                        chunk_message.extend_from_slice(header_bytes);
                        chunk_message.extend_from_slice(chunk_data);
                        
                        let total_chunk_size = chunk_message.len();
                        
                        // 🔧 增强调试：验证分片格式
                        log::debug!("   🔧 头长度字节: {:?}", header_len.to_le_bytes());
                        log::debug!("   📐 总分片大小: {} bytes (头长度: 2, 头数据: {}, 分片数据: {})", 
                            total_chunk_size, header_bytes.len(), chunk_data.len());
                        
                        // 🔧 数据完整性检查
                        if chunk_data.len() != actual_chunk_size {
                            log::error!("❌ 数据大小不匹配！期望: {}, 实际: {}", actual_chunk_size, chunk_data.len());
                            return Err(anyhow::anyhow!("数据大小不匹配"));
                        }
                        
                        // 发送分片
                        match data_channel.send(&chunk_message.into()).await {
                            Ok(_) => {
                                log::debug!("📤 分片 {}/{} 发送成功 (总: {} bytes，头: {} bytes，数据: {} bytes)", 
                                    chunk_index + 1, total_chunks, total_chunk_size, 
                                    2 + header_bytes.len(), chunk_data.len());
                            }
                            Err(e) => {
                                log::error!("❌ 分片 {}/{} 发送失败: {}", chunk_index + 1, total_chunks, e);
                                return Err(e.into());
                            }
                        }
                        
                        offset = end_offset;
                        
                        // 在分片之间添加小延迟，避免网络拥塞
                        if chunk_index < total_chunks - 1 {
                            tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
                        }
                    }
                    
                    log::debug!("✅ 分片发送完成: {} 个分片，总大小: {} bytes", total_chunks, total_size);
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
    
    /// 启动视频屏幕捕获
    pub async fn start_video_capture(&self) -> Result<()> {
        log::info!("🎬 启动视频屏幕捕获");
        
        use crate::screen_video::{VideoScreenCaptureService, CaptureConfig, QualityLevel};
        
        let mut capture_service = VideoScreenCaptureService::new(self.video_track_manager.clone());
        
        // 配置捕获参数
        let config = CaptureConfig {
            target_fps: 30,
            max_width: 1920,
            max_height: 1080,
            quality_level: QualityLevel::Medium,
        };
        capture_service.update_config(config);
        
        // 启动捕获线程
        if let Err(e) = capture_service.start_capture_thread() {
            log::error!("❌ 启动视频捕获线程失败: {}", e);
        }
        
        Ok(())
    }
    
    /// 获取视频统计信息
    pub async fn get_video_stats(&self) -> Option<VideoTrackStats> {
        let manager = self.video_track_manager.lock().unwrap();
        manager.get_stats().await
    }

    /// 关闭WebRTC连接
    pub async fn close(&self) -> Result<()> {
        log::info!("🔐 关闭WebRTC连接");
        
        // 停用视频轨道
        {
            let mut manager = self.video_track_manager.lock().unwrap();
            manager.deactivate();
        }
        
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
                    log::debug!("🖱️ 通过WebRTC收到鼠标事件: {:?}", mouse_event);
                    // 直接处理鼠标事件
                    let input_controller = crate::input::InputController::new();
                    crate::threads::handle_mouse_event(mouse_event, &input_controller);
                }
                WebSocketMessage::KeyboardEvent(keyboard_event) => {
                    log::debug!("⌨️ 通过WebRTC收到键盘事件: {:?}", keyboard_event);
                    // 直接处理键盘事件  
                    let input_controller = crate::input::InputController::new();
                    crate::threads::handle_keyboard_event(keyboard_event, &input_controller);
                }
                _ => {}
            }
        }
    }
}

/// 解析webrtc-rs的候选字符串并格式化为标准SDP格式
/// 修复webrtc-rs与浏览器的兼容性问题
/// 输入格式: "udp host 192.168.1.3:49796" 或 "udp srflx 121.227.207.147:60352"
/// 输出格式: "candidate:842163049 1 udp 1686052607 192.168.1.3 49796 typ host generation 0"
fn parse_and_format_candidate(candidate_str: &str) -> String {
    let parts: Vec<&str> = candidate_str.split_whitespace().collect();
    
    if parts.len() >= 3 {
        let transport = parts[0].to_lowercase(); // 使用小写，与浏览器保持一致
        let candidate_type = parts[1]; // host/srflx/relay等
        let address_port = parts[2]; // IP:PORT格式
        
        // 解析地址:端口，处理IPv6和webrtc-rs的bug格式
        let (ip, port) = parse_address_port(address_port);
        
        // 如果解析失败（返回空字符串），跳过该候选
        if ip.is_empty() || port.is_empty() {
            log::debug!("⚠️ 跳过无效的ICE候选: {}", candidate_str);
            return String::new(); // 返回空字符串，让上层跳过
        }
        
        // 验证端口是否有效
        if let Ok(port_num) = port.parse::<u16>() {
            // 🔧 修复webrtc-rs兼容性：使用标准的优先级计算
            // 按照RFC 5245标准计算优先级
            let priority = match candidate_type {
                "host" => {
                    // Type preference (126) + Local preference (65535) + Component (255)
                    2130706431_u32 // 标准的host候选优先级
                },
                "srflx" => {
                    // Server reflexive候选
                    1694498815_u32 // 标准的srflx候选优先级  
                },
                "relay" => {
                    // Relay候选（最低优先级）
                    16777215_u32 // 标准的relay候选优先级
                },
                _ => 1000000,
            };
            
            // 🔧 修复webrtc-rs兼容性：生成标准的foundation
            let foundation = generate_standard_foundation(&ip, candidate_type, &transport);
            
            // 🔧 修复webrtc-rs兼容性：严格按照浏览器期望的格式生成候选
            let candidate_format = match candidate_type {
                "relay" => {
                    // Relay候选必须包含raddr和rport（Chrome要求）
                    format!(
                        "candidate:{} 1 {} {} {} {} typ {} raddr 0.0.0.0 rport 0 generation 0",
                        foundation, transport, priority, ip, port_num, candidate_type
                    )
                },
                "srflx" => {
                    // Server reflexive候选需要raddr和rport信息
                    format!(
                        "candidate:{} 1 {} {} {} {} typ {} raddr {} rport {} generation 0",
                        foundation, transport, priority, ip, port_num, candidate_type,
                        ip, port_num // 使用相同地址作为related地址
                    )
                },
                _ => {
                    // Host候选的标准格式
                    format!(
                        "candidate:{} 1 {} {} {} {} typ {} generation 0",
                        foundation, transport, priority, ip, port_num, candidate_type
                    )
                }
            };
            
            log::debug!("🔧 标准SDP候选: {}", candidate_format);
            candidate_format
        } else {
            log::debug!("⚠️ 无效的端口号: {}, 跳过候选", port);
            String::new() // 返回空字符串，让上层跳过
        }
    } else {
        log::debug!("⚠️ 候选格式不正确: {}, 跳过", candidate_str);
        String::new()
    }
}

/// 生成标准的foundation值（与浏览器兼容）
fn generate_standard_foundation(ip: &str, candidate_type: &str, transport: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    
    let mut hasher = DefaultHasher::new();
    // 按照RFC 5245标准，foundation基于IP、传输协议和候选类型
    ip.hash(&mut hasher);
    transport.hash(&mut hasher);
    candidate_type.hash(&mut hasher);
    
    // 生成一个合理长度的foundation（通常8-10位数字）
    format!("{}", hasher.finish() % 4294967295) // 使用32位最大值
}

/// 解析地址:端口字符串，处理各种格式，包括webrtc-rs的bug格式
fn parse_address_port(address_port: &str) -> (String, String) {
    log::debug!("🔍 解析地址:端口: {}", address_port);
    
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
                        log::debug!("🔨 修复webrtc-rs IPv4 bug: {} -> {}:{}", address_port, ip, port);
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
                                    log::debug!("🔨 修复webrtc-rs IPv6 bug: {} -> IPv6 {}:{}", address_port, ip_part, potential_port);
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
    log::debug!("⚠️ 无法解析地址:端口 {}, 跳过该候选", address_port);
    ("".to_string(), "".to_string()) // 返回空字符串，让上层跳过
} 