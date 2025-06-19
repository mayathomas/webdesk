use anyhow::Result;
use log::{debug, warn};
use openh264::encoder::{BitRate, Encoder, EncoderConfig, FrameRate, RateControlMode, UsageType};
use openh264::OpenH264API;
use std::sync::Arc;
use tokio::sync::Mutex;
use yuv::{rgba_to_yuv420, YuvPlanarImageMut, YuvRange, YuvStandardMatrix, YuvConversionMode, YuvChromaSubsampling};

/// 视频编解码器类型
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VideoCodec {
    VP8,
}

/// 视频编码器配置
#[derive(Debug, Clone, PartialEq)]
pub struct VideoEncoderConfig {
    pub width: u32,
    pub height: u32,
    pub fps: f32,
    pub bitrate: u32,
    pub codec: VideoCodec,
    pub quality: u8, // 0-100
}


/// 编码后的视频帧
#[derive(Debug, Clone)]
pub struct EncodedFrame {
    pub data: Vec<u8>,
    pub is_keyframe: bool,
}

/// H.264视频编码器
pub struct H264VideoEncoder {
    encoder: Arc<Mutex<Encoder>>,
    config: VideoEncoderConfig,
    frame_count: u64,
    last_keyframe: u64,
    next_force_keyframe: bool,
}

impl H264VideoEncoder {
    /// 创建新的H.264编码器
    pub fn new(config: VideoEncoderConfig) -> Result<Self> {
        let encoder = Self::create_encoder(&config)?;
        Ok(Self {
            encoder: Arc::new(Mutex::new(encoder)),
            config,
            frame_count: 0,
            last_keyframe: 0,
            next_force_keyframe: true,
        })
    }

    /// 创建配置好的编码器
    fn create_encoder(_config: &VideoEncoderConfig) -> Result<Encoder> {
        let api = OpenH264API::from_source();
        
        // 使用openh264 0.8.1的正确API - 直接创建编码器，然后配置
        let cfg = EncoderConfig::new()
            .max_frame_rate(FrameRate::from_hz(_config.fps))
            .bitrate(BitRate::from_bps(_config.bitrate))
            .skip_frames(false)
            .rate_control_mode(RateControlMode::Off)
            .usage_type(UsageType::ScreenContentRealTime);

        let encoder = Encoder::with_api_config(api, cfg)?;
        Ok(encoder)
    }

    /// 编码RGBA帧数据为H.264格式
    pub async fn encode_frame(&mut self, rgba_data: &[u8], _timestamp: u64) -> Result<EncodedFrame> {
        self.frame_count += 1;
        let width = self.config.width;
        let height = self.config.height;
        
        let yuv_buffer = self.convert_rgba_to_yuv(rgba_data, width as usize, height as usize)?;

        let mut encoder = self.encoder.lock().await;

        // 若等待关键帧，则在真正编码前请求 IDR
        if self.next_force_keyframe {
            let _ = encoder.force_intra_frame();
            debug!("📣 已向 OpenH264 请求 IDR 帧 (#{}).", self.frame_count);
            // 重置标志，避免连续多帧都被标记
            self.next_force_keyframe = false;
        }

        let encoded_slice = encoder.encode(&yuv_buffer)?;
        let frame_data = encoded_slice.to_vec();
        let is_keyframe = self.is_keyframe(&frame_data);
        
        if is_keyframe {
            log::info!("✅ 成功编码I帧 (关键帧): {}字节", frame_data.len());
            self.last_keyframe = self.frame_count;
        }

        if frame_data.is_empty() {
            warn!("⚠️ H.264编码器产生了空帧数据 - 帧#{}", self.frame_count);
        }

        self.validate_annex_b(&frame_data);

        if self.frame_count - self.last_keyframe > 30 {
            self.next_force_keyframe = true;
        }

        Ok(EncodedFrame {
            data: frame_data,
            is_keyframe,
        })
    }

