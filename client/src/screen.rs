use anyhow::Result;
use base64::{Engine as _, engine::general_purpose};
use scrap::{Capturer, Display};
use std::io::ErrorKind::WouldBlock;
use std::thread;
use std::time::Duration;

/// 屏幕捕获功能 - 采用RustDesk架构：无状态函数，返回数据
pub struct ScreenCaptureService;

impl ScreenCaptureService {
    /// 创建一个新的屏幕捕获器实例（在调用线程中）
    pub fn create_capturer() -> Result<Capturer> {
        let display = Display::primary()?;
        let capturer = Capturer::new(display)?;
        Ok(capturer)
    }
    
    /// 捕获屏幕并返回base64编码的PNG数据
    /// 这个函数在捕获线程中调用，接收 &mut Capturer
    pub fn capture_screen(capturer: &mut Capturer) -> Result<(String, u32, u32)> {
        let width = capturer.width();
        let height = capturer.height();
        
        // 尝试捕获帧，可能需要重试几次
        let buffer = loop {
            match capturer.frame() {
                Ok(buffer) => break buffer,
                Err(error) => {
                    if error.kind() == WouldBlock {
                        // 帧还没准备好，等待一下
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