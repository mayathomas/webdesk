use anyhow::Result;
use scrap::{Capturer, Display};
use std::io::ErrorKind::WouldBlock;
use std::thread;
use std::time::Duration;
use std::sync::{Arc, Mutex};

use crate::video_track::VideoTrackManager;
use crate::video_encoder::{VideoEncoderConfig, VideoCodec};

/// 基于视频流的屏幕捕获服务 - 模仿Chrome Remote Desktop
pub struct VideoScreenCaptureService {
    video_track_manager: Arc<Mutex<VideoTrackManager>>,
    capture_config: CaptureConfig,
    is_running: bool,
    frame_count: u64,
}

#[derive(Debug, Clone)]
pub struct CaptureConfig {
    pub target_fps: u32,
    pub max_width: u32,
    pub max_height: u32,
    pub quality_level: QualityLevel,
}

#[derive(Debug, Clone)]
pub enum QualityLevel {
    Low,    // 低质量：快速响应
    Medium, // 中等质量：平衡
    High,   // 高质量：最佳视觉效果
}

impl VideoScreenCaptureService {
    /// 创建新的视频屏幕捕获服务
    pub fn new(video_track_manager: Arc<Mutex<VideoTrackManager>>) -> Self {
        Self {
            video_track_manager,
            capture_config: CaptureConfig::default(),
            is_running: false,
            frame_count: 0,
        }
    }
    
    /// 启动视频捕获（生成线程）
    pub fn start_capture_thread(&mut self) -> Result<()> {
        if self.is_running {
            log::warn!("📹 视频捕获已在运行");
            return Ok(());
        }
        
        log::info!("🎬 启动视频屏幕捕获线程 ({}fps)", self.capture_config.target_fps);
        self.is_running = true;
        
        let video_track_manager = self.video_track_manager.clone();
        let config = self.capture_config.clone();
        
        // 在专用线程中运行视频捕获（避免Send问题）
        std::thread::spawn(move || {
            if let Err(e) = Self::capture_loop_thread(video_track_manager, config) {
                log::error!("❌ 视频捕获线程失败: {}", e);
            }
        });
        
        Ok(())
    }
    
    /// 专用的捕获循环（在独立线程中运行）
    fn capture_loop_thread(
        video_track_manager: Arc<Mutex<VideoTrackManager>>,
        config: CaptureConfig,
    ) -> Result<()> {
        // 获取主显示器
        let display = Display::primary()?;
        let mut capturer = Capturer::new(display)?;
        
        let original_width = capturer.width() as u32;
        let original_height = capturer.height() as u32;
        
        // 计算目标分辨率
        let target_pixels = match config.quality_level {
            QualityLevel::Low => 1280 * 720,
            QualityLevel::Medium => 1920 * 1080,
            QualityLevel::High => 2560 * 1440,
        };
        
        let original_pixels = original_width * original_height;
        let (target_width, target_height) = if original_pixels <= target_pixels {
            (original_width, original_height)
        } else {
            let scale_factor = (target_pixels as f32 / original_pixels as f32).sqrt();
            let new_width = ((original_width as f32 * scale_factor) as u32 / 2) * 2;
            let new_height = ((original_height as f32 * scale_factor) as u32 / 2) * 2;
            (new_width, new_height)
        };
        
        // 配置视频编码器
        let encoder_config = VideoEncoderConfig {
            width: target_width,
            height: target_height,
            fps: config.target_fps,
            bitrate: Self::calculate_bitrate_static(target_width, target_height, &config.quality_level),
            keyframe_interval: config.target_fps,
            codec: VideoCodec::VP8,
        };
        
        // 初始化视频轨道
        {
            let mut manager = video_track_manager.lock().unwrap();
            let _track = manager.initialize(encoder_config)?;
            manager.activate();
        }
        
        log::info!("📺 视频流配置: {}x{}@{}fps, 编码器: VP8", target_width, target_height, config.target_fps);
        
        // 计算帧间隔
        let frame_interval = Duration::from_millis(1000 / config.target_fps as u64);
        let mut frame_count = 0u64;
        let mut last_frame_time = std::time::Instant::now();
        
        // 捕获循环
        loop {
            let capture_start = std::time::Instant::now();
            
            // 捕获屏幕帧
            let rgba_data = match Self::capture_frame_static(&mut capturer, target_width, target_height, original_width, original_height) {
                Ok(data) => data,
                Err(e) => {
                    log::error!("❌ 屏幕捕获失败: {}", e);
                    std::thread::sleep(frame_interval);
                    continue;
                }
            };
            
            // 发送到视频轨道（使用同步方式）
            {
                let manager = video_track_manager.lock().unwrap();
                // 这里我们暂时跳过实际的视频帧发送，因为需要异步上下文
                // 实际项目中需要使用通道或其他机制
                drop(manager);
            }
            
            frame_count += 1;
            
            // 性能统计
            if frame_count % (config.target_fps as u64) == 0 {
                let capture_time = capture_start.elapsed();
                log::debug!("🎬 捕获第{}帧，耗时: {:?}", frame_count, capture_time);
            }
            
            // 帧率控制
            let elapsed = last_frame_time.elapsed();
            if elapsed < frame_interval {
                std::thread::sleep(frame_interval - elapsed);
            }
            last_frame_time = std::time::Instant::now();
        }
    }
    
