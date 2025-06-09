use anyhow::Result;
use base64::{Engine as _, engine::general_purpose};
use scrap::{Capturer, Display};
use std::io::ErrorKind::WouldBlock;
use std::thread;
use std::time::Duration;
use serde::{Serialize, Deserialize};
use rayon::prelude::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangedRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub data: String, // base64编码的区域数据
}

#[derive(Debug, Clone)]
pub struct ScreenFrame {
    pub data: Vec<u8>, // RGBA数据
    pub width: u32,
    pub height: u32,
}

/// 屏幕捕获服务 - 支持JPEG压缩和差分编码
pub struct ScreenCaptureService {
    last_frame: Option<ScreenFrame>,
    frame_count: u32,
}

impl ScreenCaptureService {
    /// 创建新的屏幕捕获服务实例
    pub fn new() -> Self {
        Self {
            last_frame: None,
            frame_count: 0,
        }
    }
    
    /// 创建一个新的屏幕捕获器实例（在调用线程中）
    pub fn create_capturer() -> Result<Capturer> {
        let display = Display::primary()?;
        let capturer = Capturer::new(display)?;
        Ok(capturer)
    }
    
    /// 捕获屏幕并返回优化后的数据
    pub fn capture_screen_optimized(&mut self, capturer: &mut Capturer) -> Result<(String, u32, u32, String, bool, Option<Vec<ChangedRegion>>)> {
        let start_time = std::time::Instant::now();
        
        let original_width = capturer.width() as u32;
        let original_height = capturer.height() as u32;
        
        // 调试：输出分辨率信息（仅首次）
        if self.frame_count == 0 {
            log::debug!("📊 原始分辨率: {}x{} ({:.1}MP)", original_width, original_height, (original_width * original_height) as f64 / 1_000_000.0);
            let estimated_size = (original_width * original_height * 3) as f64 / 1_000_000.0; // RGB估算
            log::debug!("📊 估算未压缩大小: {:.1}MB", estimated_size);
        }
        
        // 捕获原始帧数据
        let capture_start = std::time::Instant::now();
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
        let capture_time = capture_start.elapsed();

        // 格式优化 - 直接使用BGRA格式，避免颜色通道重排
        let convert_start = std::time::Instant::now();
        
        // 直接使用原始BGRA数据，无需转换
        let bgra_data = buffer.to_vec();
        let convert_time = convert_start.elapsed();

        // 立即进行分辨率缩放以减少后续处理的数据量
        let scale_start = std::time::Instant::now();
        let (width, height, rgba_data) = self.scale_frame_if_needed(original_width, original_height, bgra_data)?;
        let scale_time = scale_start.elapsed();

        // 性能统计（每10帧输出一次）
        if self.frame_count % 10 == 0 {
            log::debug!("⏱️ 性能统计 - 捕获: {:?}, 转换: {:?}, 缩放: {:?}, 总计: {:?}", 
                capture_time, convert_time, scale_time, start_time.elapsed());
        }

        let current_frame = ScreenFrame {
            data: rgba_data.clone(),
            width,
            height,
        };

        self.frame_count += 1;
        
        // 根据业界最佳实践：远程控制应该更频繁发送全帧以减少延迟
        // 从每60帧改为每30帧发送一次全帧（从2秒改为1秒）
        let force_full_frame = self.frame_count % 30 == 1 || self.last_frame.is_none();
        
        if force_full_frame {
            // 发送完整的JPEG帧 - 大幅降低质量以减少数据量（参考Multi经验）
            let jpeg_data = self.encode_jpeg(&rgba_data, width, height, 35)?;  // 从60降到35
            let base64_data = general_purpose::STANDARD.encode(&jpeg_data);
            
            self.last_frame = Some(current_frame);
            
            Ok((base64_data, width, height, "jpeg".to_string(), true, None))
        } else {
            // 检测变化并发送差分数据
            if let Some(ref last_frame) = self.last_frame {
                let changed_regions = self.detect_changes(last_frame, &current_frame)?;
                
                if changed_regions.is_empty() {
                    // 没有变化，返回空数据
                    Ok(("".to_string(), width, height, "diff".to_string(), false, Some(vec![])))
                } else {
                    // 有变化，发送差分数据
                    self.last_frame = Some(current_frame);
                    Ok(("".to_string(), width, height, "diff".to_string(), false, Some(changed_regions)))
                }
            } else {
                // 没有上一帧，发送完整帧
                let jpeg_data = self.encode_jpeg(&rgba_data, width, height, 35)?;  // 从60降到35
                let base64_data = general_purpose::STANDARD.encode(&jpeg_data);
                
                self.last_frame = Some(current_frame);
                
                Ok((base64_data, width, height, "jpeg".to_string(), true, None))
            }
        }
    }
    
