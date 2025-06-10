use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;

use crate::types::*;
use crate::input::InputController;
use crate::screen::ScreenCaptureService;

/// RustDesk架构：屏幕捕获线程（阻塞线程）
pub fn screen_capture_thread(
    mut control_rx: tokio::sync::mpsc::UnboundedReceiver<ThreadControlSignal>,
    screen_tx: tokio::sync::mpsc::UnboundedSender<WebSocketMessage>,
) -> Result<()> {
    log::debug!("📷 屏幕捕获线程已启动 (RustDesk架构 + JPEG压缩 + 差分编码)");
    
    let mut capturer = ScreenCaptureService::create_capturer()?;
    let mut screen_service = ScreenCaptureService::new();
    let mut active = false;
    let mut last_capture = std::time::Instant::now();
    
    // 根据业界最佳实践：远程控制需要更高的帧率以减少延迟
    // 进一步提升到 25FPS (40ms)，接近30FPS的理想值
    let capture_interval = Duration::from_millis(40); // 25 FPS，更流畅的体验
    
    loop {
        // 检查控制信号（非阻塞）
        if let Ok(signal) = control_rx.try_recv() {
            match signal {
                ThreadControlSignal::Start => {
                    log::debug!("📷 屏幕捕获开始 (优化模式)");
                    active = true;
                }
                ThreadControlSignal::Stop => {
                    log::debug!("📷 屏幕捕获停止");
                    break;
                }
            }
        }
        
        // 如果激活且达到捕获间隔
        if active && last_capture.elapsed() >= capture_interval {
            match screen_service.capture_screen_optimized(&mut capturer) {
                Ok((image_data, width, height, original_width, original_height, format, full_frame, changed_regions)) => {
                    // 保存用于日志的值
                    let log_format = format.clone();
                    let log_regions_count = changed_regions.as_ref().map(|r| r.len()).unwrap_or(0);
                    
                    let screen_data = WebSocketMessage::ScreenData(ScreenData {
                        image_data,
                        width,
                        height,
                        original_width,
                        original_height,
                        format,
                        full_frame,
                        changed_regions,
                    });
                    
                    if screen_tx.send(screen_data).is_err() {
                        log::debug!("❌ 发送屏幕数据到主线程失败，主线程可能已断开");
                        break;
                    }
                    
                    // 详细日志输出
                    if full_frame {
                        log::debug!("📷 发送完整帧: {}x{} ({})", width, height, log_format);
                    } else if log_regions_count > 0 {
                        log::debug!("📷 发送差分数据: {} 个变化区域", log_regions_count);
                    } else {
                        // 每10次无变化才打印一次，避免日志垃圾
                        let mut no_change_count: u32 = 0;
                        no_change_count += 1;
                        if no_change_count % 10 == 0 {
                            log::debug!("📷 连续{}次无屏幕变化", no_change_count);
                        }
                    }
                    
                    last_capture = std::time::Instant::now();
                }
                Err(e) => {
                    log::error!("📷 屏幕捕获失败: {}", e);
                }
            }
        }
        
        // 进一步减少休眠时间，提高响应速度
        std::thread::sleep(Duration::from_millis(2)); // 从5ms减少到2ms，更快的控制循环
    }
    
    log::debug!("📷 屏幕捕获线程已停止");
    Ok(())
}

/// RustDesk架构：输入事件处理线程（异步线程）
pub async fn input_event_thread(
    input_controller: Arc<InputController>,
    mut input_rx: tokio::sync::mpsc::UnboundedReceiver<WebSocketMessage>,
    mut control_rx: tokio::sync::mpsc::UnboundedReceiver<ThreadControlSignal>,
) {
    log::debug!("🎮 输入事件处理线程已启动 (RustDesk架构)");
    
    let mut active = false;
    
    loop {
        tokio::select! {
            // 检查控制信号
            Some(signal) = control_rx.recv() => {
                match signal {
                    ThreadControlSignal::Start => {
                        log::debug!("🎮 输入事件处理开始");
                        active = true;
                    }
                    ThreadControlSignal::Stop => {
                        log::debug!("🎮 输入事件处理停止");
                        break;
                    }
                }
            }
            
            // 处理输入事件
            Some(input_msg) = input_rx.recv() => {
                if !active {
                    continue;
                }
                
                match input_msg {
                    WebSocketMessage::MouseEvent(mouse_event) => {
                        handle_mouse_event(mouse_event, &input_controller);
                    }
                    WebSocketMessage::KeyboardEvent(keyboard_event) => {
                        handle_keyboard_event(keyboard_event, &input_controller);
                    }
                    _ => {}
                }
            }
        }
    }
    
    log::debug!("🎮 输入事件处理线程已停止");
}

/// 处理鼠标事件 - 按照VNC/RDP标准，只处理原始鼠标事件
pub fn handle_mouse_event(mouse_event: MouseEvent, input_controller: &InputController) {
    match mouse_event.event_type.as_str() {
        "press" => {
            log::debug!("🖱️ 处理鼠标按下: 按钮={}, 坐标=({}, {})", 
                mouse_event.button, mouse_event.x, mouse_event.y);
            if let Err(e) = input_controller.press_mouse_button(mouse_event.x, mouse_event.y, &mouse_event.button) {
                log::error!("❌ 鼠标按下失败: {}", e);
            }
        }
        "release" => {
            log::debug!("🖱️ 处理鼠标释放: 按钮={}, 坐标=({}, {})", 
                mouse_event.button, mouse_event.x, mouse_event.y);
            if let Err(e) = input_controller.release_mouse_button(mouse_event.x, mouse_event.y, &mouse_event.button) {
                log::error!("❌ 鼠标释放失败: {}", e);
            }
        }
        "move" => {
            if let Err(e) = input_controller.move_mouse(mouse_event.x, mouse_event.y) {
                log::error!("❌ 鼠标移动失败: {}", e);
            }
        }
        "scroll" => {
            if let Some(delta) = mouse_event.scroll_delta {
                log::debug!("🎡 处理鼠标滚轮: 方向={}, 坐标=({}, {})", 
                    delta, mouse_event.x, mouse_event.y);
                if let Err(e) = input_controller.scroll_mouse(mouse_event.x, mouse_event.y, delta) {
                    log::error!("❌ 鼠标滚轮失败: {}", e);
                }
            } else {
                log::warn!("⚠️ 滚轮事件缺少scroll_delta字段");
            }
        }
        unknown => {
            log::warn!("⚠️ 未知的鼠标事件类型: {}", unknown);
        }
    }
}

/// 处理键盘事件
pub fn handle_keyboard_event(keyboard_event: KeyboardEvent, input_controller: &InputController) {
    match keyboard_event.event_type.as_str() {
        "press" => {
            if let Err(e) = input_controller.press_key(&keyboard_event.key) {
                log::error!("❌ 按键按下失败: {}", e);
            }
        }
        "release" => {
            if let Err(e) = input_controller.release_key(&keyboard_event.key) {
                log::error!("❌ 按键释放失败: {}", e);
            }
        }
        _ => {}
    }
}