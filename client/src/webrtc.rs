use anyhow::Result;
use webrtc::data_channel::RTCDataChannel;
use webrtc::peer_connection::configuration::RTCConfiguration;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::APIBuilder;
use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::interceptor::registry::Registry;
use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;

use crate::types::*;
use crate::video_stream_manager::{VideoStreamManager, StreamConfig};
use crate::video_encoder::NetworkQuality;
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

/// H.264 WebRTC客户端状态
#[derive(Clone)]
pub struct WebRTCClient {
    pub peer_connection: Arc<RTCPeerConnection>,
    pub data_channel: Arc<Mutex<Option<Arc<RTCDataChannel>>>>,
    pub video_stream_manager: Arc<Mutex<Option<VideoStreamManager>>>,
    pub signaling_tx: mpsc::UnboundedSender<WebSocketMessage>,
    pub client_id: String,
    pub data_channel_ready_tx: Option<mpsc::UnboundedSender<()>>,
}

impl WebRTCClient {
    /// 创建新的VP8 WebRTC客户端
    pub async fn new(
        client_id: String,
        signaling_tx: mpsc::UnboundedSender<WebSocketMessage>,
        data_channel_ready_tx: Option<mpsc::UnboundedSender<()>>,
    ) -> Result<Self> {
        log::info!("🌐 初始化WebRTC客户端...");
        
        // 创建媒体引擎并注册默认编解码器
        let mut media_engine = MediaEngine::default();
        media_engine.register_default_codecs()?;
        log::info!("✅ 已注册默认编解码器（H.264, VP8, Opus等）");
        
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
                log::warn!("⚠️ 加载WebRTC配置失败: {}", e);
                return Err(anyhow::anyhow!("⚠️ 加载WebRTC配置失败: {}", e));
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
        
        Ok(Self {
            peer_connection,
            data_channel: Arc::new(Mutex::new(None)),
            video_stream_manager: Arc::new(Mutex::new(None)),
            signaling_tx,
            client_id,
            data_channel_ready_tx,
        })
    }
    
    /// 初始化H.264视频流
    pub async fn initialize_video_stream(&self, quality: NetworkQuality) -> Result<()> {
        log::info!("🎬 初始化H.264视频流...");
        
        // 创建流配置
        let config = match quality {
            NetworkQuality::Excellent => StreamConfig::high_quality(),
            NetworkQuality::Good => StreamConfig::standard_quality(),
            NetworkQuality::Poor => StreamConfig::low_latency(),
        };
        
        // 创建视频流管理器
        let mut stream_manager = VideoStreamManager::new(config).await?;
        
        // 获取视频轨道
        let video_track = stream_manager.get_video_track().await;
        
        // 重要！：必须在SDP协商之前添加视频轨道到PeerConnection
        // 这是解决浏览器端ontrack事件不触发的关键！
        let rtp_sender = self.peer_connection.add_track(video_track).await?;
        log::info!("🎥 H.264视频轨道已添加到PeerConnection");
        
        // 启动RTPSender的读取任务（处理RTCP）
        tokio::spawn(async move {
            let mut rtcp_buf = vec![0u8; 1500];
            while let Ok((_, _)) = rtp_sender.read(&mut rtcp_buf).await {
                // 处理RTCP数据 - 这对于某些WebRTC实现是必需的
            }
        });
        
        // 启动视频流 - 但暂时不发送数据
        stream_manager.start().await?;
        log::info!("🚀 H.264视频流已启动（待连接建立后激活）");
        
        // 保存视频流管理器
        {
            let mut manager_guard = self.video_stream_manager.lock().await;
            *manager_guard = Some(stream_manager);
        }
        
        Ok(())
    }
    
