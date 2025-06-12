use anyhow::{Result, anyhow};
use log::{debug, error, info};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

#[cfg(target_os = "windows")]
use self::screen_capture_windows::WindowsScreenCapture;
#[cfg(target_os = "linux")]
use crate::screen_capture_linux::LinuxScreenCapture;
#[cfg(target_os = "macos")]
use crate::screen_capture_macos::MacOSScreenCapture;

/// H.264视频流屏幕捕获服务
/// 专门为H.264编码器优化的屏幕捕获实现
pub struct VideoScreenCapture {
    /// 平台特定的屏幕捕获实现
    #[cfg(target_os = "windows")]
    capturer: Arc<Mutex<WindowsScreenCapture>>,
    #[cfg(target_os = "macos")]
    capturer: Arc<Mutex<MacOSScreenCapture>>,
    #[cfg(target_os = "linux")]
    capturer: Arc<Mutex<LinuxScreenCapture>>,

    /// 捕获配置
    config: CaptureConfig,
    /// 性能统计
    stats: Arc<Mutex<CaptureStats>>,
}

/// 屏幕捕获配置
#[derive(Debug, Clone)]
pub struct CaptureConfig {
    /// 目标分辨率
    pub target_width: u32,
    pub target_height: u32,
    /// 最大帧率
    pub max_fps: f32,
}

/// 捕获的视频帧
#[derive(Debug, Clone)]
pub struct VideoFrame {
    /// RGBA像素数据
    pub data: Vec<u8>,
}

/// 捕获统计信息
#[derive(Debug, Clone, Default)]
pub struct CaptureStats {
    pub dropped_frames: u64,
    pub capture_errors: u64,
}

impl VideoScreenCapture {
    /// 创建新的视频屏幕捕获服务
    pub async fn new(config: CaptureConfig) -> Result<Self> {
        info!(
            "🎥 初始化视频屏幕捕获: {}x{}@{:.1}fps",
            config.target_width, config.target_height, config.max_fps
        );

        // 创建平台特定的捕获器
        #[cfg(target_os = "windows")]
        let capturer = Arc::new(Mutex::new(WindowsScreenCapture::new(&config).await?));

        #[cfg(target_os = "macos")]
        let capturer = Arc::new(Mutex::new(MacOSScreenCapture::new(&config).await?));

        #[cfg(target_os = "linux")]
        let capturer = Arc::new(Mutex::new(LinuxScreenCapture::new(&config).await?));

        Ok(Self {
            capturer,
            config,
            stats: Arc::new(Mutex::new(CaptureStats::default())),
        })
    }

    /// 启动连续捕获流
    pub async fn start_capture_stream(
        &self,
    ) -> Result<tokio::sync::mpsc::UnboundedReceiver<VideoFrame>> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

        #[cfg(target_os = "windows")]
        let capturer: Arc<Mutex<WindowsScreenCapture>> = Arc::clone(&self.capturer);
        #[cfg(target_os = "macos")]
        let capturer: Arc<Mutex<MacOSScreenCapture>> = Arc::clone(&self.capturer);
        #[cfg(target_os = "linux")]
        let capturer: Arc<Mutex<LinuxScreenCapture>> = Arc::clone(&self.capturer);

        let config = self.config.clone();
        let stats = Arc::clone(&self.stats);

        // 启动捕获任务
        tokio::spawn(async move {
            let frame_interval = Duration::from_secs_f32(1.0 / config.max_fps);
            let mut last_capture = Instant::now();

            loop {
                let now = Instant::now();
                let time_since_last = now.duration_since(last_capture);

                // 控制帧率
                if time_since_last < frame_interval {
                    tokio::time::sleep(frame_interval - time_since_last).await;
                    continue;
                }

                // 创建临时capture实例进行捕获
                let capture_result = {
                    let mut cap = capturer.lock().await;
                    cap.capture_frame().await
                };

                match capture_result {
                    Ok(raw_frame) => {
                        let frame = VideoFrame {
                            data: raw_frame.data,
                        };

                        if tx.send(frame).is_err() {
                            debug!("🛑 捕获流接收者已断开连接");
                            break;
                        }
                    }
                    Err(e) => {
                        error!("❌ 捕获帧失败: {}", e);
                        let mut s = stats.lock().await;
                        s.capture_errors += 1;

                        // 短暂等待后重试
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }

                last_capture = now;
            }
        });

        info!("✅ 视频捕获流已启动，目标FPS: {:.1}", config.max_fps);
        Ok(rx)
    }

