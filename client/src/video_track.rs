use anyhow::Result;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::rtp::packetizer::{new_packetizer, Packetizer};
use webrtc::rtp::codecs::h264::H264Payloader;
use webrtc::rtp::packet::Packet as RtpPacket;
use webrtc::rtp::sequence::new_random_sequencer;
use webrtc::track::track_local::TrackLocalWriter;
use bytes::Bytes;

use crate::video_encoder::VideoEncoderConfig;
use crate::video_encoder::{EncodedFrame, H264VideoEncoder, NetworkQuality, VideoEncoderFactory};
use std::time::Instant;

/// 视频轨道统计信息
#[derive(Debug, Clone, Default)]
pub struct VideoTrackStats {
    // 添加详细统计字段
    pub frames_sent: u64,
    pub bytes_sent: u64,
    pub keyframes_sent: u64,
    pub transmission_errors: u64,
    pub average_bitrate_kbps: f64,
    pub last_keyframe_time: Option<Instant>,
}

/// H.264视频轨道 - 管理视频编码和WebRTC传输
pub struct H264VideoTrack {
    /// WebRTC视频轨道
    track: Arc<TrackLocalStaticRTP>,
    /// H.264编码器
    encoder: Arc<Mutex<H264VideoEncoder>>,
    /// 传输统计
    stats: Arc<Mutex<VideoTrackStats>>,
    /// 配置信息
    config: Arc<Mutex<VideoEncoderConfig>>,
    /// 帧发送队列
    frame_sender: Option<mpsc::UnboundedSender<EncodedFrame>>,
    /// RTP Packetizer（保持全局递增序列号）
    packetizer: Arc<Mutex<Box<dyn Packetizer + Send + Sync>>>,
}

