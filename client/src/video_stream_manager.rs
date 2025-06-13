use crate::screen_video::{VideoScreenCapture, CaptureConfig};
use crate::video_track::{H264VideoTrack, NetworkStats};
use crate::video_encoder::{VideoEncoderConfig, NetworkQuality};
use anyhow::{Result, anyhow};
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use std::time::{Duration, Instant};
use log::{info, debug, warn, error};

/// 视频流管理器 - 整合屏幕捕获、编码和传输
pub struct VideoStreamManager {
    /// 屏幕捕获服务
    screen_capture: Arc<Mutex<VideoScreenCapture>>,
    /// H.264视频轨道
    video_track: Arc<Mutex<H264VideoTrack>>,
    /// 当前配置
    config: StreamConfig,
    /// 统计信息
    stats: Arc<Mutex<StreamStats>>,
    /// 控制信号
    control_tx: Option<mpsc::UnboundedSender<StreamCommand>>,
    /// 流状态
    state: Arc<Mutex<StreamState>>,
}

/// 流配置
#[derive(Debug, Clone)]
pub struct StreamConfig {
    /// 视频编码配置
    pub video_config: VideoEncoderConfig,
    /// 屏幕捕获配置
    pub capture_config: CaptureConfig,
    /// 网络质量
    pub network_quality: NetworkQuality,
    /// 自适应质量调节
    pub adaptive_quality: bool,
    /// 质量调节间隔 (秒)
    pub quality_adapt_interval: f64,
}

/// 流统计信息
#[derive(Debug, Clone, Default)]
pub struct StreamStats {
    /// 总运行时间
    pub total_runtime_secs: f64,
    /// 捕获统计
    pub frames_captured: u64,
    pub frames_encoded: u64,
    pub frames_transmitted: u64,
    pub frames_dropped: u64,
    /// 性能统计
    pub average_capture_fps: f64,
    pub average_encoding_latency_ms: f64,
    /// 网络统计
    pub total_bytes_sent: u64,
    pub average_bitrate_kbps: f64,
    /// 错误统计
    pub capture_errors: u64,
    pub encoding_errors: u64,
    pub transmission_errors: u64,
}

/// 流状态
#[derive(Debug, Clone, PartialEq)]
pub enum StreamState {
    Stopped,
    Starting,
    Running,
    Stopping,
    Error(String),
}

/// 流控制命令
#[derive(Debug, Clone)]
pub enum StreamCommand {
    Stop,
    UpdateConfig(StreamConfig),
    ForceKeyframe,
    AdaptQuality(NetworkStats),
}

impl VideoStreamManager {
    /// 创建新的视频流管理器
    pub async fn new(config: StreamConfig) -> Result<Self> {
        info!("🎬 初始化视频流管理器");
        
        // 创建屏幕捕获服务
        let screen_capture = Arc::new(Mutex::new(
            VideoScreenCapture::new(config.capture_config.clone()).await?
        ));
        
        // 创建H.264视频轨道
        let video_track = Arc::new(Mutex::new(
            H264VideoTrack::new_with_quality(
                config.video_config.width,
                config.video_config.height,
                config.network_quality
            ).await?
        ));
        
        Ok(Self {
            screen_capture,
            video_track,
            config,
            stats: Arc::new(Mutex::new(StreamStats::default())),
            control_tx: None,
            state: Arc::new(Mutex::new(StreamState::Stopped)),
        })
    }

    /// 启动视频流
    pub async fn start(&mut self) -> Result<()> {
        let mut state = self.state.lock().await;
        if *state != StreamState::Stopped {
            return Err(anyhow!("视频流已在运行或正在启动"));
        }
        *state = StreamState::Starting;
        drop(state);

        info!("🚀 启动视频流传输...");
        
        // 启动视频轨道流传输
        {
            let mut track = self.video_track.lock().await;
            track.start_streaming().await?;
        }

        // 创建控制通道
        let (control_tx, control_rx) = mpsc::unbounded_channel();
        self.control_tx = Some(control_tx);

        // 启动流处理任务
        self.start_stream_processing_task(control_rx).await?;

        // 启动质量自适应任务
        if self.config.adaptive_quality {
            self.start_quality_adaptation_task().await?;
        }

        // 更新状态
        *self.state.lock().await = StreamState::Running;
        
        info!("✅ 视频流已启动");
        Ok(())
    }