    /// 获取统计信息
    pub async fn get_stats(&self) -> CaptureStats {
        self.stats.lock().await.clone()
    }
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            target_width: 1920,
            target_height: 1080,
            max_fps: 30.0,
        }
    }
}

// 平台特定的屏幕捕获trait
#[async_trait::async_trait]
pub trait PlatformScreenCapture {
    async fn new(config: &CaptureConfig) -> Result<Self>
    where
        Self: Sized;
    async fn capture_frame(&mut self) -> Result<RawVideoFrame>;
}

/// 原始视频帧 (平台特定格式)
#[derive(Debug, Clone)]
pub struct RawVideoFrame {
    pub data: Vec<u8>,
}

// 由于各平台实现较为复杂，这里先提供接口定义
// 具体实现将在各平台专用文件中完成

#[cfg(target_os = "windows")]
mod screen_capture_windows {
    use super::*;
    use std::mem;
    use std::ptr;
    use winapi::ctypes::c_void;
    use winapi::um::wingdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC,
        DIB_RGB_COLORS, GetDIBits, SRCCOPY, SelectObject,
    };
    use winapi::um::winuser::{GetDC, GetSystemMetrics, ReleaseDC, SM_CXSCREEN, SM_CYSCREEN};

    pub struct WindowsScreenCapture {
        screen_width: u32,
        screen_height: u32,
    }

    #[async_trait::async_trait]
    impl PlatformScreenCapture for WindowsScreenCapture {
        async fn new(_config: &CaptureConfig) -> Result<Self> {
            unsafe {
                let screen_width = GetSystemMetrics(SM_CXSCREEN) as u32;
                let screen_height = GetSystemMetrics(SM_CYSCREEN) as u32;

                log::info!("🖥️ Windows屏幕尺寸: {}x{}", screen_width, screen_height);

                Ok(Self {
                    screen_width,
                    screen_height,
                })
            }
        }

        async fn capture_frame(&mut self) -> Result<RawVideoFrame> {
            unsafe {
                // 获取屏幕DC
                let screen_dc = GetDC(ptr::null_mut());
                if screen_dc.is_null() {
                    return Err(anyhow!("无法获取屏幕DC"));
                }

                // 创建兼容DC
                let mem_dc = CreateCompatibleDC(screen_dc);
                if mem_dc.is_null() {
                    ReleaseDC(ptr::null_mut(), screen_dc);
                    return Err(anyhow!("无法创建兼容DC"));
                }

                // 创建兼容位图
                let bitmap = CreateCompatibleBitmap(
                    screen_dc,
                    self.screen_width as i32,
                    self.screen_height as i32,
                );
                if bitmap.is_null() {
                    winapi::um::wingdi::DeleteDC(mem_dc);
                    ReleaseDC(ptr::null_mut(), screen_dc);
                    return Err(anyhow!("无法创建兼容位图"));
                }

                // 选择位图到DC
                let old_bitmap = SelectObject(mem_dc, bitmap as *mut c_void);

                // 执行位图传输
                let result = BitBlt(
                    mem_dc,
                    0,
                    0,
                    self.screen_width as i32,
                    self.screen_height as i32,
                    screen_dc,
                    0,
                    0,
                    SRCCOPY,
                );

                if result == 0 {
                    SelectObject(mem_dc, old_bitmap);
                    winapi::um::wingdi::DeleteObject(bitmap as *mut c_void);
                    winapi::um::wingdi::DeleteDC(mem_dc);
                    ReleaseDC(ptr::null_mut(), screen_dc);
                    return Err(anyhow!("BitBlt失败"));
                }

                // 准备BITMAPINFO结构
                let mut bitmap_info: BITMAPINFO = mem::zeroed();
                bitmap_info.bmiHeader.biSize = mem::size_of::<BITMAPINFOHEADER>() as u32;
                bitmap_info.bmiHeader.biWidth = self.screen_width as i32;
                bitmap_info.bmiHeader.biHeight = -(self.screen_height as i32); // 负值表示从上到下
                bitmap_info.bmiHeader.biPlanes = 1;
                bitmap_info.bmiHeader.biBitCount = 32; // BGRA
                bitmap_info.bmiHeader.biCompression = BI_RGB;

                // 分配像素数据缓冲区
                let pixel_count = (self.screen_width * self.screen_height) as usize;
                let mut pixel_data: Vec<u8> = vec![0; pixel_count * 4]; // BGRA格式

                // 获取位图数据
                let lines_copied = GetDIBits(
                    mem_dc,
                    bitmap,
                    0,
                    self.screen_height,
                    pixel_data.as_mut_ptr() as *mut c_void,
                    &mut bitmap_info,
                    DIB_RGB_COLORS,
                );

                // 清理资源
                SelectObject(mem_dc, old_bitmap);
                winapi::um::wingdi::DeleteObject(bitmap as *mut c_void);
                winapi::um::wingdi::DeleteDC(mem_dc);
                ReleaseDC(ptr::null_mut(), screen_dc);

                if lines_copied == 0 {
                    return Err(anyhow!("GetDIBits失败"));
                }

                // Windows GDI返回的是BGRA格式，需要转换为RGBA
                let mut rgba_data = Vec::with_capacity(pixel_data.len());
                for chunk in pixel_data.chunks(4) {
                    if chunk.len() == 4 {
                        // BGRA -> RGBA
                        rgba_data.push(chunk[2]); // R
                        rgba_data.push(chunk[1]); // G  
                        rgba_data.push(chunk[0]); // B
                        rgba_data.push(chunk[3]); // A
                    }
                }

                log::debug!(
                    "📷 成功捕获{}x{}屏幕帧 ({} bytes)",
                    self.screen_width,
                    self.screen_height,
                    rgba_data.len()
                );

                Ok(RawVideoFrame { data: rgba_data })
            }
        }
    }
}

