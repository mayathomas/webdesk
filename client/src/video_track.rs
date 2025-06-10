use anyhow::Result;
use std::sync::Arc;
use tokio::sync::mpsc;
use webrtc::media::Sample;
use webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;

use webrtc::track::track_local::track_local_static_sample::TrackLocalStaticSample;
use webrtc::track::track_local::TrackLocal;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::video_encoder::{VideoEncoder, VideoEncoderConfig};

/// 视频轨道管理器 - 模仿Chrome Remote Desktop的媒体流传输
pub struct VideoTrack {
    track: Arc<TrackLocalStaticSample>,
    encoder: VideoEncoder,
    frame_sender: mpsc::UnboundedSender<Vec<u8>>,
    is_active: bool,
}

impl VideoTrack {
    /// 创建新的视频轨道
    pub fn new(config: VideoEncoderConfig) -> Result<Self> {
        log::info!("🎥 创建视频轨道: {:?}x{}@{}fps", 
            config.width, config.height, config.fps);
        
        // 根据编码器类型选择RTP载荷类型
        let (codec_name, payload_type) = match config.codec {
            crate::video_encoder::VideoCodec::VP8 => ("VP8", 96),
            crate::video_encoder::VideoCodec::VP9 => ("VP9", 98),
            crate::video_encoder::VideoCodec::H264 => ("H264", 102),
        };
        
        // 创建RTP编解码器能力
        let codec_capability = RTCRtpCodecCapability {
            mime_type: format!("video/{}", codec_name),
            clock_rate: 90000, // 视频标准时钟频率
            channels: 0,
            sdp_fmtp_line: "".to_string(),
            rtcp_feedback: vec![],
        };
        
        // 创建WebRTC视频轨道
        let track = Arc::new(TrackLocalStaticSample::new(
            codec_capability,
            "video".to_string(),
            "remote_desktop_video".to_string(),
        ));
        
        // 创建视频编码器
        let encoder = VideoEncoder::new(config)?;
        
        // 创建帧数据通道
        let (frame_sender, mut frame_receiver) = mpsc::unbounded_channel::<Vec<u8>>();
        
        // 启动帧处理任务
        let track_clone = track.clone();
        tokio::spawn(async move {
            while let Some(frame_data) = frame_receiver.recv().await {
                if let Err(e) = Self::send_frame_to_track(&track_clone, frame_data).await {
                    log::error!("❌ 发送视频帧失败: {}", e);
                }
            }
        });
        
        Ok(Self {
            track,
            encoder,
            frame_sender,
            is_active: false,
        })
    }
    
    /// 获取WebRTC轨道实例
    pub fn get_track(&self) -> Arc<TrackLocalStaticSample> {
        self.track.clone()
    }
    
    /// 编码并发送RGBA帧数据
    pub async fn send_rgba_frame(&mut self, rgba_data: &[u8]) -> Result<()> {
        if !self.is_active {
            log::debug!("⏸️ 视频轨道未激活，跳过帧");
            return Ok(());
        }
        
        // 获取当前时间戳
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_millis() as u64;
        
        // 编码帧
        let encoded_frame = self.encoder.encode_frame(rgba_data, timestamp)?;
        
        // 发送到轨道
        if let Err(e) = self.frame_sender.send(encoded_frame.data) {
            log::error!("❌ 发送帧数据到通道失败: {}", e);
        }
        
        Ok(())
    }
    
    /// 激活视频轨道
    pub fn activate(&mut self) {
        log::info!("▶️ 激活视频轨道");
        self.is_active = true;
    }
    
    /// 停用视频轨道
    pub fn deactivate(&mut self) {
        log::info!("⏸️ 停用视频轨道");
        self.is_active = false;
    }
    
    /// 更新编码器配置
    pub fn update_config(&mut self, config: VideoEncoderConfig) -> Result<()> {
        self.encoder.update_config(config)
    }
    
    /// 发送编码帧到WebRTC轨道
    async fn send_frame_to_track(
        track: &Arc<TrackLocalStaticSample>,
        frame_data: Vec<u8>,
    ) -> Result<()> {
        // 创建RTP样本
        let sample = Sample {
            data: frame_data.into(),
            timestamp: SystemTime::now(),
            ..Default::default()
        };
        
        // 发送到WebRTC轨道
        track.write_sample(&sample).await?;
        
        Ok(())
    }
    
    /// 获取轨道统计信息
    pub async fn get_stats(&self) -> VideoTrackStats {
        VideoTrackStats {
            is_active: self.is_active,
            track_id: self.track.id().to_string(),
            codec: format!("{:?}", self.encoder.get_config().codec),
            resolution: format!("{}x{}", 
                self.encoder.get_config().width,
                self.encoder.get_config().height),
            fps: self.encoder.get_config().fps,
            bitrate: self.encoder.get_config().bitrate,
        }
    }
}

/// 视频轨道统计信息
#[derive(Debug, Clone)]
pub struct VideoTrackStats {
    pub is_active: bool,
    pub track_id: String,
    pub codec: String,
    pub resolution: String,
    pub fps: u32,
    pub bitrate: u32,
}

/// 视频轨道管理器
pub struct VideoTrackManager {
    video_track: Option<VideoTrack>,
}

impl VideoTrackManager {
    /// 创建新的视频轨道管理器
    pub fn new() -> Self {
        Self {
            video_track: None,
        }
    }
    
    /// 初始化视频轨道
    pub fn initialize(&mut self, config: VideoEncoderConfig) -> Result<Arc<TrackLocalStaticSample>> {
        log::info!("🎬 初始化视频轨道管理器");
        
        let track = VideoTrack::new(config)?;
        let webrtc_track = track.get_track();
        
        self.video_track = Some(track);
        
        Ok(webrtc_track)
    }
    
    /// 发送屏幕帧
    pub async fn send_screen_frame(&mut self, rgba_data: &[u8]) -> Result<()> {
        if let Some(ref mut track) = self.video_track {
            track.send_rgba_frame(rgba_data).await?;
        }
        Ok(())
    }
    
    /// 激活所有轨道
    pub fn activate(&mut self) {
        if let Some(ref mut track) = self.video_track {
            track.activate();
        }
    }
    
    /// 停用所有轨道
    pub fn deactivate(&mut self) {
        if let Some(ref mut track) = self.video_track {
            track.deactivate();
        }
    }
    
    /// 获取统计信息
    pub async fn get_stats(&self) -> Option<VideoTrackStats> {
        if let Some(ref track) = self.video_track {
            Some(track.get_stats().await)
        } else {
            None
        }
    }
}