    /// 静态方法：捕获帧数据
    fn capture_frame_static(
        capturer: &mut Capturer,
        target_width: u32,
        target_height: u32,
        original_width: u32,
        original_height: u32,
    ) -> Result<Vec<u8>> {
        // 捕获原始帧
        let buffer = loop {
            match capturer.frame() {
                Ok(buffer) => break buffer,
                Err(error) => {
                    if error.kind() == WouldBlock {
                        thread::sleep(Duration::from_millis(1));
                        continue;
                    } else {
                        return Err(anyhow::anyhow!("屏幕捕获失败: {}", error));
                    }
                }
            }
        };
        
        // 转换BGRA到RGBA
        let mut rgba_data = Vec::with_capacity(buffer.len());
        for chunk in buffer.chunks(4) {
            if chunk.len() >= 4 {
                rgba_data.push(chunk[2]); // R
                rgba_data.push(chunk[1]); // G
                rgba_data.push(chunk[0]); // B
                rgba_data.push(chunk[3]); // A
            }
        }
        
        // 如果需要缩放
        if target_width != original_width || target_height != original_height {
            rgba_data = Self::resize_rgba_static(&rgba_data, original_width, original_height, target_width, target_height)?;
        }
        
        Ok(rgba_data)
    }
    
    /// 静态方法：调整RGBA帧大小
    fn resize_rgba_static(
        rgba_data: &[u8],
        src_width: u32,
        src_height: u32,
        dst_width: u32,
        dst_height: u32,
    ) -> Result<Vec<u8>> {
        let mut resized = Vec::with_capacity((dst_width * dst_height * 4) as usize);
        
        let x_ratio = src_width as f32 / dst_width as f32;
        let y_ratio = src_height as f32 / dst_height as f32;
        
        for y in 0..dst_height {
            for x in 0..dst_width {
                let src_x = (x as f32 * x_ratio) as u32;
                let src_y = (y as f32 * y_ratio) as u32;
                
                if src_x < src_width && src_y < src_height {
                    let src_idx = ((src_y * src_width + src_x) * 4) as usize;
                    if src_idx + 3 < rgba_data.len() {
                        resized.push(rgba_data[src_idx]);
                        resized.push(rgba_data[src_idx + 1]);
                        resized.push(rgba_data[src_idx + 2]);
                        resized.push(rgba_data[src_idx + 3]);
                    } else {
                        resized.extend_from_slice(&[0, 0, 0, 255]);
                    }
                } else {
                    resized.extend_from_slice(&[0, 0, 0, 255]);
                }
            }
        }
        
        Ok(resized)
    }
    
    /// 静态方法：计算码率
    fn calculate_bitrate_static(width: u32, height: u32, quality: &QualityLevel) -> u32 {
        let pixels = width * height;
        
        match quality {
            QualityLevel::Low => (pixels / 1000).max(500_000),
            QualityLevel::Medium => (pixels / 500).max(1_000_000),
            QualityLevel::High => (pixels / 300).max(2_000_000),
        }
    }
    
    /// 停止捕获循环
    pub fn stop_capture(&mut self) {
        log::info!("🛑 停止视频捕获循环");
        self.is_running = false;
    }
    
    /// 捕获单帧屏幕数据
    fn capture_screen_frame(&self, capturer: &mut Capturer, target_width: u32, target_height: u32) -> Result<Vec<u8>> {
        // 获取原始尺寸（在捕获之前）
        let original_width = capturer.width() as u32;
        let original_height = capturer.height() as u32;
        
        // 捕获原始帧
        let buffer = loop {
            match capturer.frame() {
                Ok(buffer) => break buffer,
                Err(error) => {
                    if error.kind() == WouldBlock {
                        thread::sleep(Duration::from_millis(1));
                        continue;
                    } else {
                        return Err(anyhow::anyhow!("屏幕捕获失败: {}", error));
                    }
                }
            }
        };
        
        // 转换BGRA到RGBA
        let mut rgba_data = Vec::with_capacity(buffer.len());
        for chunk in buffer.chunks(4) {
            if chunk.len() >= 4 {
                rgba_data.push(chunk[2]); // R
                rgba_data.push(chunk[1]); // G  
                rgba_data.push(chunk[0]); // B
                rgba_data.push(chunk[3]); // A
            }
        }
        
        // 如果需要缩放
        if target_width != original_width || target_height != original_height {
            rgba_data = self.resize_rgba_frame(&rgba_data, original_width, original_height, target_width, target_height)?;
        }
        
        Ok(rgba_data)
    }
    