#[cfg(target_os = "macos")]
mod screen_capture_macos {
    use super::*;

    pub struct MacOSScreenCapture {
        config: CaptureConfig,
    }

    #[async_trait::async_trait]
    impl PlatformScreenCapture for MacOSScreenCapture {
        async fn new(config: &CaptureConfig) -> Result<Self> {
            Ok(Self {
                config: config.clone(),
            })
        }

        async fn capture_frame(&mut self) -> Result<RawVideoFrame> {
            // TODO: 实现macOS Screen Capture Kit
            Err(anyhow!("macOS屏幕捕获未实现"))
        }

        async fn update_config(&mut self, config: &CaptureConfig) -> Result<()> {
            self.config = config.clone();
            Ok(())
        }
    }
}

#[cfg(target_os = "linux")]
mod screen_capture_linux {
    use super::*;

    pub struct LinuxScreenCapture {
        config: CaptureConfig,
    }

    #[async_trait::async_trait]
    impl PlatformScreenCapture for LinuxScreenCapture {
        async fn new(config: &CaptureConfig) -> Result<Self> {
            Ok(Self {
                config: config.clone(),
            })
        }

        async fn capture_frame(&mut self) -> Result<RawVideoFrame> {
            // TODO: 实现Linux X11/Wayland屏幕捕获
            Err(anyhow!("Linux屏幕捕获未实现"))
        }

        async fn update_config(&mut self, config: &CaptureConfig) -> Result<()> {
            self.config = config.clone();
            Ok(())
        }
    }
}
