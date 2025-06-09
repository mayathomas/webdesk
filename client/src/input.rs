use anyhow::Result;
use rdev::{simulate, Button, EventType, Key};
use std::thread;
use std::time::Duration;

pub struct InputController;

impl InputController {
    pub fn new() -> Self {
        Self
    }

    /// 模拟鼠标移动
    pub fn move_mouse(&self, x: f64, y: f64) -> Result<()> {
        let event_type = EventType::MouseMove { x, y };
        simulate(&event_type)?;
        Ok(())
    }

    /// 模拟鼠标点击
    pub fn click_mouse(&self, x: f64, y: f64, button: &str) -> Result<()> {
        // 先移动到指定位置
        self.move_mouse(x, y)?;
        
        // 稍微延迟一下
        thread::sleep(Duration::from_millis(10));
        
        let button = match button {
            "left" => Button::Left,
            "right" => Button::Right,
            "middle" => Button::Middle,
            _ => Button::Left,
        };

        // 按下鼠标
        simulate(&EventType::ButtonPress(button))?;
        thread::sleep(Duration::from_millis(10));
        
        // 释放鼠标
        simulate(&EventType::ButtonRelease(button))?;
        
        Ok(())
    }

    /// 模拟鼠标滚轮
    pub fn scroll_mouse(&self, x: f64, y: f64, delta: i32) -> Result<()> {
        self.move_mouse(x, y)?;
        
        let delta_x = 0i64;
        let delta_y = delta as i64;
        
        simulate(&EventType::Wheel { delta_x, delta_y })?;
        Ok(())
    }

    /// 模拟按键按下
    pub fn press_key(&self, key_code: &str) -> Result<()> {
        if let Some(key) = self.parse_key(key_code) {
            simulate(&EventType::KeyPress(key))?;
        }
        Ok(())
    }

    /// 模拟按键释放
    pub fn release_key(&self, key_code: &str) -> Result<()> {
        if let Some(key) = self.parse_key(key_code) {
            simulate(&EventType::KeyRelease(key))?;
        }
        Ok(())
    }

    /// 解析键码
    fn parse_key(&self, key_code: &str) -> Option<Key> {
        match key_code {
            // 字母键
            "KeyA" => Some(Key::KeyA),
            "KeyB" => Some(Key::KeyB),
            "KeyC" => Some(Key::KeyC),
            "KeyD" => Some(Key::KeyD),
            "KeyE" => Some(Key::KeyE),
            "KeyF" => Some(Key::KeyF),
            "KeyG" => Some(Key::KeyG),
            "KeyH" => Some(Key::KeyH),
            "KeyI" => Some(Key::KeyI),
            "KeyJ" => Some(Key::KeyJ),
            "KeyK" => Some(Key::KeyK),
            "KeyL" => Some(Key::KeyL),
            "KeyM" => Some(Key::KeyM),
            "KeyN" => Some(Key::KeyN),
            "KeyO" => Some(Key::KeyO),
            "KeyP" => Some(Key::KeyP),
            "KeyQ" => Some(Key::KeyQ),
            "KeyR" => Some(Key::KeyR),
            "KeyS" => Some(Key::KeyS),
            "KeyT" => Some(Key::KeyT),
            "KeyU" => Some(Key::KeyU),
            "KeyV" => Some(Key::KeyV),
            "KeyW" => Some(Key::KeyW),
            "KeyX" => Some(Key::KeyX),
            "KeyY" => Some(Key::KeyY),
            "KeyZ" => Some(Key::KeyZ),
            
            // 数字键
            "Digit0" => Some(Key::Num0),
            "Digit1" => Some(Key::Num1),
            "Digit2" => Some(Key::Num2),
            "Digit3" => Some(Key::Num3),
            "Digit4" => Some(Key::Num4),
            "Digit5" => Some(Key::Num5),
            "Digit6" => Some(Key::Num6),
            "Digit7" => Some(Key::Num7),
            "Digit8" => Some(Key::Num8),
            "Digit9" => Some(Key::Num9),
            
            // 功能键
            "F1" => Some(Key::F1),
            "F2" => Some(Key::F2),
            "F3" => Some(Key::F3),
            "F4" => Some(Key::F4),
            "F5" => Some(Key::F5),
            "F6" => Some(Key::F6),
            "F7" => Some(Key::F7),
            "F8" => Some(Key::F8),
            "F9" => Some(Key::F9),
            "F10" => Some(Key::F10),
            "F11" => Some(Key::F11),
            "F12" => Some(Key::F12),
            
            // 特殊键
            "Enter" => Some(Key::Return),
            "Space" => Some(Key::Space),
            "Backspace" => Some(Key::Backspace),
            "Tab" => Some(Key::Tab),
            "Escape" => Some(Key::Escape),
            "ShiftLeft" => Some(Key::ShiftLeft),
            "ShiftRight" => Some(Key::ShiftRight),
            "ControlLeft" => Some(Key::ControlLeft),
            "ControlRight" => Some(Key::ControlRight),
            "AltLeft" => Some(Key::Alt),
            "AltRight" => Some(Key::AltGr),
            "MetaLeft" => Some(Key::MetaLeft),
            "MetaRight" => Some(Key::MetaRight),
            
            // 方向键
            "ArrowUp" => Some(Key::UpArrow),
            "ArrowDown" => Some(Key::DownArrow),
            "ArrowLeft" => Some(Key::LeftArrow),
            "ArrowRight" => Some(Key::RightArrow),
            
            // 其他常用键
            "Delete" => Some(Key::Delete),
            "Home" => Some(Key::Home),
            "End" => Some(Key::End),
            "PageUp" => Some(Key::PageUp),
            "PageDown" => Some(Key::PageDown),
            "Insert" => Some(Key::Insert),
            
            _ => {
                log::debug!("未知的键码: {}", key_code);
                None
            }
        }
    }
} 