    /// 编码RGBA帧数据为H.264格式 (同步版本，供阻塞线程调用，避免在已有异步执行器中再创建执行器)
    pub fn encode_frame_sync(&mut self, rgba_data: &[u8], _timestamp: u64) -> Result<EncodedFrame> {
        // 注意：此实现与 `encode_frame` 基本一致，但不依赖 async/await，直接使用 blocking_lock()
        self.frame_count += 1;
        let width = self.config.width;
        let height = self.config.height;

        let yuv_buffer = self.convert_rgba_to_yuv(rgba_data, width as usize, height as usize)?;

        // 阻塞方式获取内部 OpenH264 encoder 的可变引用
        let mut encoder = self.encoder.blocking_lock();

        // 若等待关键帧，则在真正编码前请求 IDR
        if self.next_force_keyframe {
            let _ = encoder.force_intra_frame();
            debug!("📣 已向 OpenH264 请求 IDR 帧 (#{}).", self.frame_count);
            // 重置标志，避免连续多帧都被标记
            self.next_force_keyframe = false;
        }

        let encoded_slice = encoder.encode(&yuv_buffer)?;
        let frame_data = encoded_slice.to_vec();
        let is_keyframe = self.is_keyframe(&frame_data);

        if is_keyframe {
            log::info!("✅ 成功编码I帧 (关键帧): {}字节", frame_data.len());
            self.last_keyframe = self.frame_count;
        }

        if frame_data.is_empty() {
            warn!("⚠️ H.264编码器产生了空帧数据 - 帧#{}", self.frame_count);
        }

        self.validate_annex_b(&frame_data);

        // 若距上次关键帧超过阈值，则触发下一帧关键帧
        if self.frame_count - self.last_keyframe > 30 {
            self.next_force_keyframe = true;
        }

        Ok(EncodedFrame {
            data: frame_data,
            is_keyframe,
        })
    }

    /// 将RGBA数据转换为YUV I420格式（SIMD加速，基于 yuvutils-rs）
    fn convert_rgba_to_yuv(&self, rgba_data: &[u8], width: usize, height: usize) -> Result<openh264::formats::YUVBuffer> {
        // 使用 yuvutils_rs 分配并转换
        let mut planar = YuvPlanarImageMut::<u8>::alloc(
            width as u32,
            height as u32,
            YuvChromaSubsampling::Yuv420,
        );

        // 执行 SIMD 转换（RGBA -> YUV420P）
        rgba_to_yuv420(
            &mut planar,
            rgba_data,
            (width * 4) as u32,
            YuvRange::Limited,
            YuvStandardMatrix::Bt709,
            YuvConversionMode::Balanced,
        ).map_err(|e| anyhow::anyhow!("rgba_to_yuv420 failed: {e}"))?;

        // 通过 borrow() 获取只读切片，兼容 yuv crate 的 BufferStoreMut API
        let y_plane = planar.y_plane.borrow();
        let u_plane = planar.u_plane.borrow();
        let v_plane = planar.v_plane.borrow();

        let mut yuv_vec = Vec::with_capacity(y_plane.len() + u_plane.len() + v_plane.len());
        yuv_vec.extend_from_slice(y_plane);
        yuv_vec.extend_from_slice(u_plane);
        yuv_vec.extend_from_slice(v_plane);

        let yuv = openh264::formats::YUVBuffer::from_vec(yuv_vec, width, height);
        Ok(yuv)
    }

    /// 检查编码后的数据是否为关键帧 (IDR)
    fn is_keyframe(&self, frame_data: &[u8]) -> bool {
        // 简单检查是否存在NAL类型为5的单元 (IDR)
        // H.264 Annex B格式: 00 00 00 01 [NAL Header]
        let start_code3: [u8; 3] = [0, 0, 1];
        let start_code4: [u8; 4] = [0, 0, 0, 1];

        for (i, _) in frame_data.windows(4).enumerate() {
            let offset = if i > 0 && frame_data[i - 1] == 0 { 3 } else { 4 };
            if i + offset >= frame_data.len() { continue; }
            
            if frame_data[i..].starts_with(&start_code4) || frame_data[i..].starts_with(&start_code3) {
                let nal_header = frame_data[i + offset];
                let nal_type = nal_header & 0x1F;
                if nal_type == 5 {
                    return true;
                }
            }
        }
        false
    }