    /// 设置WebRTC事件监听器
    pub async fn setup_handlers(&mut self) -> Result<()> {
        let signaling_tx = self.signaling_tx.clone();
        
        // 监听连接状态变化
        let peer_connection_clone_state = self.peer_connection.clone();
        let video_stream_manager_clone = Arc::clone(&self.video_stream_manager);
        
        self.peer_connection.on_peer_connection_state_change(Box::new(move |state| {
            log::info!("🔗 WebRTC连接状态变化: {:?}", state);
            let stream_manager = Arc::clone(&video_stream_manager_clone);
            
            match state {
                RTCPeerConnectionState::New => {
                    log::info!("🆕 WebRTC连接初始化");
                }
                RTCPeerConnectionState::Connecting => {
                    log::info!("🔄 WebRTC正在连接...");
                }
                RTCPeerConnectionState::Connected => {
                    log::info!("🎉 WebRTC P2P连接已建立！开始激活H.264视频流！");
                    
                    // 连接建立后，立即开始发送视频数据
                    tokio::spawn(async move {
                        // 稍等片刻确保连接完全稳定
                        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                        
                        // 检查并确保视频流管理器状态
                        let guard = stream_manager.lock().await;
                        if let Some(manager) = guard.as_ref() {
                            let state = manager.get_state().await;
                            log::info!("📊 连接建立后视频流状态: {:?}", state);
                            
                            let stats = manager.get_stats().await;
                            log::info!("📊 H.264流统计: {:.1}fps, {:.1}kbps, {}帧", 
                                stats.average_capture_fps, 
                                stats.average_bitrate_kbps,
                                stats.frames_transmitted);
                                
                            // 激活视频轨道开始发送数据
                            if let Err(e) = manager.activate_video_track().await {
                                log::error!("❌ 激活视频轨道失败: {}", e);
                            } else {
                                log::info!("🚀 视频轨道已激活");
                            }
                            
                            // 强制生成关键帧以确保浏览器能正确解码
                            if let Err(e) = manager.force_keyframe().await {
                                log::error!("❌ 强制生成关键帧失败: {}", e);
                            } else {
                                log::info!("🔑 已强制生成H.264关键帧");
                            }
                                
                            // 如果视频流未在运行，记录警告
                            if matches!(state, crate::video_stream_manager::StreamState::Stopped) {
                                log::warn!("⚠️ WebRTC连接已建立但视频流未启动！");
                            }
                        } else {
                            log::error!("❌ WebRTC连接建立但视频流管理器未初始化！");
                        }
                    });
                }
                RTCPeerConnectionState::Disconnected => {
                    log::warn!("⚠️ WebRTC连接已断开");
                }
                RTCPeerConnectionState::Failed => {
                    log::error!("❌ WebRTC连接失败");
                    
                    // 获取详细的连接失败信息
                    let pc = peer_connection_clone_state.clone();
                    tokio::spawn(async move {
                        log::error!("🔍 获取详细的WebRTC失败信息...");
                        
                        // 检查连接状态
                        log::error!("🔗 当前连接状态: {:?}", pc.connection_state());
                        log::error!("🧊 当前ICE连接状态: {:?}", pc.ice_connection_state());
                        log::error!("📡 当前ICE收集状态: {:?}", pc.ice_gathering_state());
                        
                        // 获取本地和远程描述
                        if let Some(local_desc) = pc.local_description().await {
                            log::debug!("📤 本地描述前100字符: {:.100}", local_desc.sdp);
                        } else {
                            log::error!("❌ 没有本地描述");
                        }
                        if let Some(remote_desc) = pc.remote_description().await {
                            log::debug!("📥 远程描述前100字符: {:.100}", remote_desc.sdp);
                        } else {
                            log::error!("❌ 没有远程描述");
                        }
                    });
                }
                RTCPeerConnectionState::Closed => {
                    log::info!("🔒 WebRTC连接已关闭");
                    
                    // 停止视频流
                    tokio::spawn(async move {
                        let mut manager_option = {
                            let mut guard = stream_manager.lock().await;
                            guard.take() // 取出Option<VideoStreamManager>
                        };
                        
                        if let Some(ref mut manager) = manager_option {
                            if let Err(e) = manager.stop().await {
                                log::error!("❌ 停止H.264视频流失败: {}", e);
                            }
                        }
                    });
                }
                _ => {
                    log::info!("🔍 WebRTC连接状态: {:?}", state);
                }
            }
            Box::pin(async {})
        }));
        
        // 监听ICE连接状态变化
        let peer_connection_clone_ice = self.peer_connection.clone();
        self.peer_connection.on_ice_connection_state_change(Box::new(move |state| {
            log::info!("🧊 ICE连接状态变化: {:?}", state);
            match state {
                webrtc::ice_transport::ice_connection_state::RTCIceConnectionState::New => {
                    log::info!("🆕 ICE连接初始化");
                }
                webrtc::ice_transport::ice_connection_state::RTCIceConnectionState::Checking => {
                    log::info!("🔍 ICE正在检查连通性...");
                    
                    let pc = peer_connection_clone_ice.clone();
                    tokio::spawn(async move {
                        // 等待一些时间让ICE检查进行
                        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
                        
                        // 分析当前的ICE候选
                        if let Some(local_desc) = pc.local_description().await {
                            let analysis = Self::analyze_sdp(&local_desc.sdp);
                            log::info!("🔍 本地SDP分析: Host:{} Srflx:{} Relay:{}", 
                                analysis.host_candidates, 
                                analysis.srflx_candidates, 
                                analysis.relay_candidates);
                        }
                    });
                }
                webrtc::ice_transport::ice_connection_state::RTCIceConnectionState::Connected => {
                    log::info!("🧊 ICE连接成功 - P2P通道已建立");
                }
                webrtc::ice_transport::ice_connection_state::RTCIceConnectionState::Completed => {
                    log::info!("🎯 ICE连接完成 - 最佳路径已选择");
                }
                webrtc::ice_transport::ice_connection_state::RTCIceConnectionState::Failed => {
                    log::error!("❌ ICE连接失败");
                }
                webrtc::ice_transport::ice_connection_state::RTCIceConnectionState::Disconnected => {
                    log::warn!("⚠️ ICE连接断开");
                }
                webrtc::ice_transport::ice_connection_state::RTCIceConnectionState::Closed => {
                    log::info!("🔒 ICE连接已关闭");
                }
                _ => {
                    log::info!("🔍 ICE连接状态: {:?}", state);
                }
            }
            Box::pin(async {})
        }));
        
        // 监听ICE候选生成
        let signaling_tx_ice = signaling_tx.clone();
        self.peer_connection.on_ice_candidate(Box::new(move |ice_candidate| {
            let tx = signaling_tx_ice.clone();
            Box::pin(async move {
                if let Some(candidate) = ice_candidate {
                    // 使用webrtc库提供的字段格式化候选信息
                    let candidate_string = format!("candidate:{} 1 {} {} {} {} typ {} generation 0",
                        candidate.foundation,
                        candidate.protocol.to_string().to_lowercase(),
                        candidate.priority,
                        candidate.address,
                        candidate.port,
                        candidate.typ.to_string().to_lowercase()
                    );
                    
                    log::debug!("🧊 生成ICE候选: {}", candidate_string);
                    
                    let ice_msg = WebSocketMessage::WebRTCIceCandidate {
                        target_id: "browser".to_string(),
                        ice_candidate: IceCandidate {
                            candidate: candidate_string,
                            sdp_mid: Some("0".to_string()), // 默认使用媒体线索引0
                            sdp_mline_index: Some(0),
                        },
                    };
                    
                    if let Err(e) = tx.send(ice_msg) {
                        log::error!("❌ 发送ICE候选失败: {}", e);
                    }
                } else {
                    log::debug!("🏁 ICE候选收集完成");
                }
            })
        }));
        
        // 设置数据通道处理 (用于输入事件传输)
        self.setup_data_channel_handlers().await?;
        
        log::info!("✅ WebRTC事件监听器已设置");
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
    
    /// 设置数据通道处理 (用于输入事件传输)
    async fn setup_data_channel_handlers(&self) -> Result<()> {
        // 监听数据通道（浏览器端创建的数据通道）
        let data_channel_ref = self.data_channel.clone();
        let ready_tx_clone = self.data_channel_ready_tx.clone();
        let video_stream_manager = Arc::clone(&self.video_stream_manager);
        
        self.peer_connection.on_data_channel(Box::new(move |data_channel| {
            let data_channel = Arc::clone(&data_channel);
            log::info!("🎉 收到来自浏览器的数据通道: {} (状态: {:?})", 
                data_channel.label(), data_channel.ready_state());
            
            // 保存数据通道引用
            let data_channel_clone = Arc::clone(&data_channel);
            let data_channel_ref_clone = data_channel_ref.clone();
            tokio::spawn(async move {
                let mut dc_ref = data_channel_ref_clone.lock().await;
                *dc_ref = Some(data_channel_clone);
                log::debug!("✅ 数据通道引用已保存到客户端");
            });
            
            // 设置数据通道监听器
            let dc_clone_for_open = Arc::clone(&data_channel);
            let ready_tx = ready_tx_clone.clone();
            let video_manager_for_dc = Arc::clone(&video_stream_manager);
            data_channel.on_open(Box::new(move || {
                log::info!("🚀 数据通道已打开，可以开始双向数据传输！状态: {:?}", 
                    dc_clone_for_open.ready_state());
                log::info!("🎉 现在可以开始屏幕捕获和传输了！");
                
                // 通知主线程数据通道已就绪
                if let Some(ref tx) = ready_tx {
                    let _ = tx.send(());
                }
                
                // 检查视频流状态
                let video_manager = Arc::clone(&video_manager_for_dc);
                tokio::spawn(async move {
                    let guard = video_manager.lock().await;
                    if let Some(manager) = guard.as_ref() {
                        let state = manager.get_state().await;
                        log::info!("📊 数据通道就绪时视频流状态: {:?}", state);
                        
                        if matches!(state, crate::video_stream_manager::StreamState::Stopped) {
                            log::warn!("⚠️ 数据通道已就绪但视频流未启动！这可能导致黑屏");
                        } else {
                            log::info!("✅ 视频流正在运行，应该可以看到画面");
                        }
                    } else {
                        log::warn!("⚠️ 数据通道已就绪但视频流管理器未初始化！");
                    }
                });
                
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
        
        // 分析收到的Offer SDP
        log::debug!("📥 收到的Offer SDP内容:");
        log::debug!("{}", session_description.sdp);
        
        let offer = RTCSessionDescription::offer(session_description.sdp)?;
        self.peer_connection.set_remote_description(offer).await?;
        
        // 检查PeerConnection当前的发送器状态
        let senders = self.peer_connection.get_senders().await;
        log::info!("📡 PeerConnection发送器数量: {}", senders.len());
        
        for (i, sender) in senders.iter().enumerate() {
            let track = sender.track().await;
            if let Some(track) = track {
                log::info!("📡 发送器 {}: 轨道类型={}, ID={}", i, track.kind(), track.id());
            } else {
                log::warn!("⚠️ 发送器 {} 没有关联的轨道", i);
            }
        }
        
        // 创建Answer
        let answer = self.peer_connection.create_answer(None).await?;
        
        // 详细分析生成的Answer SDP
        log::info!("📤 生成的Answer SDP分析:");
        let answer_sdp = &answer.sdp;
        log::debug!("📤 Answer SDP完整内容:");
        log::debug!("{}", answer_sdp);
        
        // 检查Answer中的关键信息
        let has_video = answer_sdp.contains("m=video");
        let has_h264 = answer_sdp.contains("h264") || answer_sdp.contains("H264");
        let video_lines: Vec<&str> = answer_sdp.lines()
            .filter(|line| line.starts_with("m=video") || line.contains("h264") || line.contains("H264"))
            .collect();
            
        log::info!("🎬 Answer SDP检查结果:");
        log::info!("  - 包含视频媒体: {}", has_video);
        log::info!("  - 包含H.264编解码器: {}", has_h264);
        log::info!("  - 相关SDP行: {:?}", video_lines);
        
        if !has_video {
            log::error!("❌ 生成的Answer SDP中没有视频媒体描述！这会导致浏览器黑屏！");
            log::error!("🔍 可能的原因:");
            log::error!("   1. 视频轨道添加失败");
            log::error!("   2. SDP协商过程中视频轨道丢失");
            log::error!("   3. 编解码器不匹配");
        }
        
        if !has_h264 {
            log::error!("❌ Answer SDP中没有H.264编解码器支持！");
        }
        
        // 设置本地描述
        self.peer_connection.set_local_description(answer.clone()).await?;
        
        // 发送Answer (包含H.264编解码器支持)
        let answer_msg = WebSocketMessage::WebRTCAnswer {
            target_id: self.client_id.clone(),
            session_description: SessionDescription {
                sdp_type: "answer".to_string(),
                sdp: answer.sdp,
            },
        };
        
        match self.signaling_tx.send(answer_msg) {
            Ok(_) => {
                log::info!("📤 WebRTC Answer发送成功 (包含视频: {}, H.264: {})", has_video, has_h264);
            }
            Err(e) => {
                log::error!("❌ Answer发送失败: {}", e);
                return Err(e.into());
            }
        }
        
        log::info!("🎯 WebRTC H.264连接协商已启动");
        Ok(())
    }
    
    /// 处理ICE候选
    pub async fn handle_ice_candidate(&self, ice_candidate: IceCandidate) -> Result<()> {
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

    
    /// 强制生成关键帧
    pub async fn force_keyframe(&self) -> Result<()> {
        if let Some(manager) = self.video_stream_manager.lock().await.as_ref() {
            manager.force_keyframe().await?;
            log::info!("🔑 已强制生成H.264关键帧");
        }
        Ok(())
    }
    
    /// 更新视频流质量
    pub async fn update_video_quality(&self, quality: NetworkQuality) -> Result<()> {
        if let Some(manager) = self.video_stream_manager.lock().await.as_mut() {
            let new_config = match quality {
                NetworkQuality::Excellent => StreamConfig::high_quality(),
                NetworkQuality::Good => StreamConfig::standard_quality(),
                NetworkQuality::Poor => StreamConfig::low_latency(),
            };
            
            manager.update_config(new_config).await?;
            log::info!("🔧 已更新H.264视频流质量: {:?}", quality);
        }
        Ok(())
    }

    /// 关闭WebRTC连接
    pub async fn close(&self) -> Result<()> {
        log::info!("🔐 关闭H.264 WebRTC连接");
        
        // 停止视频流
        if let Some(manager) = self.video_stream_manager.lock().await.as_mut() {
            manager.stop().await?;
            log::info!("🛑 H.264视频流已停止");
        }
        
        self.peer_connection.close().await?;
        log::info!("✅ WebRTC连接已关闭");
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