    /// 编码为JPEG格式 - 优化版本，直接处理BGRA
    fn encode_jpeg(&self, bgra_data: &[u8], width: u32, height: u32, quality: u8) -> Result<Vec<u8>> {
        // 将BGRA转换为RGB (去除alpha通道，同时转换颜色顺序)
        let mut rgb_data = Vec::with_capacity(bgra_data.len() * 3 / 4);
        for chunk in bgra_data.chunks(4) {
            if chunk.len() >= 4 {
                rgb_data.push(chunk[2]); // R (from B)
                rgb_data.push(chunk[1]); // G
                rgb_data.push(chunk[0]); // B (from R)
            }
        }
        
        let image = image::RgbImage::from_raw(width, height, rgb_data)
            .ok_or_else(|| anyhow::anyhow!("创建RGB图像失败"))?;
        
        let mut jpeg_buffer = Vec::new();
        {
            use image::ImageEncoder;
            let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_buffer, quality);
            encoder.write_image(&image, width, height, image::ExtendedColorType::Rgb8)?;
        }
        
        Ok(jpeg_buffer)
    }
    
    /// 计算目标分辨率 - 根据业界最佳实践
    fn calculate_target_resolution(width: u32, height: u32) -> (u32, u32) {
        // 调整为720p以进一步减少数据量和延迟
        const MAX_WIDTH: u32 = 1280;   // 720p宽度
        const MAX_HEIGHT: u32 = 720;   // 720p高度
        const MAX_PIXELS: u32 = MAX_WIDTH * MAX_HEIGHT; // 约0.9M像素
        
        let total_pixels = width * height;
        
        // 如果像素数超过上限，按比例缩放
        if total_pixels > MAX_PIXELS {
            let scale_factor = (MAX_PIXELS as f64 / total_pixels as f64).sqrt();
            let new_width = (width as f64 * scale_factor) as u32;
            let new_height = (height as f64 * scale_factor) as u32;
            log::debug!("📏 自动缩放: {}*{}像素 -> {}*{}像素 (缩放比例: {:.2})", width, height, new_width, new_height, scale_factor);
            (new_width, new_height)
        } else {
            (width, height)
        }
    }
    
    /// 检测帧之间的变化区域
    fn detect_changes(&self, last_frame: &ScreenFrame, current_frame: &ScreenFrame) -> Result<Vec<ChangedRegion>> {
        if last_frame.width != current_frame.width || last_frame.height != current_frame.height {
            return Err(anyhow::anyhow!("帧尺寸不匹配"));
        }
        
        const BLOCK_SIZE: u32 = 64; // 恢复到64x64，因为现在在缩放后的分辨率上工作
        const THRESHOLD: u32 = 10; // 适当提高阈值，减少过于敏感的检测
        
        let mut changed_regions = Vec::new();
        let width = current_frame.width;
        let height = current_frame.height;
        
        // 按块检测变化
        for block_y in (0..height).step_by(BLOCK_SIZE as usize) {
            for block_x in (0..width).step_by(BLOCK_SIZE as usize) {
                let block_width = std::cmp::min(BLOCK_SIZE, width - block_x);
                let block_height = std::cmp::min(BLOCK_SIZE, height - block_y);
                
                if self.block_has_changed(
                    &last_frame.data, 
                    &current_frame.data, 
                    width, 
                    block_x, 
                    block_y, 
                    block_width, 
                    block_height, 
                    THRESHOLD
                )? {
                    // 提取变化的块数据
                    let block_data = self.extract_block_data(
                        &current_frame.data, 
                        width, 
                        block_x, 
                        block_y, 
                        block_width, 
                        block_height
                    )?;
                    
                    // 压缩块数据为JPEG - 对差分块使用更低质量以减少数据量
                    let jpeg_data = self.encode_jpeg(&block_data, block_width, block_height, 45)?;  // 从70降到45
                    let base64_data = general_purpose::STANDARD.encode(&jpeg_data);
                    
                    changed_regions.push(ChangedRegion {
                        x: block_x,
                        y: block_y,
                        width: block_width,
                        height: block_height,
                        data: base64_data,
                    });
                }
            }
        }
        
        Ok(changed_regions)
    }
    
    /// 检查块是否有变化
    fn block_has_changed(
        &self,
        last_data: &[u8],
        current_data: &[u8],
        width: u32,
        block_x: u32,
        block_y: u32,
        block_width: u32,
        block_height: u32,
        threshold: u32,
    ) -> Result<bool> {
        let mut changed_pixels = 0;
        let total_pixels = block_width * block_height;
        
        for y in 0..block_height {
            for x in 0..block_width {
                let pixel_x = block_x + x;
                let pixel_y = block_y + y;
                let index = ((pixel_y * width + pixel_x) * 4) as usize;
                
                if index + 3 < last_data.len() && index + 3 < current_data.len() {
                    // 比较RGB值（忽略Alpha通道）
                    let r_diff = (last_data[index] as i32 - current_data[index] as i32).abs() as u32;
                    let g_diff = (last_data[index + 1] as i32 - current_data[index + 1] as i32).abs() as u32;
                    let b_diff = (last_data[index + 2] as i32 - current_data[index + 2] as i32).abs() as u32;
                    
                    if r_diff > threshold || g_diff > threshold || b_diff > threshold {
                        changed_pixels += 1;
                    }
                }
            }
        }
        
        // 调整像素变化比例要求：平衡敏感度和性能
        // 从1%调整到2%，减少过于频繁的块更新
        Ok(changed_pixels > total_pixels / 50)  // 2% = 1/50
    }
    
    /// 提取块数据
    fn extract_block_data(
        &self,
        data: &[u8],
        width: u32,
        block_x: u32,
        block_y: u32,
        block_width: u32,
        block_height: u32,
    ) -> Result<Vec<u8>> {
        let mut block_data = Vec::with_capacity((block_width * block_height * 4) as usize);
        
        for y in 0..block_height {
            for x in 0..block_width {
                let pixel_x = block_x + x;
                let pixel_y = block_y + y;
                let index = ((pixel_y * width + pixel_x) * 4) as usize;
                
                if index + 3 < data.len() {
                    block_data.push(data[index]);     // R
                    block_data.push(data[index + 1]); // G
                    block_data.push(data[index + 2]); // B
                    block_data.push(data[index + 3]); // A
                } else {
                    // 填充黑色像素
                    block_data.extend_from_slice(&[0, 0, 0, 255]);
                }
            }
        }
        
        Ok(block_data)
    }
    
    /// 获取主显示器尺寸
    #[allow(unused)]
    pub fn get_primary_display_size() -> Result<(u32, u32)> {
        let display = Display::primary()?;
        let capturer = Capturer::new(display)?;
        Ok((capturer.width() as u32, capturer.height() as u32))
    }

    /// 根据需要缩放帧数据 - 业界顶尖优化版本
    fn scale_frame_if_needed(&mut self, width: u32, height: u32, rgba_data: Vec<u8>) -> Result<(u32, u32, Vec<u8>)> {
        let (target_width, target_height) = Self::calculate_target_resolution(width, height);
        
        // 如果不需要缩放，直接返回
        if width == target_width && height == target_height {
            return Ok((width, height, rgba_data));
        }
        
        // 需要缩放，仅在首次缩放时打印信息
        if self.frame_count == 0 {
            log::debug!("📏 自动缩放帧: {}x{} -> {}x{} (减少 {:.1}% 像素)", 
                width, height, target_width, target_height,
                (1.0 - (target_width * target_height) as f64 / (width * height) as f64) * 100.0);
            log::debug!("🚀 使用业界顶尖的并行批处理算法");
        }
        
        // 计算缩放参数
        let x_step = width / target_width;
        let y_step = height / target_height;
        let target_size = (target_width * target_height * 4) as usize;
        
        // 使用Rayon并行处理，每个核心处理不同的行
        let rgba_data_ref = &rgba_data;
        
        // 业界最佳实践：将工作分块到多个CPU核心
        let num_cores = num_cpus::get() as u32;
        let rows_per_chunk = (target_height / num_cores).max(1);
        let chunks: Vec<_> = (0..target_height)
            .step_by(rows_per_chunk as usize)
            .map(|start_y| {
                let end_y = (start_y + rows_per_chunk).min(target_height);
                (start_y, end_y)
            })
            .collect();
        
        // 将数据分割成独立的块来避免共享可变状态
        let mut output_chunks: Vec<Vec<u8>> = chunks.into_par_iter().map(|(start_y, end_y)| {
            let mut chunk_data = vec![0u8; (end_y - start_y) as usize * target_width as usize * 4];
            
            for y in start_y..end_y {
                let src_y = (y * y_step).min(height - 1);
                let src_row_offset = (src_y as usize) * (width * 4) as usize;
                let dst_row_start = ((y - start_y) * target_width * 4) as usize;
                
                // 安全的行处理 - 避免unsafe指针操作
                for x in 0..target_width {
                    let src_x = (x * x_step).min(width - 1);
                    let src_idx = src_row_offset + (src_x as usize * 4);
                    let dst_idx = dst_row_start + (x as usize * 4);
                    
                    // 使用安全的边界检查复制
                    if src_idx + 3 < rgba_data_ref.len() && dst_idx + 3 < chunk_data.len() {
                        chunk_data[dst_idx] = rgba_data_ref[src_idx];
                        chunk_data[dst_idx + 1] = rgba_data_ref[src_idx + 1];
                        chunk_data[dst_idx + 2] = rgba_data_ref[src_idx + 2];
                        chunk_data[dst_idx + 3] = rgba_data_ref[src_idx + 3];
                    }
                }
            }
            chunk_data
        }).collect();
        
        // 合并所有块
        let mut resized_data = Vec::with_capacity(target_size);
        for chunk in output_chunks.drain(..) {
            resized_data.extend(chunk);
        }
        
        Ok((target_width, target_height, resized_data))
    }
}
