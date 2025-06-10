use anyhow::Result;

/// 视频编码器配置
#[derive(Debug, Clone)]
pub struct VideoEncoderConfig {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate: u32,
    pub keyframe_interval: u32,
    pub codec: VideoCodec,
}

#[derive(Debug, Clone)]
pub enum VideoCodec {
    VP8,
    VP9,
    H264,
}

/// 编码后的视频帧
#[derive(Debug, Clone)]
pub struct EncodedFrame {
    pub data: Vec<u8>,
    pub is_keyframe: bool,
    pub timestamp: u64,
    pub frame_type: FrameType,
}

#[derive(Debug, Clone)]
pub enum FrameType {
    KeyFrame,
    DeltaFrame,
}

/// 视频编码器 - 模仿Chrome Remote Desktop的VP8编码器
pub struct VideoEncoder {
    config: VideoEncoderConfig,
    frame_count: u64,
    last_keyframe: u64,
}

impl VideoEncoder {
    /// 创建新的视频编码器实例
    pub fn new(config: VideoEncoderConfig) -> Result<Self> {
        log::info!("🎬 初始化视频编码器: {:?}x{}@{}fps, {:?}", 
            config.width, config.height, config.fps, config.codec);
            
        Ok(Self {
            config,
            frame_count: 0,
            last_keyframe: 0,
        })
    }
    
    /// 编码RGBA帧数据为视频
    pub fn encode_frame(&mut self, rgba_data: &[u8], timestamp: u64) -> Result<EncodedFrame> {
        self.frame_count += 1;
        
        // 决定是否为关键帧（参考Chrome RD的策略）
        let force_keyframe = self.frame_count == 1 || 
            (self.frame_count - self.last_keyframe) >= self.config.keyframe_interval as u64;
            
        if force_keyframe {
            self.last_keyframe = self.frame_count;
        }
        
        let frame_type = if force_keyframe {
            FrameType::KeyFrame
        } else {
            FrameType::DeltaFrame
        };
        
        // 根据编码器类型编码
        let encoded_data = match self.config.codec {
            VideoCodec::VP8 => self.encode_vp8(rgba_data, force_keyframe)?,
            VideoCodec::H264 => self.encode_h264(rgba_data, force_keyframe)?,
            VideoCodec::VP9 => self.encode_vp9(rgba_data, force_keyframe)?,
        };
        
        log::debug!("🎬 编码完成: 帧#{}, {}字节, {}", 
            self.frame_count, encoded_data.len(),
            if force_keyframe { "关键帧" } else { "差分帧" });
        
        Ok(EncodedFrame {
            data: encoded_data,
            is_keyframe: force_keyframe,
            timestamp,
            frame_type,
        })
    }
    
    /// 使用VP8编码器 - Chrome Remote Desktop的首选编码器
    fn encode_vp8(&self, rgba_data: &[u8], is_keyframe: bool) -> Result<Vec<u8>> {
        // 简化的VP8编码实现
        // 在实际项目中，这里会使用libvpx或类似库
        
        // 转换RGBA到YUV420P格式（VP8要求）
        let yuv_data = self.rgba_to_yuv420p(rgba_data)?;
        
        // 模拟VP8编码
        let mut encoded = Vec::new();
        
        // VP8帧头
        if is_keyframe {
            encoded.extend_from_slice(&[0x10, 0x02, 0x00]); // VP8关键帧头
        } else {
            encoded.extend_from_slice(&[0x30, 0x02, 0x00]); // VP8差分帧头
        }
        
        // 简化的压缩（实际应使用VP8库）
        let compressed = self.simple_compress(&yuv_data);
        encoded.extend_from_slice(&compressed);
        
        Ok(encoded)
    }
    
    /// 使用H.264编码器
    fn encode_h264(&self, rgba_data: &[u8], is_keyframe: bool) -> Result<Vec<u8>> {
        // H.264编码实现
        let yuv_data = self.rgba_to_yuv420p(rgba_data)?;
        
        let mut encoded = Vec::new();
        
        // H.264 NAL单元头
        if is_keyframe {
            encoded.extend_from_slice(&[0x00, 0x00, 0x00, 0x01, 0x67]); // SPS
            encoded.extend_from_slice(&[0x00, 0x00, 0x00, 0x01, 0x68]); // PPS
            encoded.extend_from_slice(&[0x00, 0x00, 0x00, 0x01, 0x65]); // IDR帧
        } else {
            encoded.extend_from_slice(&[0x00, 0x00, 0x00, 0x01, 0x61]); // P帧
        }
        
        let compressed = self.simple_compress(&yuv_data);
        encoded.extend_from_slice(&compressed);
        
        Ok(encoded)
    }
    