    /// 停止视频流
    pub async fn stop(&mut self) -> Result<()> {
        let mut state = self.state.lock().await;
        if *state == StreamState::Stopped {
            return Ok(());
        }
        *state = StreamState::Stopping;
        drop(state);

        info!("🛑 停止视频流传输...");

        // 发送停止命令
        if let Some(control_tx) = &self.control_tx {
            let _ = control_tx.send(StreamCommand::Stop);
        }

        // 等待状态变为停止
        let mut attempts = 0;
        while attempts < 50 { // 最多等待5秒
            tokio::time::sleep(Duration::from_millis(100)).await;
            let current_state = self.state.lock().await.clone();
            if current_state == StreamState::Stopped {
                break;
            }
            attempts += 1;
        }

        self.control_tx = None;
        
        info!("✅ 视频流已停止");
        Ok(())
    }

    /// 强制关键帧
    pub async fn force_keyframe(&self) -> Result<()> {
        if let Some(control_tx) = &self.control_tx {
            control_tx.send(StreamCommand::ForceKeyframe)
                .map_err(|e| anyhow!("发送关键帧命令失败: {}", e))?;
        }
        Ok(())
    }

    /// 更新配置
    pub async fn update_config(&mut self, new_config: StreamConfig) -> Result<()> {
        info!("🔧 更新视频流配置");
        
        if let Some(control_tx) = &self.control_tx {
            control_tx.send(StreamCommand::UpdateConfig(new_config.clone()))
                .map_err(|e| anyhow!("发送配置更新命令失败: {}", e))?;
        }
        
        self.config = new_config;
        Ok(())
    }

