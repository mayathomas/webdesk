use anyhow::Result;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use webrtc::media::Sample;
use webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;
use webrtc::track::track_local::track_local_static_sample::TrackLocalStaticSample;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::video_encoder::VideoEncoderConfig;
use crate::video_encoder::{H264VideoEncoder, VideoEncoderFactory, NetworkQuality, EncodedFrame};
use std::time::{Duration, Instant};


/// 视频轨道统计信息
#[derive(Debug, Clone, Default)]
pub struct VideoTrackStats {
    // 添加详细统计字段
    pub frames_encoded: u64,
    pub frames_sent: u64,
    pub bytes_sent: u64,
    pub keyframes_sent: u64,
    pub transmission_errors: u64,
    pub average_encode_time_ms: f64,
    pub average_bitrate_kbps: f64,
    pub last_keyframe_time: Option<Instant>,
}


/// H.264视频轨道 - 管理视频编码和WebRTC传输
pub struct H264VideoTrack {
    /// WebRTC视频轨道
    track: Arc<TrackLocalStaticSample>,
    /// H.264编码器
    encoder: Arc<Mutex<H264VideoEncoder>>,
    /// 传输统计
    stats: Arc<Mutex<VideoTrackStats>>,
    /// 配置信息
    config: VideoEncoderConfig,
    /// 帧发送队列
    frame_sender: Option<mpsc::UnboundedSender<EncodedFrame>>,
}

impl H264VideoTrack {
    /// 使用预设配置创建视频轨道
    pub async fn new_with_quality(
        width: u32, 
        height: u32, 
        network_quality: NetworkQuality
    ) -> Result<Self> {
        let encoder = VideoEncoderFactory::create_adaptive(width, height, network_quality)?;
        let config = encoder.get_config().clone();
        
        // 使用相同的H.264配置
        let track = Arc::new(TrackLocalStaticSample::new(
            RTCRtpCodecCapability {
                mime_type: "video/H264".to_owned(),
                clock_rate: 90000,
                channels: 0,
                sdp_fmtp_line: "profile-level-id=42e01e;packetization-mode=1;level-asymmetry-allowed=1".to_owned(),
                rtcp_feedback: vec![
                    webrtc::rtp_transceiver::RTCPFeedback {
                        typ: "nack".to_owned(),
                        parameter: "".to_owned(),
                    },
                    webrtc::rtp_transceiver::RTCPFeedback {
                        typ: "nack".to_owned(),
                        parameter: "pli".to_owned(),
                    },
                    webrtc::rtp_transceiver::RTCPFeedback {
                        typ: "ccm".to_owned(),
                        parameter: "fir".to_owned(),
                    },
                ],
            },
            "video".to_owned(),
            "h264_remote_desktop".to_owned(),
        ));

        log::info!("✅ H.264视频轨道创建成功 (质量: {:?})", network_quality);

        Ok(Self {
            track,
            encoder: Arc::new(Mutex::new(encoder)),
            config,
            frame_sender: None,
            stats: Arc::new(Mutex::new(VideoTrackStats::default())),
        })
    }

    /// 启动视频流传输
    pub async fn start_streaming(&mut self) -> Result<()> {
        let (tx, mut rx) = mpsc::unbounded_channel::<EncodedFrame>();
        self.frame_sender = Some(tx);

        let track = Arc::clone(&self.track);
        let stats = Arc::clone(&self.stats);
        
        // 启动帧发送任务
        tokio::spawn(async move {
            let mut sequence_number = 0u16;
            
            while let Some(frame) = rx.recv().await {
                match Self::send_h264_frame(&track, &frame, &mut sequence_number).await {
                    Ok(bytes_sent) => {
                        let mut stats_guard = stats.lock().await;
                        stats_guard.frames_sent += 1;
                        stats_guard.bytes_sent += bytes_sent as u64;
                        
                        if frame.is_keyframe {
                            stats_guard.keyframes_sent += 1;
                            stats_guard.last_keyframe_time = Some(Instant::now());
                        }
                        
                        // 计算平均比特率
                        let duration_secs = Instant::now().duration_since(
                            stats_guard.last_keyframe_time.unwrap_or_else(Instant::now)
                        ).as_secs_f64();
                        
                        if duration_secs > 0.0 {
                            stats_guard.average_bitrate_kbps = 
                                (stats_guard.bytes_sent as f64 * 8.0) / (duration_secs * 1000.0);
                        }
                    }
                    Err(e) => {
                        log::error!("❌ 发送H.264帧失败: {}", e);
                        let mut stats_guard = stats.lock().await;
                        stats_guard.transmission_errors += 1;
                    }
                }
            }
        });

        log::info!("✅ H.264视频流传输已启动");
        Ok(())
    }

