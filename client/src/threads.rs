use std::sync::Arc;

use crate::types::*;
use crate::input::InputController;


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