    /// 调整RGBA帧大小
    fn resize_rgba_frame(
        &self, 
        rgba_data: &[u8], 
        src_width: u32, 
        src_height: u32, 
        dst_width: u32, 
        dst_height: u32
    ) -> Result<Vec<u8>> {
        // 使用简单的双线性插值缩放
        let mut resized = Vec::with_capacity((dst_width * dst_height * 4) as usize);
        
        let x_ratio = src_width as f32 / dst_width as f32;
        let y_ratio = src_height as f32 / dst_height as f32;
        
        for y in 0..dst_height {
            for x in 0..dst_width {
                let src_x = (x as f32 * x_ratio) as u32;
                let src_y = (y as f32 * y_ratio) as u32;
                
                if src_x < src_width && src_y < src_height {
                    let src_idx = ((src_y * src_width + src_x) * 4) as usize;
                    if src_idx + 3 < rgba_data.len() {
                        resized.push(rgba_data[src_idx]);     // R
                        resized.push(rgba_data[src_idx + 1]); // G
                        resized.push(rgba_data[src_idx + 2]); // B
                        resized.push(rgba_data[src_idx + 3]); // A
                    } else {
                        resized.extend_from_slice(&[0, 0, 0, 255]); // 黑色像素
                    }
                } else {
                    resized.extend_from_slice(&[0, 0, 0, 255]); // 黑色像素
                }
            }
        }
        
        Ok(resized)
    }
    
    /// 计算目标分辨率 - 基于Chrome Remote Desktop的策略
    fn calculate_target_resolution(&self, original_width: u32, original_height: u32) -> (u32, u32) {
        let max_pixels = match self.capture_config.quality_level {
            QualityLevel::Low => 1280 * 720,    // 720p
            QualityLevel::Medium => 1920 * 1080, // 1080p
            QualityLevel::High => 2560 * 1440,   // 1440p
        };
        
        let original_pixels = original_width * original_height;
        
        if original_pixels <= max_pixels {
            return (original_width, original_height);
        }
        
        // 等比例缩放
        let scale_factor = (max_pixels as f32 / original_pixels as f32).sqrt();
        let new_width = ((original_width as f32 * scale_factor) as u32 / 2) * 2; // 确保偶数
        let new_height = ((original_height as f32 * scale_factor) as u32 / 2) * 2;
        
        (new_width, new_height)
    }
    
    /// 计算目标码率
    fn calculate_bitrate(&self, width: u32, height: u32) -> u32 {
        let pixels = width * height;
        
        match self.capture_config.quality_level {
            QualityLevel::Low => (pixels / 1000).max(500_000),    // 0.5-2 Mbps
            QualityLevel::Medium => (pixels / 500).max(1_000_000), // 1-4 Mbps
            QualityLevel::High => (pixels / 300).max(2_000_000),   // 2-8 Mbps
        }
    }
    
    /// 更新捕获配置
    pub fn update_config(&mut self, config: CaptureConfig) {
        self.capture_config = config;
        log::info!("🔧 更新捕获配置: {}fps, {:?}", 
            self.capture_config.target_fps, self.capture_config.quality_level);
    }
    
    /// 获取捕获统计信息
    pub async fn get_capture_stats(&self) -> CaptureStats {
        let video_stats = {
            let manager = self.video_track_manager.lock().unwrap();
            manager.get_stats().await
        };
        
        CaptureStats {
            is_running: self.is_running,
            frame_count: self.frame_count,
            target_fps: self.capture_config.target_fps,
            quality_level: self.capture_config.quality_level.clone(),
            video_track_stats: video_stats,
        }
    }
}

/// 捕获统计信息
#[derive(Debug, Clone)]
pub struct CaptureStats {
    pub is_running: bool,
    pub frame_count: u64,
    pub target_fps: u32,
    pub quality_level: QualityLevel,
    pub video_track_stats: Option<crate::video_track::VideoTrackStats>,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            target_fps: 30,        // Chrome RD典型帧率
            max_width: 1920,       // 最大宽度
            max_height: 1080,      // 最大高度  
            quality_level: QualityLevel::Medium, // 默认中等质量
        }
    }
}