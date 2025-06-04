use anyhow::Result;
use base64::{Engine as _, engine::general_purpose};
use scrap::{Capturer, Display};
use std::io::ErrorKind::WouldBlock;
use std::thread;
use std::time::Duration;
use serde::{Serialize, Deserialize};

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
        let width = capturer.width() as u32;
        let height = capturer.height() as u32;
        
        // 捕获原始帧数据
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

        // 将BGRA格式转换为RGBA
        let mut rgba_data = Vec::with_capacity(buffer.len());
        for chunk in buffer.chunks(4) {
            if chunk.len() == 4 {
                rgba_data.push(chunk[2]); // R
                rgba_data.push(chunk[1]); // G
                rgba_data.push(chunk[0]); // B
                rgba_data.push(chunk[3]); // A
            }
        }

        let current_frame = ScreenFrame {
            data: rgba_data.clone(),
            width,
            height,
        };

        self.frame_count += 1;
        
        // 每30帧强制发送一次完整帧，或者第一帧
        let force_full_frame = self.frame_count % 30 == 1 || self.last_frame.is_none();
        
        if force_full_frame {
            // 发送完整的JPEG帧
            let jpeg_data = self.encode_jpeg(&rgba_data, width, height, 75)?;
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
                let jpeg_data = self.encode_jpeg(&rgba_data, width, height, 75)?;
                let base64_data = general_purpose::STANDARD.encode(&jpeg_data);
                
                self.last_frame = Some(current_frame);
                
                Ok((base64_data, width, height, "jpeg".to_string(), true, None))
            }
        }
    }
    
    /// 编码为JPEG格式
    fn encode_jpeg(&self, rgba_data: &[u8], width: u32, height: u32, quality: u8) -> Result<Vec<u8>> {
        // 将RGBA转换为RGB (去除alpha通道)
        let mut rgb_data = Vec::with_capacity(rgba_data.len() * 3 / 4);
        for chunk in rgba_data.chunks(4) {
            if chunk.len() >= 3 {
                rgb_data.push(chunk[0]); // R
                rgb_data.push(chunk[1]); // G
                rgb_data.push(chunk[2]); // B
            }
        }
        
        let image = image::RgbImage::from_raw(width, height, rgb_data)
            .ok_or_else(|| anyhow::anyhow!("创建RGB图像失败"))?;
        
        let mut jpeg_buffer = Vec::new();
        {
            use image::ImageEncoder;
            let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_buffer, quality);
            encoder.write_image(&image, width, height, image::ColorType::Rgb8)?;
        }
        
        Ok(jpeg_buffer)
    }
    
    /// 检测帧之间的变化区域
    fn detect_changes(&self, last_frame: &ScreenFrame, current_frame: &ScreenFrame) -> Result<Vec<ChangedRegion>> {
        if last_frame.width != current_frame.width || last_frame.height != current_frame.height {
            return Err(anyhow::anyhow!("帧尺寸不匹配"));
        }
        
        const BLOCK_SIZE: u32 = 64; // 64x64像素块
        const THRESHOLD: u32 = 10; // 变化阈值（每个通道）
        
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
                    
                    // 压缩块数据为JPEG
                    let jpeg_data = self.encode_jpeg(&block_data, block_width, block_height, 80)?;
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
        
        // 如果超过5%的像素发生变化，认为这个块有变化
        Ok(changed_pixels > total_pixels / 20)
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
    
    /// 兼容性方法：原始PNG捕获
    pub fn capture_screen(capturer: &mut Capturer) -> Result<(String, u32, u32)> {
        let width = capturer.width();
        let height = capturer.height();
        
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

        // 将BGRA格式转换为RGBA
        let mut rgba_data = Vec::with_capacity(buffer.len());
        for chunk in buffer.chunks(4) {
            if chunk.len() == 4 {
                rgba_data.push(chunk[2]); // R
                rgba_data.push(chunk[1]); // G
                rgba_data.push(chunk[0]); // B
                rgba_data.push(chunk[3]); // A
            }
        }

        // 创建PNG图像
        let image = image::RgbaImage::from_raw(
            width as u32, 
            height as u32, 
            rgba_data
        ).ok_or_else(|| anyhow::anyhow!("创建图像失败"))?;

        // 转换为PNG格式的bytes
        let mut png_buffer = Vec::new();
        {
            use image::ImageEncoder;
            let encoder = image::codecs::png::PngEncoder::new(&mut png_buffer);
            encoder.write_image(
                &image, 
                width as u32, 
                height as u32, 
                image::ColorType::Rgba8
            )?;
        }

        // 编码为base64
        let base64_data = general_purpose::STANDARD.encode(&png_buffer);
        
        Ok((base64_data, width as u32, height as u32))
    }
    
    /// 获取主显示器尺寸
    #[allow(unused)]
    pub fn get_primary_display_size() -> Result<(u32, u32)> {
        let display = Display::primary()?;
        let capturer = Capturer::new(display)?;
        Ok((capturer.width() as u32, capturer.height() as u32))
    }
} 