    /// 编码并发送视频帧
    pub async fn encode_and_send_frame(&self, rgba_data: &[u8]) -> Result<()> {
        let start_time = Instant::now();
        
        // 检查输入数据
        if rgba_data.is_empty() {
            log::warn!("⚠️ 接收到空的RGBA数据，跳过编码");
            return Ok(());
        }
        
        log::debug!("🎬 接收到RGBA数据: {}字节 ({}x{})", 
            rgba_data.len(), self.config.width, self.config.height);
        
        // 验证数据长度是否匹配期望的分辨率
        let expected_size = (self.config.width * self.config.height * 4) as usize;
        if rgba_data.len() != expected_size {
            log::warn!("⚠️ RGBA数据长度不匹配: 期望{}字节, 实际{}字节", 
                expected_size, rgba_data.len());
        }
        
        // 获取时间戳
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_millis() as u64;

        // 编码帧
        let frame = {
            let mut encoder = self.encoder.lock().await;
            encoder.encode_frame(rgba_data, timestamp).await?
        };

        let encode_time = start_time.elapsed();
        
        log::debug!("🎬 编码完成: {}字节输出, 用时{:.1}ms", 
            frame.data.len(), encode_time.as_millis());

        // 更新编码统计
        {
            let mut stats = self.stats.lock().await;
            stats.frames_encoded += 1;
            stats.average_encode_time_ms = 
                (stats.average_encode_time_ms * (stats.frames_encoded - 1) as f64 + 
                 encode_time.as_millis() as f64) / stats.frames_encoded as f64;
        }

        // 立即发送而不是通过队列缓存（降低延迟）
        if !frame.data.is_empty() {
            let mut sequence_number = 0u16; // 临时序列号
            match Self::send_h264_frame(&self.track, &frame, &mut sequence_number).await {
                Ok(bytes_sent) => {
                    let mut stats = self.stats.lock().await;
                    stats.frames_sent += 1;
                    stats.bytes_sent += bytes_sent as u64;
                    
                    if frame.is_keyframe {
                        stats.keyframes_sent += 1;
                        stats.last_keyframe_time = Some(Instant::now());
                    }
                    
                    log::debug!("✅ 立即发送成功: {}字节", bytes_sent);
                }
                Err(e) => {
                    log::error!("❌ 立即发送失败: {}", e);
                    let mut stats = self.stats.lock().await;
                    stats.transmission_errors += 1;
                }
            }
        } else {
            log::debug!("⚠️ 跳过空帧发送");
        }

        Ok(())
    }

    /// 发送H.264帧到WebRTC（使用TrackLocalStaticSample）
    async fn send_h264_frame(
        track: &Arc<TrackLocalStaticSample>,
        frame: &EncodedFrame,
        _sequence_number: &mut u16
    ) -> Result<usize> {
        // 记录所有帧发送尝试（包括空帧，用于调试）
        if frame.data.is_empty() {
            log::debug!("📤 尝试发送空的H.264帧 - 跳过");
            return Ok(0);
        }

        log::debug!("📤 发送H.264帧: {}字节, {}帧", frame.data.len(), 
            if frame.is_keyframe { "I" } else { "P" });
        
        // 计算帧持续时间 - 假设30fps，每帧33.33ms
        let frame_duration = Duration::from_millis(33); // 30fps = 1000ms/30 ≈ 33.33ms
        
        // 使用RTP时间戳而不是SystemTime
        // H.264使用90kHz时钟频率，所以每帧增加90000/30 = 3000
        static mut RTP_TIMESTAMP: u32 = 0;
        let _rtp_timestamp = unsafe {
            RTP_TIMESTAMP = RTP_TIMESTAMP.wrapping_add(3000);
            RTP_TIMESTAMP
        };
        
        // 创建媒体样本 - 使用正确的时间戳格式
        let sample = Sample {
            data: frame.data.clone().into(),
            timestamp: SystemTime::now(),
            duration: frame_duration,
            ..Default::default()
        };
        
        // 发送到WebRTC轨道
        match track.write_sample(&sample).await {
            Ok(_) => {
                log::debug!("✅ H.264帧发送成功: {}字节", frame.data.len());
                Ok(frame.data.len())
            }
            Err(e) => {
                log::error!("❌ H.264帧发送失败: {}", e);
                Err(e.into())
            }
        }
    }