impl H264VideoTrack {
    /// 使用预设配置创建视频轨道
    pub async fn new_with_quality(
        width: u32, 
        height: u32, 
        network_quality: NetworkQuality,
    ) -> Result<Self> {
        let encoder = VideoEncoderFactory::create_adaptive(width, height, network_quality)?;
        let config = encoder.get_config().clone();
        
        // 使用相同的H.264配置
        let track = Arc::new(TrackLocalStaticRTP::new(
            RTCRtpCodecCapability {
                mime_type: "video/H264".to_owned(),
                clock_rate: 90000,
                channels: 0,
                sdp_fmtp_line:
                    "profile-level-id=42e01e;packetization-mode=1;level-asymmetry-allowed=1"
                        .to_owned(),
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

        // ---------- 初始化全局 Packetizer ----------
        let mtu = 1200; // 典型 MTU
        let payload_type = 102; // 需与 SDP 保持一致
        let ssrc = rand::random::<u32>();
        let payloader = Box::new(H264Payloader::default());
        let sequencer = Box::new(new_random_sequencer());

        let packetizer_raw = new_packetizer(
            mtu,
            payload_type,
            ssrc,
            payloader,
            sequencer,
            90_000, // clock rate
        );
        let packetizer: Box<dyn Packetizer + Send + Sync> = Box::new(packetizer_raw);

        let packetizer = Arc::new(Mutex::new(packetizer));

        Ok(Self {
            track,
            encoder: Arc::new(Mutex::new(encoder)),
            config: Arc::new(Mutex::new(config)),
            frame_sender: None,
            stats: Arc::new(Mutex::new(VideoTrackStats::default())),
            packetizer,
        })
    }

    /// 启动视频流传输
    pub async fn start_streaming(&mut self) -> Result<()> {
        let (tx, mut rx) = mpsc::unbounded_channel::<EncodedFrame>();
        self.frame_sender = Some(tx);

        let track = Arc::clone(&self.track);
        let stats = Arc::clone(&self.stats);
        let packetizer_ref = Arc::clone(&self.packetizer);
        
        // 启动帧发送任务
        tokio::spawn(async move {
            while let Some(frame) = rx.recv().await {
                match Self::send_h264_frame(&track, &packetizer_ref, &frame).await {
                    Ok(bytes_sent) => {
                        let mut stats_guard = stats.lock().await;
                        stats_guard.frames_sent += 1;
                        stats_guard.bytes_sent += bytes_sent as u64;
                        
                        if frame.is_keyframe {
                            stats_guard.keyframes_sent += 1;
                            stats_guard.last_keyframe_time = Some(Instant::now());
                        }
                        
                        // 计算平均比特率
                        let duration_secs = Instant::now()
                            .duration_since(
                                stats_guard.last_keyframe_time.unwrap_or_else(Instant::now),
                            )
                            .as_secs_f64();
                        
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

    /// 发送H.264帧到WebRTC（使用TrackLocalStaticSample）
    async fn send_h264_frame(
        track: &Arc<TrackLocalStaticRTP>,
        packetizer_arc: &Arc<Mutex<Box<dyn Packetizer + Send + Sync>>>,
        frame: &EncodedFrame,
    ) -> Result<usize> {
        // 记录所有帧发送尝试（包括空帧，用于调试）
        if frame.data.is_empty() {
            log::debug!("📤 尝试发送空的H.264帧 - 跳过");
            return Ok(0);
        }

        log::debug!(
            "📤 发送H.264帧: {}字节, {}帧",
            frame.data.len(),
            if frame.is_keyframe { "I" } else { "P" }
        );
        
        // 使用RTP时间戳而不是SystemTime
        // H.264使用90kHz时钟频率，所以每帧增加90000/30 = 3000
        static mut RTP_TIMESTAMP: u32 = 0;
        let _rtp_timestamp = unsafe {
            RTP_TIMESTAMP = RTP_TIMESTAMP.wrapping_add(3000);
            RTP_TIMESTAMP
        };
        
        // 使用持久化 Packetizer，保证全局递增序列号
        let mut packetizer_guard = packetizer_arc.lock().await;

        // 将 Annex-B 数据切分为单个 NALU，不含起始码
        let nalus: Vec<&[u8]> = {
            let mut nalus = Vec::new();
            let mut start = 0usize;
            let data = frame.data.as_slice();
            let len = data.len();
            // 简单查找 0x000001 / 0x00000001
            let mut i = 0usize;
            while i + 3 < len {
                if data[i] == 0 && data[i+1] == 0 && ((data[i+2] == 1) || (data[i+2]==0 && i+4<len && data[i+3]==1)) {
                    if i > start {
                        nalus.push(&data[start..i]);
                    }
                    // 跳过起始码
                    if data[i+2]==1 {
                        start = i + 3;
                        i += 3;
                    } else {
                        start = i + 4;
                        i += 4;
                    }
                    continue;
                }
                i += 1;
            }
            if start < len {
                nalus.push(&data[start..]);
            }
            nalus
        };

        let mut pkts: Vec<RtpPacket> = Vec::new();
        for (idx, nalu) in nalus.iter().enumerate() {
            let is_last_nalu = idx == nalus.len() - 1;
            let n_bytes = Bytes::copy_from_slice(nalu);
            let samples_inc = if idx == 0 { 3000 } else { 0 };
            let mut pks = packetizer_guard.packetize(&n_bytes, samples_inc)?;
            if is_last_nalu {
                if let Some(last) = pks.last_mut() {
                    last.header.marker = true;
                }
            }
            pkts.extend(pks);
        }

        let mut total_bytes = 0usize;
        for mut pkt in pkts {
            total_bytes += pkt.payload.len() + 12; // 12字节RTP头
            // write_rtp 需要 &mut Packet
            if let Err(e) = track.write_rtp(&mut pkt).await {
                return Err(e.into());
            }
        }

        Ok(total_bytes)
    }

    /// 获取WebRTC轨道
    pub fn get_track(&self) -> Arc<TrackLocalStaticRTP> {
        Arc::clone(&self.track)
    }

    /// 更新编码器配置
    pub async fn update_config(&mut self, new_config: VideoEncoderConfig) -> Result<()> {
        let mut encoder = self.encoder.lock().await;
        encoder.update_config(new_config.clone()).await?;
        let mut config = self.config.lock().await;
        *config = new_config;
        log::info!("🔧 H.264视频轨道配置已更新");
        Ok(())
    }

    /// 强制生成关键帧
    pub async fn force_keyframe(&self) {
        let mut enc = self.encoder.lock().await;
        enc.request_keyframe();
        log::info!("🔑 已标记下一帧为关键帧");
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
                let (w, h) = (1920, 1080);  
                let cfg = self.config.lock().await.clone();
                let new_config = VideoEncoderConfig {
                    width: w,
                    height: h,
                    fps: (cfg.fps * 0.5).max(10.0),
                    bitrate: (cfg.bitrate / 2).max(200_000),
                    codec: cfg.codec.clone(),
                    quality: 60,
                };
                self.update_config(new_config).await?;
            }
            NetworkQuality::Good => {
                let (w, h) = (1920, 1080);  
                let cfg = self.config.lock().await.clone();
                let new_config = VideoEncoderConfig {
                    width: w,
                    height: h,
                    fps: 30.0,
                    bitrate: 2_000_000,
                    codec: cfg.codec.clone(),
                    quality: 75,
                };
                self.update_config(new_config).await?;
            }
            NetworkQuality::Excellent => {
                let (w, h) = (1920, 1080);  
                let cfg = self.config.lock().await.clone();
                let new_config = VideoEncoderConfig {
                    width: w,
                    height: h,
                    fps: 60.0,
                    bitrate: 5_000_000,
                    codec: cfg.codec.clone(),
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

    /// 同步编码RGBA数据，不执行发送（可在spawn_blocking中调用）
    pub fn encode_rgba_sync(&mut self, rgba_data: &[u8]) -> Result<crate::video_encoder::EncodedFrame> {
        let start_time = std::time::Instant::now();

        // 阻塞调用内部异步编码器
        let encoded = futures::executor::block_on(async {
            let mut enc = self.encoder.lock().await;
            enc.encode_frame(rgba_data, 0).await
        })?;

        log::debug!(
            "🎬 [sync] 编码完成: {}字节, 用时{:.1}ms",
            encoded.data.len(),
            start_time.elapsed().as_millis()
        );

        Ok(encoded)
    }

    /// 发送已编码帧到WebRTC轨道（异步）
    pub async fn send_encoded_frame(&self, frame: &crate::video_encoder::EncodedFrame) -> Result<()> {
        let bytes_sent = Self::send_h264_frame(&self.track, &self.packetizer, frame).await?;

        let mut stats = self.stats.lock().await;
        stats.frames_sent += 1;
        stats.bytes_sent += bytes_sent as u64;
        if frame.is_keyframe {
            stats.keyframes_sent += 1;
            stats.last_keyframe_time = Some(std::time::Instant::now());
        }
        Ok(())
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