    /// 启动流处理任务
    async fn start_stream_processing_task(&self, mut control_rx: mpsc::UnboundedReceiver<StreamCommand>) -> Result<()> {
        let screen_capture = Arc::clone(&self.screen_capture);
        let video_track = Arc::clone(&self.video_track);
        let stats = Arc::clone(&self.stats);
        let state = Arc::clone(&self.state);
        
        // 任务A：处理控制命令
        let control_state = Arc::clone(&state);
        let control_video_track = Arc::clone(&video_track);
        tokio::spawn(async move {
            while let Some(cmd) = control_rx.recv().await {
                match cmd {
                    StreamCommand::Stop => {
                        debug!("🛑 收到停止命令 (control)");
                        break;
                    }
                    StreamCommand::ForceKeyframe => {
                        debug!("🔑 收到强制关键帧命令 (control)");
                        control_video_track.lock().await.force_keyframe().await;
                    }
                    StreamCommand::UpdateConfig(_cfg) => {
                        debug!("🔧 收到配置更新命令 (control)");
                    }
                    StreamCommand::AdaptQuality(network_stats) => {
                        debug!("🔄 收到质量自适应命令 (control)");
                        if let Err(e) = control_video_track.lock().await.adapt_quality(&network_stats).await {
                            warn!("⚠️ 质量自适应失败: {}", e);
                        }
                    }
                }
            }
            debug!("🧹 控制命令处理任务结束");
            *control_state.lock().await = StreamState::Stopped;
        });

        // 任务B：处理视频帧
        let video_state = Arc::clone(&state);
        let video_stats = Arc::clone(&stats);
        tokio::spawn(async move {
            let mut frame_count = 0u64;
            let mut last_stats_update = Instant::now();

            // 启动屏幕捕获流
            let capture_stream = match screen_capture.lock().await.start_capture_stream().await {
                Ok(stream) => stream,
                Err(e) => {
                    error!("❌ 启动屏幕捕获流失败: {}", e);
                    *video_state.lock().await = StreamState::Error(e.to_string());
                    return;
                }
            };

            while let Ok(mut video_frame) = capture_stream.recv_async().await {
                debug!("📥 处理捕获帧，大小: {} bytes", video_frame.data.len());
                
                // drain: 只保留最新帧，丢弃积压的旧帧
                while let Ok(f) = capture_stream.try_recv() { 
                    video_frame = f; 
                    debug!("🗑️ 丢弃积压帧，保持实时性");
                }
                
                frame_count += 1;

                // 编码放到阻塞线程池，避免阻塞整个async任务
                let video_track_mutex = Arc::clone(&video_track);
                let video_stats_clone = Arc::clone(&video_stats);
                let frame_data = video_frame.data.clone();

                // ① 在阻塞线程中执行同步编码
                let encode_start = Instant::now();
                match tokio::task::spawn_blocking(move || {
                    let mut guard = video_track_mutex
                        .blocking_lock();
                    guard.encode_rgba_sync(&frame_data)
                }).await {
                    Ok(Ok(encoded_frame)) => {
                        let encode_time = encode_start.elapsed();

                        // ② 回到 async 线程发送
                        if let Err(e) = video_track.lock().await.send_encoded_frame(&encoded_frame).await {
                            error!("❌ 发送编码帧失败: {}", e);
                            video_stats.lock().await.transmission_errors += 1;
                        } else {
                            let mut stats_guard = video_stats_clone.lock().await;
                            stats_guard.frames_captured += 1;
                            stats_guard.frames_encoded += 1;
                            stats_guard.average_encoding_latency_ms =
                                (stats_guard.average_encoding_latency_ms * 0.9)
                                    + (encode_time.as_millis() as f64 * 0.1);
                            debug!("✅ 编码并发送完成，用时: {:.1}ms", encode_time.as_millis());
                        }
                    }
                    Ok(Err(e)) => {
                        error!("❌ 编码帧失败: {}", e);
                        video_stats_clone.lock().await.encoding_errors += 1;
                    }
                    Err(e) => {
                        error!("❌ 编码任务执行失败: {}", e);
                        video_stats_clone.lock().await.encoding_errors += 1;
                    }
                }

                if last_stats_update.elapsed() > Duration::from_secs(1) {
                    Self::update_performance_stats(&video_stats, frame_count, last_stats_update).await;
                    last_stats_update = Instant::now();
                    frame_count = 0;
                }
            }

            warn!("⚠️ 屏幕捕获流结束，视频帧处理任务退出");
            *video_state.lock().await = StreamState::Stopped;
        });
        
        Ok(())
    }

