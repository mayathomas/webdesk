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
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

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
    encoder: Arc<Mutex<H264VideoEncoder>>,            // 当前使用的编码器
    pending_encoder: Arc<Mutex<Option<H264VideoEncoder>>>, // 正在预热的新编码器
    /// 传输统计
    stats: Arc<Mutex<VideoTrackStats>>,
    /// 配置信息
    config: Arc<Mutex<VideoEncoderConfig>>,
    /// 帧发送队列
    frame_sender: Option<mpsc::UnboundedSender<EncodedFrame>>,
    /// RTP Packetizer（保持全局递增序列号）
    packetizer: Arc<Mutex<Box<dyn Packetizer + Send + Sync>>>,
    /// 缓存最近的 SPS / PPS NALU（不含起始码）
    last_sps: Arc<Mutex<Option<Vec<u8>>>>,
    last_pps: Arc<Mutex<Option<Vec<u8>>>>,
    /// 是否已经发送过首帧黑屏（全局一次）
    bootstrap_sent: Arc<AtomicBool>,
    /// 上次真正应用配置的时间，用于 Cool-down
    last_cfg_change: Arc<Mutex<Instant>>,
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
            pending_encoder: Arc::new(Mutex::new(None)),
            config: Arc::new(Mutex::new(config)),
            frame_sender: None,
            stats: Arc::new(Mutex::new(VideoTrackStats::default())),
            packetizer,
            last_sps: Arc::new(Mutex::new(None)),
            last_pps: Arc::new(Mutex::new(None)),
            bootstrap_sent: Arc::new(AtomicBool::new(false)),
            last_cfg_change: Arc::new(Mutex::new(Instant::now())),
        })
    }

    /// 启动视频流传输
    pub async fn start_streaming(&mut self) -> Result<()> {
        let (tx, mut rx) = mpsc::unbounded_channel::<EncodedFrame>();
        self.frame_sender = Some(tx);

        let track = Arc::clone(&self.track);
        let stats = Arc::clone(&self.stats);
        let packetizer_ref = Arc::clone(&self.packetizer);
        let sps_ref = Arc::clone(&self.last_sps);
        let pps_ref = Arc::clone(&self.last_pps);
        
        // 启动帧发送任务
        tokio::spawn(async move {
            while let Some(frame) = rx.recv().await {
                match Self::send_h264_frame(&track, &packetizer_ref, &sps_ref, &pps_ref, &frame).await {
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

        // --------------------------------------------------------------------
        // 首帧保障：如果捕获链路过慢，浏览器在若干秒内得不到任何RTP
        // 包就会判定连接失败。这里额外启动一个后台任务，主动构造一帧
        // 全黑 RGBA 图像并编码为 IDR，再通过同一个 frame_sender 发送。
        // 只尝试一次；若发送成功即可立即"开画"，极大缩短首帧等待。
        // --------------------------------------------------------------------
        if let Some(bootstrap_tx) = self.frame_sender.as_ref().cloned() {
            let encoder_ref = Arc::clone(&self.encoder);
            let stats_ref = Arc::clone(&self.stats);
            let flag = Arc::clone(&self.bootstrap_sent);

            tokio::spawn(async move {
                use std::time::Duration;
                tokio::time::sleep(Duration::from_millis(500)).await;

                // 若已经发过黑帧，则直接退出
                if flag.load(Ordering::SeqCst) {
                    return;
                }

                // 再检查一次真实帧数量，确保捕获链路确实还未送帧
                let sent_frames = {
                    let st = stats_ref.lock().await;
                    st.frames_sent
                };
                if sent_frames > 0 {
                    return; // 已有画面，不插黑帧
                }

                // 尝试原子置位，防止并发重复
                if flag.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
                    return;
                }

                // 构造黑色 RGBA
                let cfg = {
                    let enc = encoder_ref.lock().await;
                    enc.get_config().clone()
                };
                let blank_rgba = vec![0u8; (cfg.width * cfg.height * 4) as usize];

                // 编码黑帧并标记为关键帧
                let maybe_frame = {
                    let mut enc = encoder_ref.lock().await;
                    enc.request_keyframe();
                    enc.encode_frame(&blank_rgba, 0).await
                };

                match maybe_frame {
                    Ok(frame) => {
                        if bootstrap_tx.send(frame).is_ok() {
                            log::info!("🚀 已主动发送首帧黑屏IDR以加速开画");
                        }
                    }
                    Err(e) => {
                        log::warn!("⚠️ 主动黑帧编码失败: {}", e);
                    }
                }
            });
        }

        // ----------------------------------------------
        // 关键帧预热：连接建立后的 1 秒内，连续请求 3 次 IDR，
        // 以确保浏览器端尽早获得可解码的关键帧。
        // ----------------------------------------------
        {
            let encoder = Arc::clone(&self.encoder);
            tokio::spawn(async move {
                for _i in 0..2 {
                    {
                        let mut enc = encoder.lock().await;
                        enc.request_keyframe();
                    }
                    // 两次足够触发 IDR；减少过量关键帧导致的闪黑
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
            });
        }

        // ----------------------------------------------
        // 周期性检查：若 2 秒内未发送关键帧，则请求关键帧。
        // 解决再次连接后浏览器等待 IDR 的问题。
        // ----------------------------------------------
        {
            let encoder = Arc::clone(&self.encoder);
            let stats_ref = Arc::clone(&self.stats);
            tokio::spawn(async move {
                use std::time::Duration;
                loop {
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    let need_kf = {
                        let stats = stats_ref.lock().await;
                        match stats.last_keyframe_time {
                            Some(ts) => ts.elapsed() > Duration::from_secs(3),
                            None => true,
                        }
                    };
                    if need_kf {
                        let mut enc = encoder.lock().await;
                        // 连续申请两次，间隔 50ms，提高命中率
                        enc.request_keyframe();
                        drop(enc);
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        let mut enc2 = encoder.lock().await;
                        enc2.request_keyframe();
                    }
                }
            });
        }

        // --------------------------------------------------------------------
        // 继续保活：如果 2 秒内依然没有发送足够帧 (frames_sent < 5)，
        // 每隔 33ms 发送一帧黑屏直到捕获链路接管或达到 30 帧上限。
        // --------------------------------------------------------------------
        {
            let stats_ref = Arc::clone(&self.stats);
            let encoder_ref = Arc::clone(&self.encoder);
            let sender_opt = self.frame_sender.as_ref().cloned();

            if let Some(fallback_tx) = sender_opt {
                tokio::spawn(async move {
                    use std::time::Duration;
                    tokio::time::sleep(Duration::from_secs(2)).await;

                    let mut sent = {
                        let st = stats_ref.lock().await;
                        st.frames_sent
                    };

                    if sent >= 5 {
                        return; // 捕获已正常工作，不需要填充
                    }

                    log::warn!("⚠️ 捕获链路仍未产出帧，开始发送保活黑帧...");

                    for _ in 0..30 { // 最多 1 秒
                        sent = {
                            let st = stats_ref.lock().await;
                            st.frames_sent
                        };
                        if sent >= 5 {
                            log::info!("✅ 捕获链路恢复，停止黑帧保活");
                            break;
                        }

                        // 构造黑色 RGBA
                        let cfg = {
                            let enc = encoder_ref.lock().await;
                            enc.get_config().clone()
                        };
                        let blank_data = vec![0u8; (cfg.width * cfg.height * 4) as usize];

                        // 编码并发送
                        if let Ok(frame) = {
                            let mut enc = encoder_ref.lock().await;
                            enc.request_keyframe();
                            enc.encode_frame(&blank_data, 0).await
                        } {
                            let _ = fallback_tx.send(frame);
                        }

                        tokio::time::sleep(Duration::from_millis(33)).await;
                    }
                });
            }
        }

        log::info!("✅ H.264视频流传输已启动");
        Ok(())
    }

    /// 发送H.264帧到WebRTC（使用TrackLocalStaticSample）
    async fn send_h264_frame(
        track: &Arc<TrackLocalStaticRTP>,
        packetizer_arc: &Arc<Mutex<Box<dyn Packetizer + Send + Sync>>>,
        sps_cache: &Arc<Mutex<Option<Vec<u8>>>>,
        pps_cache: &Arc<Mutex<Option<Vec<u8>>>>,
        frame: &EncodedFrame,
    ) -> Result<usize> {
        if frame.is_keyframe {
            log::info!("→ RTP 关键帧：{} 字节", frame.data.len());
        }
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

        // ----------------- RTP 时间戳增量 -----------------
        use std::time::Instant;
        static mut LAST_INSTANT: Option<Instant> = None;
        let now_i = Instant::now();
        let samples_inc_frame: u32 = unsafe {
            match LAST_INSTANT {
                Some(prev) => {
                    let delta_us = now_i.duration_since(prev).as_micros() as u32;
                    // 90 kHz * delta(ms)
                    ((90 * delta_us) / 1000).max(1)
                }
                None => 3000, // 默认 30fps
            }
        };
        unsafe { LAST_INSTANT = Some(now_i); }

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

        // -----------------------------------------
        // 如果是关键帧：
        // 1. 先扫描 nalus，提取本帧内的 SPS / PPS，更新缓存；
        // 2. 再使用 "最新" SPS / PPS 组装 STAP-A 并在发送
        //    实际帧数据前写入，避免编码器切换后发送了旧参数。
        // -----------------------------------------

        let mut cur_sps: Option<Vec<u8>> = None;
        let mut cur_pps: Option<Vec<u8>> = None;

        if frame.is_keyframe {
            for nalu in &nalus {
                if nalu.is_empty() { continue; }
                let nal_type = nalu[0] & 0x1F;
                match nal_type {
                    7 => cur_sps = Some(nalu.to_vec()),
                    8 => cur_pps = Some(nalu.to_vec()),
                    _ => {}
                }
            }

            // 更新全局缓存（用于后续非关键帧）
            if let Some(ref sps) = cur_sps {
                *sps_cache.lock().await = Some(sps.clone());
            }
            if let Some(ref pps) = cur_pps {
                *pps_cache.lock().await = Some(pps.clone());
            }

            // 组装并发送 STAP-A（确保 SPS/PPS 先于 IDR 到达浏览器）
            if let (Some(sps), Some(pps)) = (cur_sps.clone(), cur_pps.clone()) {
                let mut stap = Vec::with_capacity(1 + 2 + sps.len() + 2 + pps.len());
                stap.push(0x78); // F=0, NRI=3(11), Type=24
                stap.extend_from_slice(&(sps.len() as u16).to_be_bytes());
                stap.extend_from_slice(&sps);
                stap.extend_from_slice(&(pps.len() as u16).to_be_bytes());
                stap.extend_from_slice(&pps);
                let stap_bytes = Bytes::from(stap);
                let pkts = packetizer_guard.packetize(&stap_bytes, 0)?;
                for mut pkt in pkts {
                    if let Err(e) = track.write_rtp(&mut pkt).await {
                        return Err(e.into());
                    }
                }
            }
        }

        let mut pkts: Vec<RtpPacket> = Vec::new();
        for (idx, nalu) in nalus.iter().enumerate() {
            let is_last_nalu = idx == nalus.len() - 1;
            let n_bytes = Bytes::copy_from_slice(nalu);
            let samples_inc = if idx == 0 { samples_inc_frame } else { 0 };
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
        {
            let cfg = self.config.lock().await;
            if *cfg == new_config {
                return Ok(()); // 无变化
            }
        }

        // 构造新编码器(同步)，并放入 pending，等待首个关键帧后自动切换
        let mut new_enc = H264VideoEncoder::new(new_config.clone())?;
        new_enc.request_keyframe(); // 确保第一帧就是 IDR

        *self.pending_encoder.lock().await = Some(new_enc);
        *self.config.lock().await = new_config;
        log::info!("🔧 预热新编码器，等待关键帧无缝切换");
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
        
        // 辅助比较函数：允许±15% 码率、±20% fps 变化内忽略
        fn approx_equal(a: u32, b: u32, percent: u32) -> bool {
            let (max, min) = if a > b { (a, b) } else { (b, a) };
            (max - min) * 100 <= max * percent
        }

        // 冷却 10s，防抖动
        {
            let ts = self.last_cfg_change.lock().await;
            if ts.elapsed() < Duration::from_secs(10) {
                return Ok(());
            }
        }

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
                if new_config.quality != cfg.quality ||
                   !approx_equal(new_config.bitrate, cfg.bitrate, 15) ||
                   !approx_equal(new_config.fps as u32, cfg.fps as u32, 20) {
                    self.update_config(new_config).await?;
                    *self.last_cfg_change.lock().await = Instant::now();
                }
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
                if new_config.quality != cfg.quality ||
                   !approx_equal(new_config.bitrate, cfg.bitrate, 15) ||
                   !approx_equal(new_config.fps as u32, cfg.fps as u32, 20) {
                    self.update_config(new_config).await?;
                    *self.last_cfg_change.lock().await = Instant::now();
                }
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
                if new_config.quality != cfg.quality ||
                   !approx_equal(new_config.bitrate, cfg.bitrate, 15) ||
                   !approx_equal(new_config.fps as u32, cfg.fps as u32, 20) {
                    self.update_config(new_config).await?;
                    *self.last_cfg_change.lock().await = Instant::now();
                }
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
    pub fn encode_rgba_sync(&mut self, rgba_data: &[u8]) -> Result<Option<crate::video_encoder::EncodedFrame>> {
        let start_time = std::time::Instant::now();

        let use_pending = {
            let pen_opt = self.pending_encoder.blocking_lock();
            pen_opt.is_some()
        };

        let encoded_opt = if use_pending {
            // 用新的编码器尝试编码（同步）
            let mut pen_guard = self.pending_encoder.blocking_lock();
            let pen = pen_guard.as_mut().unwrap();
            let enc = pen.encode_frame_sync(rgba_data, 0)?;
            // 若尚未关键帧，则丢弃（返回None）
            if !enc.is_keyframe {
                log::debug!("⏳ 预热阶段丢弃非关键帧 {} bytes", enc.data.len());
                return Ok(None);
            }
            enc
        } else {
            let mut enc = self.encoder.blocking_lock();
            enc.encode_frame_sync(rgba_data, 0)?
        };

        let encoded = encoded_opt;

        // 如果成功且是 keyframe，并且用的是 pending，则完成切换
        if use_pending && encoded.is_keyframe {
            let mut pen_guard = self.pending_encoder.blocking_lock();
            if let Some(new_enc) = pen_guard.take() {
                let mut cur_guard = self.encoder.blocking_lock();
                *cur_guard = new_enc;
                log::info!("🎉 新编码器关键帧已发送，完成无缝切换");
            }
        }

        log::debug!(
            "🎬 [sync] 编码完成: {}字节, 用时{:.1}ms",
            encoded.data.len(),
            start_time.elapsed().as_millis()
        );

        Ok(Some(encoded))
    }

    /// 发送已编码帧到WebRTC轨道（异步）
    pub async fn send_encoded_frame(&self, frame: &crate::video_encoder::EncodedFrame) -> Result<()> {
        let bytes_sent = Self::send_h264_frame(&self.track, &self.packetizer, &self.last_sps, &self.last_pps, frame).await?;

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
