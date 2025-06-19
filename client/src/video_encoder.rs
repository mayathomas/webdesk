use anyhow::Result;
use log::{debug, warn};
use openh264::encoder::{BitRate, Encoder, EncoderConfig, FrameRate, RateControlMode, UsageType};
use openh264::OpenH264API;
use std::sync::Arc;
use tokio::sync::Mutex;

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

    /// 更新编码器配置
    pub async fn update_config(&mut self, new_config: VideoEncoderConfig) -> Result<()> {
        if self.config == new_config {
            return Ok(());
        }

        // 重新创建编码器，因为openh264 0.8.1没有update_config方法
        let new_encoder = Self::create_encoder(&new_config)?;
        let mut encoder = self.encoder.lock().await;
        *encoder = new_encoder;

        self.config = new_config;
        self.frame_count = 0;
        self.last_keyframe = 0;
        self.next_force_keyframe = true;
        Ok(())
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

    /// 将RGBA数据转换为YUV I420格式
    fn convert_rgba_to_yuv(&self, rgba_data: &[u8], width: usize, height: usize) -> Result<openh264::formats::YUVBuffer> {
        let mut rgb_data = Vec::with_capacity(width * height * 3);
        for chunk in rgba_data.chunks_exact(4) {
            rgb_data.extend_from_slice(&[chunk[0], chunk[1], chunk[2]]);
        }

        // 创建临时向量来存储YUV数据
        let y_size = width * height;
        let u_size = (width / 2) * (height / 2);
        let v_size = u_size;
        
        let mut y_data = vec![0u8; y_size];
        let mut u_data = vec![0u8; u_size];
        let mut v_data = vec![0u8; v_size];
        
        rgb_to_i420(&rgb_data, width as u32, height as u32, &mut y_data, &mut u_data, &mut v_data);
        
        // 将Y、U、V数据合并为一个向量，按照I420格式
        let mut yuv_data = Vec::with_capacity(y_size + u_size + v_size);
        yuv_data.extend_from_slice(&y_data);
        yuv_data.extend_from_slice(&u_data);
        yuv_data.extend_from_slice(&v_data);
        
        // 使用from_vec创建YUVBuffer
        let yuv = openh264::formats::YUVBuffer::from_vec(yuv_data, width, height);
        
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

/// 将RGB888 (R,G,B连续) 数据转换为 I420(YUV420 Planar)
pub fn rgb_to_i420(rgb: &[u8], width: u32, height: u32, y_out: &mut [u8], u_out: &mut [u8], v_out: &mut [u8]) {
    let u_width = (width / 2) as usize;
    let u_height = (height / 2) as usize;
    let u_size = u_width * u_height;
    let v_size = u_size;
    let y_size = (width * height) as usize;

    if y_out.len() < y_size || u_out.len() < u_size || v_out.len() < v_size {
        // Not enough space
        return;
    }

    let mut y_idx = 0;
    let mut u_idx = 0;
    let mut v_idx = 0;

    for j in 0..height {
        for i in 0..width {
            let r = rgb[((j * width + i) * 3) as usize] as i32;
            let g = rgb[((j * width + i) * 3 + 1) as usize] as i32;
            let b = rgb[((j * width + i) * 3 + 2) as usize] as i32;

            let y = ((66 * r + 129 * g + 25 * b + 128) >> 8) + 16;
            y_out[y_idx] = y.clamp(16, 235) as u8;
            y_idx += 1;

            if j % 2 == 0 && i % 2 == 0 {
                let u = ((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128;
                let v = ((112 * r - 94 * g - 18 * b + 128) >> 8) + 128;

                u_out[u_idx] = u.clamp(16, 240) as u8;
                v_out[v_idx] = v.clamp(16, 240) as u8;

                u_idx += 1;
                v_idx += 1;
            }
        }
    }
}