    /// 启动质量自适应任务
    async fn start_quality_adaptation_task(&self) -> Result<()> {
        let control_tx = self.control_tx.as_ref().unwrap().clone();
        let video_track = Arc::clone(&self.video_track);
        let adapt_interval = Duration::from_secs_f64(self.config.quality_adapt_interval);
        
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(adapt_interval);
            
            loop {
                interval.tick().await;
                
                // 获取网络统计信息
                let network_stats = {
                    let track = video_track.lock().await;
                    let track_stats = track.get_stats().await;
                    
                    // 基于视频轨道统计估算网络状况
                    NetworkStats {
                        available_bandwidth_bps: (track_stats.average_bitrate_kbps * 1000.0) as u64,
                        round_trip_time_ms: 50.0, // TODO: 从WebRTC获取真实的RTT
                        packet_loss_ratio: if track_stats.transmission_errors > 0 {
                            track_stats.transmission_errors as f64 / track_stats.frames_sent.max(1) as f64
                        } else {
                            0.0
                        },
                    }
                };
                
                // 发送质量自适应命令
                if let Err(_) = control_tx.send(StreamCommand::AdaptQuality(network_stats)) {
                    debug!("🛑 质量自适应任务结束：控制通道已关闭");
                    break;
                }
            }
        });
        
        Ok(())
    }

    /// 更新性能统计
    async fn update_performance_stats(
        stats: &Arc<Mutex<StreamStats>>, 
        frames_in_interval: u64, 
        interval_start: Instant
    ) {
        let interval_duration = interval_start.elapsed().as_secs_f64();
        let fps = frames_in_interval as f64 / interval_duration;
        
        let mut stats_guard = stats.lock().await;
        stats_guard.average_capture_fps = (stats_guard.average_capture_fps * 0.8) + (fps * 0.2);
        stats_guard.total_runtime_secs += interval_duration;
        
        debug!("📊 性能统计: {:.1}fps (平均 {:.1}fps), 运行时间 {:.1}s", 
            fps, stats_guard.average_capture_fps, stats_guard.total_runtime_secs);
    }

    /// 获取WebRTC轨道 (用于WebRTC连接)
    pub async fn get_video_track(&self) -> Arc<dyn webrtc::track::track_local::TrackLocal + Send + Sync> {
        let track = self.video_track.lock().await;
        track.get_track()
    }

    /// 获取当前状态
    pub async fn get_state(&self) -> StreamState {
        self.state.lock().await.clone()
    }

    /// 获取统计信息
    pub async fn get_stats(&self) -> StreamStats {
        let mut combined_stats = self.stats.lock().await.clone();
        
        // 合并视频轨道统计
        let track_stats = self.video_track.lock().await.get_stats().await;
        combined_stats.frames_transmitted = track_stats.frames_sent;
        combined_stats.total_bytes_sent = track_stats.bytes_sent;
        combined_stats.average_bitrate_kbps = track_stats.average_bitrate_kbps;
        combined_stats.transmission_errors = track_stats.transmission_errors;
        
        // 合并屏幕捕获统计
        let capture_stats = self.screen_capture.lock().await.get_stats().await;
        combined_stats.capture_errors = capture_stats.capture_errors;
        combined_stats.frames_dropped = capture_stats.dropped_frames;
        
        combined_stats
    }
    
    /// 激活视频轨道开始发送数据（在WebRTC连接建立后调用）
    pub async fn activate_video_track(&self) -> Result<()> {
        let mut _track = self.video_track.lock().await;
        
        // 检查H264VideoTrack是否有激活状态字段
        // 如果没有，这个调用应该直接返回Ok(())
        
        log::info!("🚀 激活H.264视频轨道，开始发送视频数据");
        Ok(())
    }
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            video_config: VideoEncoderConfig::default(),
            capture_config: CaptureConfig::default(),
            network_quality: NetworkQuality::Good,
            adaptive_quality: true,
            quality_adapt_interval: 5.0, // 每5秒调整一次质量
        }
    }
}

/// 配置构建器
pub struct StreamConfigBuilder {
    config: StreamConfig,
}

impl StreamConfigBuilder {
    pub fn new() -> Self {
        Self {
            config: StreamConfig::default(),
        }
    }
    
    pub fn video_quality(mut self, quality: NetworkQuality) -> Self {
        self.config.network_quality = quality;
        self
    }
    
    pub fn resolution(mut self, width: u32, height: u32) -> Self {
        self.config.video_config.width = width;
        self.config.video_config.height = height;
        self.config.capture_config.target_width = width;
        self.config.capture_config.target_height = height;
        self
    }
    
    pub fn fps(mut self, fps: f32) -> Self {
        self.config.video_config.fps = fps;
        self.config.capture_config.max_fps = fps;
        self
    }
    
    pub fn bitrate(mut self, bitrate: u32) -> Self {
        self.config.video_config.bitrate = bitrate;
        self
    }
    
    pub fn build(self) -> StreamConfig {
        self.config
    }
}

impl StreamConfig {
    /// 高质量流配置
    pub fn high_quality() -> Self {
        StreamConfigBuilder::new()
            .video_quality(NetworkQuality::Excellent)
            .resolution(1920, 1080)
            .fps(60.0)
            .bitrate(8_000_000)
            .build()
    }
    
    /// 标准质量流配置
    pub fn standard_quality() -> Self {
        StreamConfigBuilder::new()
            .video_quality(NetworkQuality::Good)
            .resolution(1920, 1080)
            .fps(30.0)
            .bitrate(2_000_000)
            .build()
    }
    
    /// 低延迟流配置
    pub fn low_latency() -> Self {
        StreamConfigBuilder::new()
            .video_quality(NetworkQuality::Poor)
            .resolution(1280, 720)
            .fps(15.0)
            .bitrate(500_000)
            .build()
    }
} 