    /// 使用VP9编码器
    fn encode_vp9(&self, rgba_data: &[u8], is_keyframe: bool) -> Result<Vec<u8>> {
        // VP9编码实现
        let yuv_data = self.rgba_to_yuv420p(rgba_data)?;
        
        let mut encoded = Vec::new();
        
        // VP9帧头
        if is_keyframe {
            encoded.extend_from_slice(&[0x82, 0x49, 0x83, 0x42]); // VP9关键帧
        } else {
            encoded.extend_from_slice(&[0x02, 0x49, 0x83, 0x42]); // VP9差分帧
        }
        
        let compressed = self.simple_compress(&yuv_data);
        encoded.extend_from_slice(&compressed);
        
        Ok(encoded)
    }
    
    /// 将RGBA转换为YUV420P格式
    fn rgba_to_yuv420p(&self, rgba_data: &[u8]) -> Result<Vec<u8>> {
        let pixel_count = (self.config.width * self.config.height) as usize;
        let mut yuv_data = Vec::with_capacity(pixel_count * 3 / 2);
        
        // Y分量 (亮度)
        let mut y_plane = Vec::with_capacity(pixel_count);
        for chunk in rgba_data.chunks(4) {
            if chunk.len() >= 3 {
                let r = chunk[0] as f32;
                let g = chunk[1] as f32;
                let b = chunk[2] as f32;
                
                // ITU-R BT.601转换公式
                let y = (0.299 * r + 0.587 * g + 0.114 * b) as u8;
                y_plane.push(y);
            }
        }
        
        // U和V分量 (色度) - 每2x2像素采样一次
        let chroma_width = self.config.width / 2;
        let chroma_height = self.config.height / 2;
        let chroma_count = (chroma_width * chroma_height) as usize;
        
        let mut u_plane = Vec::with_capacity(chroma_count);
        let mut v_plane = Vec::with_capacity(chroma_count);
        
        for y in (0..self.config.height).step_by(2) {
            for x in (0..self.config.width).step_by(2) {
                let idx = (y * self.config.width + x) as usize * 4;
                if idx + 2 < rgba_data.len() {
                    let r = rgba_data[idx] as f32;
                    let g = rgba_data[idx + 1] as f32;
                    let b = rgba_data[idx + 2] as f32;
                    
                    let u = (-0.147 * r - 0.289 * g + 0.436 * b + 128.0) as u8;
                    let v = (0.615 * r - 0.515 * g - 0.100 * b + 128.0) as u8;
                    
                    u_plane.push(u);
                    v_plane.push(v);
                }
            }
        }
        
        // 组合YUV平面
        yuv_data.extend_from_slice(&y_plane);
        yuv_data.extend_from_slice(&u_plane);
        yuv_data.extend_from_slice(&v_plane);
        
        Ok(yuv_data)
    }
    
    /// 简单的数据压缩 (实际应使用专业编码库)
    fn simple_compress(&self, data: &[u8]) -> Vec<u8> {
        // 使用简单的RLE压缩作为占位符
        let mut compressed = Vec::new();
        
        if data.is_empty() {
            return compressed;
        }
        
        let mut current_byte = data[0];
        let mut count = 1u8;
        
        for &byte in data.iter().skip(1) {
            if byte == current_byte && count < 255 {
                count += 1;
            } else {
                compressed.push(count);
                compressed.push(current_byte);
                current_byte = byte;
                count = 1;
            }
        }
        
        // 添加最后一组
        compressed.push(count);
        compressed.push(current_byte);
        
        compressed
    }
    
    /// 更新编码器配置
    pub fn update_config(&mut self, config: VideoEncoderConfig) -> Result<()> {
        log::info!("🔧 更新视频编码器配置: {:?}x{}@{}fps", 
            config.width, config.height, config.fps);
        self.config = config;
        Ok(())
    }
    
    /// 获取当前配置
    pub fn get_config(&self) -> &VideoEncoderConfig {
        &self.config
    }
}

impl Default for VideoEncoderConfig {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            fps: 30,
            bitrate: 2000000, // 2 Mbps
            keyframe_interval: 30, // 每30帧一个关键帧
            codec: VideoCodec::VP8, // Chrome RD默认使用VP8
        }
    }
}