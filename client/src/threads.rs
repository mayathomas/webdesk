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
    println!("📷 屏幕捕获线程已启动 (RustDesk架构 + JPEG压缩 + 差分编码)");
    
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
                    println!("📷 屏幕捕获开始 (优化模式)");
                    active = true;
                }
                ThreadControlSignal::Stop => {
                    println!("📷 屏幕捕获停止");
                    break;
                }
            }
        }
        
        // 如果激活且达到捕获间隔
        if active && last_capture.elapsed() >= capture_interval {
            match screen_service.capture_screen_optimized(&mut capturer) {
                Ok((image_data, width, height, format, full_frame, changed_regions)) => {
                    // 保存用于日志的值
                    let log_format = format.clone();
                    let log_regions_count = changed_regions.as_ref().map(|r| r.len()).unwrap_or(0);
                    
                    let screen_data = WebSocketMessage::ScreenData(ScreenData {
                        image_data,
                        width,
                        height,
                        format,
                        full_frame,
                        changed_regions,
                    });
                    
                    if screen_tx.send(screen_data).is_err() {
                        println!("❌ 发送屏幕数据到主线程失败，主线程可能已断开");
                        break;
                    }
                    
                    // 详细日志输出
                    if full_frame {
                        println!("📷 发送完整帧: {}x{} ({})", width, height, log_format);
                    } else if log_regions_count > 0 {
                        println!("📷 发送差分数据: {} 个变化区域", log_regions_count);
                    } else {
                        // 每10次无变化才打印一次，避免日志垃圾
                        let mut no_change_count: u32 = 0;
                        no_change_count += 1;
                        if no_change_count % 10 == 0 {
                            println!("📷 连续{}次无屏幕变化", no_change_count);
                        }
                    }
                    
                    last_capture = std::time::Instant::now();
                }
                Err(e) => {
                    eprintln!("📷 屏幕捕获失败: {}", e);
                }
            }
        }
        
        // 进一步减少休眠时间，提高响应速度
        std::thread::sleep(Duration::from_millis(2)); // 从5ms减少到2ms，更快的控制循环
    }
    
    println!("📷 屏幕捕获线程已停止");
    Ok(())
}

/// RustDesk架构：输入事件处理线程（异步线程）
pub async fn input_event_thread(
    input_controller: Arc<InputController>,
    mut input_rx: tokio::sync::mpsc::UnboundedReceiver<WebSocketMessage>,
    mut control_rx: tokio::sync::mpsc::UnboundedReceiver<ThreadControlSignal>,
) {
    println!("🎮 输入事件处理线程已启动 (RustDesk架构)");
    
    let mut active = false;
    
    loop {
        tokio::select! {
            // 检查控制信号
            Some(signal) = control_rx.recv() => {
                match signal {
                    ThreadControlSignal::Start => {
                        println!("🎮 输入事件处理开始");
                        active = true;
                    }
                    ThreadControlSignal::Stop => {
                        println!("🎮 输入事件处理停止");
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
    
    println!("🎮 输入事件处理线程已停止");
}

/// 处理鼠标事件
fn handle_mouse_event(mouse_event: MouseEvent, input_controller: &InputController) {
    match mouse_event.event_type.as_str() {
        "click" => {
            if let Err(e) =
                input_controller.click_mouse(mouse_event.x, mouse_event.y, &mouse_event.button)
            {
                eprintln!("❌ 鼠标点击失败: {}", e);
            }
        }
        "move" => {
            if let Err(e) = input_controller.move_mouse(mouse_event.x, mouse_event.y) {
                eprintln!("❌ 鼠标移动失败: {}", e);
            }
        }
        "scroll" => {
            if let Some(delta) = mouse_event.scroll_delta {
                if let Err(e) = input_controller.scroll_mouse(mouse_event.x, mouse_event.y, delta) {
                    eprintln!("❌ 鼠标滚轮失败: {}", e);
                }
            }
        }
        _ => {}
    }
}

/// 处理键盘事件
fn handle_keyboard_event(keyboard_event: KeyboardEvent, input_controller: &InputController) {
    match keyboard_event.event_type.as_str() {
        "press" => {
            if let Err(e) = input_controller.press_key(&keyboard_event.key) {
                eprintln!("❌ 按键按下失败: {}", e);
            }
        }
        "release" => {
            if let Err(e) = input_controller.release_key(&keyboard_event.key) {
                eprintln!("❌ 按键释放失败: {}", e);
            }
        }
        _ => {}
    }
} 