    /// 获取WebRTC轨道
    pub fn get_track(&self) -> Arc<TrackLocalStaticSample> {
        Arc::clone(&self.track)
    }

    /// 更新编码器配置
    pub async fn update_config(&mut self, new_config: VideoEncoderConfig) -> Result<()> {
        let mut encoder = self.encoder.lock().await;
        encoder.update_config(new_config.clone()).await?;
        self.config = new_config;
        log::info!("🔧 H.264视频轨道配置已更新");
        Ok(())
    }

    /// 强制生成关键帧
    pub async fn force_keyframe(&self) {
        // 注意：openh264 0.8.1的编码器没有直接的force_keyframe方法
        // 关键帧的生成现在由编码器内部的时间逻辑控制
        log::debug!("🔑 请求强制关键帧（由编码器内部时间逻辑控制）");
    }

    /// 获取统计信息
    pub async fn get_stats(&self) -> VideoTrackStats {
        self.stats.lock().await.clone()
    }

    /// 自适应调整编码质量
    pub async fn adapt_quality(&mut self, network_stats: &NetworkStats) -> Result<()> {
        let current_quality = self.determine_quality(network_stats);
        
        match current_quality {
            NetworkQuality::Poor => {
                // 降低质量：减少帧率和比特率
                let new_config = VideoEncoderConfig {
                    width: self.config.width,
                    height: self.config.height,
                    fps: (self.config.fps * 0.5).max(10.0),
                    bitrate: (self.config.bitrate / 2).max(200_000),
                    codec: self.config.codec.clone(),
                    quality: 60,
                };
                self.update_config(new_config).await?;
            }
            NetworkQuality::Good => {
                // 标准质量
                let new_config = VideoEncoderConfig {
                    width: self.config.width,
                    height: self.config.height,
                    fps: 30.0,
                    bitrate: 2_000_000,
                    codec: self.config.codec.clone(),
                    quality: 75,
                };
                self.update_config(new_config).await?;
            }
            NetworkQuality::Excellent => {
                // 提高质量：增加帧率和比特率
                let new_config = VideoEncoderConfig {
                    width: self.config.width,
                    height: self.config.height,
                    fps: 60.0,
                    bitrate: 5_000_000,
                    codec: self.config.codec.clone(),
                    quality: 90,
                };
                self.update_config(new_config).await?;
            }
        }

        Ok(())
    }

    /// 根据网络状况确定质量等级
    fn determine_quality(&self, stats: &NetworkStats) -> NetworkQuality {
        // 综合考虑带宽、延迟和丢包率
        let bandwidth_mbps = stats.available_bandwidth_bps as f64 / 1_000_000.0;
        let rtt_ms = stats.round_trip_time_ms;
        let packet_loss = stats.packet_loss_ratio;

        if bandwidth_mbps > 10.0 && rtt_ms < 50.0 && packet_loss < 0.01 {
            NetworkQuality::Excellent
        } else if bandwidth_mbps > 2.0 && rtt_ms < 200.0 && packet_loss < 0.05 {
            NetworkQuality::Good
        } else {
            NetworkQuality::Poor
        }
    }
}

/// 网络统计信息
#[derive(Debug, Clone)]
pub struct NetworkStats {
    pub available_bandwidth_bps: u64,
    pub round_trip_time_ms: f64,
    pub packet_loss_ratio: f64,
}

impl Default for NetworkStats {
    fn default() -> Self {
        Self {
            available_bandwidth_bps: 5_000_000, // 5Mbps
            round_trip_time_ms: 50.0,
            packet_loss_ratio: 0.0,
        }
    }
}