    /// 验证并打印H.264 Annex B格式的NAL单元
    fn validate_annex_b(&self, data: &[u8]) {
        if !log::log_enabled!(log::Level::Debug) || data.is_empty() {
            return;
        }

        let mut count = 0;
        let mut i = 0;
        while i < data.len() {
            // Find start code
            if let Some(start_pos) = self.find_start_code(&data[i..]) {
                let pos = i + start_pos;
                let nal_header_pos = pos + if pos > 0 && data[pos - 1] == 0 { 3 } else { 4 };
                
                if nal_header_pos < data.len() {
                    let nal_header = data[nal_header_pos];
                    let nal_type = nal_header & 0x1F;
                    log::debug!(
                        "🔍 发现 {}字节起始码，偏移{}, NAL类型{}",
                        if nal_header_pos - pos == 4 { "4" } else { "3" },
                        pos,
                        nal_type
                    );
                    count += 1;
                    i = nal_header_pos;
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        
        if count > 0 {
            log::debug!("✅ H.264 Annex B格式验证通过: 找到{}个NAL单元", count);
        } else {
            log::warn!("⚠️ 未在编码输出中找到H.264 Annex B起始码");
        }
    }

    fn find_start_code(&self, data: &[u8]) -> Option<usize> {
        data.windows(4).position(|window| window == [0, 0, 0, 1])
            .or_else(|| data.windows(3).position(|window| window == [0, 0, 1]))
    }

    pub fn get_config(&self) -> &VideoEncoderConfig {
        &self.config
    }

    /// 请求下一帧编码为关键帧
    pub fn request_keyframe(&mut self) {
        self.next_force_keyframe = true;
    }
}

impl Default for VideoEncoderConfig {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            fps: 30.0,
            bitrate: 2000000, // 2 Mbps
            codec: VideoCodec::VP8, // 默认使用VP8
            quality: 75,
        }
    }
}

/// 编码器工厂 - 方便创建不同配置的编码器
pub struct VideoEncoderFactory;

impl VideoEncoderFactory {
    /// 创建高质量编码器 (适合高速网络)
    pub fn create_high_quality(width: u32, height: u32) -> Result<H264VideoEncoder> {
        let config = VideoEncoderConfig {
            width,
            height,
            fps: 60.0,
            bitrate: 8_000_000, // 8Mbps
            codec: VideoCodec::VP8,
            quality: 90,
        };
        H264VideoEncoder::new(config)
    }

    /// 创建标准质量编码器 (平衡性能和质量)
    pub fn create_standard_quality(width: u32, height: u32) -> Result<H264VideoEncoder> {
        let config = VideoEncoderConfig {
            width,
            height,
            fps: 30.0,
            bitrate: 2_000_000, // 2Mbps
            codec: VideoCodec::VP8,
            quality: 75,
        };
        H264VideoEncoder::new(config)
    }

    /// 创建低延迟编码器 (适合慢速网络)
    pub fn create_low_latency(width: u32, height: u32) -> Result<H264VideoEncoder> {
        let config = VideoEncoderConfig {
            width,
            height,
            fps: 30.0,        // 提升到 30fps 减少首帧等待
            bitrate: 800_000, // 略提高起始码率
            codec: VideoCodec::VP8,
            quality: 60,
        };
        H264VideoEncoder::new(config)
    }

    /// 根据网络条件自动选择配置
    pub fn create_adaptive(width: u32, height: u32, network_quality: NetworkQuality) -> Result<H264VideoEncoder> {
        match network_quality {
            NetworkQuality::Excellent => Self::create_high_quality(width, height),
            NetworkQuality::Good => Self::create_standard_quality(width, height),
            NetworkQuality::Poor => Self::create_low_latency(width, height),
        }
    }
}

/// 网络质量等级
#[derive(Debug, Clone, Copy)]
pub enum NetworkQuality {
    Excellent, // 优秀 (>10Mbps, RTT<50ms)
    Good,      // 良好 (1-10Mbps, RTT<200ms)
    Poor,      // 较差 (<1Mbps, RTT>200ms)
}