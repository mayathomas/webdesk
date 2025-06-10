use anyhow::Result;

// ===== 跨平台输入控制trait定义 =====

/// 跨平台键盘输入控制trait
pub trait KeyboardController {
    /// 发送键盘按下事件
    fn send_key_down(&self, key_code: u32, scan_code: u32) -> anyhow::Result<()>;
    
    /// 发送键盘松开事件
    fn send_key_up(&self, key_code: u32, scan_code: u32) -> anyhow::Result<()>;
}

/// 跨平台鼠标输入控制trait
pub trait MouseController {
    /// 移动鼠标到指定位置
    fn move_mouse(&self, x: i32, y: i32) -> anyhow::Result<()>;
    
    /// 发送鼠标按下事件
    fn send_mouse_down(&self, button: u32, x: i32, y: i32) -> anyhow::Result<()>;
    
    /// 发送鼠标松开事件
    fn send_mouse_up(&self, button: u32, x: i32, y: i32) -> anyhow::Result<()>;
    
    /// 发送鼠标滚轮事件
    fn send_scroll(&self, delta_x: i32, delta_y: i32) -> anyhow::Result<()>;
}

// ===== Windows平台实现 =====

#[cfg(windows)]
pub struct PlatformInputController;

#[cfg(windows)]
impl PlatformInputController {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(windows)]
impl KeyboardController for PlatformInputController {
    fn send_key_down(&self, key_code: u32, _scan_code: u32) -> anyhow::Result<()> {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, VIRTUAL_KEY, KEYBD_EVENT_FLAGS,
            MapVirtualKeyW, MAPVK_VK_TO_VSC
        };
        
        let mut input = INPUT::default();
        input.r#type = INPUT_KEYBOARD;
        
        // 根据微软官方建议，同时设置虚拟键码和扫描码
        // 使用MapVirtualKeyW动态获取正确的扫描码
        let scan_code = unsafe { MapVirtualKeyW(key_code, MAPVK_VK_TO_VSC) };
        
        input.Anonymous.ki = KEYBDINPUT {
            wVk: VIRTUAL_KEY(key_code as u16),
            wScan: scan_code as u16,
            dwFlags: KEYBD_EVENT_FLAGS(0), // 不使用任何特殊标志
            time: 0,
            dwExtraInfo: 0,
        };
        
        let result = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
        if result == 0 {
            return Err(anyhow::anyhow!("Failed to send key down event"));
        }
        
        Ok(())
    }
    
    fn send_key_up(&self, key_code: u32, _scan_code: u32) -> anyhow::Result<()> {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, VIRTUAL_KEY, KEYEVENTF_KEYUP, MapVirtualKeyW, MAPVK_VK_TO_VSC
        };
        
        let mut input = INPUT::default();
        input.r#type = INPUT_KEYBOARD;
        
        // 同样动态获取扫描码
        let scan_code = unsafe { MapVirtualKeyW(key_code, MAPVK_VK_TO_VSC) };
        
        input.Anonymous.ki = KEYBDINPUT {
            wVk: VIRTUAL_KEY(key_code as u16),
            wScan: scan_code as u16,
            dwFlags: KEYEVENTF_KEYUP, // 只设置按键释放标志
            time: 0,
            dwExtraInfo: 0,
        };
        
        let result = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
        if result == 0 {
            return Err(anyhow::anyhow!("Failed to send key up event"));
        }
        
        Ok(())
    }
}

#[cfg(windows)]
impl MouseController for PlatformInputController {
    fn move_mouse(&self, x: i32, y: i32) -> anyhow::Result<()> {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_MOUSE, MOUSEINPUT, MOUSEEVENTF_MOVE, MOUSEEVENTF_ABSOLUTE
        };
        use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
        
        let screen_width = unsafe { GetSystemMetrics(SM_CXSCREEN) };
        let screen_height = unsafe { GetSystemMetrics(SM_CYSCREEN) };
        
        // 转换为绝对坐标 (0-65535)
        let abs_x = ((x * 65535) / screen_width) as i32;
        let abs_y = ((y * 65535) / screen_height) as i32;
        
        let mut input = INPUT::default();
        input.r#type = INPUT_MOUSE;
        input.Anonymous.mi = MOUSEINPUT {
            dx: abs_x,
            dy: abs_y,
            mouseData: 0,
            dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE,
            time: 0,
            dwExtraInfo: 0,
        };
        
        let result = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
        if result == 0 {
            return Err(anyhow::anyhow!("Failed to move mouse"));
        }
        
        Ok(())
    }
    
    fn send_mouse_down(&self, button: u32, x: i32, y: i32) -> anyhow::Result<()> {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_MOUSE, MOUSEINPUT, 
            MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_MIDDLEDOWN
        };
        
        // 先移动鼠标到指定位置
        self.move_mouse(x, y)?;
        
        let mut input = INPUT::default();
        input.r#type = INPUT_MOUSE;
        input.Anonymous.mi = MOUSEINPUT {
            dx: 0,
            dy: 0,
            mouseData: 0,
            dwFlags: match button {
                1 => MOUSEEVENTF_LEFTDOWN,
                2 => MOUSEEVENTF_RIGHTDOWN,
                3 => MOUSEEVENTF_MIDDLEDOWN,
                _ => return Err(anyhow::anyhow!("Unsupported mouse button: {}", button)),
            },
            time: 0,
            dwExtraInfo: 0,
        };
        
        let result = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
        if result == 0 {
            return Err(anyhow::anyhow!("Failed to send mouse down event"));
        }
        
        Ok(())
    }
    
    fn send_mouse_up(&self, button: u32, x: i32, y: i32) -> anyhow::Result<()> {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_MOUSE, MOUSEINPUT,
            MOUSEEVENTF_LEFTUP, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_MIDDLEUP
        };
        
        // 先移动鼠标到指定位置
        self.move_mouse(x, y)?;
        
        let mut input = INPUT::default();
        input.r#type = INPUT_MOUSE;
        input.Anonymous.mi = MOUSEINPUT {
            dx: 0,
            dy: 0,
            mouseData: 0,
            dwFlags: match button {
                1 => MOUSEEVENTF_LEFTUP,
                2 => MOUSEEVENTF_RIGHTUP,
                3 => MOUSEEVENTF_MIDDLEUP,
                _ => return Err(anyhow::anyhow!("Unsupported mouse button: {}", button)),
            },
            time: 0,
            dwExtraInfo: 0,
        };
        
        let result = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
        if result == 0 {
            return Err(anyhow::anyhow!("Failed to send mouse up event"));
        }
        
        Ok(())
    }
    
    fn send_scroll(&self, delta_x: i32, delta_y: i32) -> anyhow::Result<()> {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_MOUSE, MOUSEINPUT, MOUSEEVENTF_WHEEL, MOUSEEVENTF_HWHEEL
        };
        
        if delta_y != 0 {
            let mut input = INPUT::default();
            input.r#type = INPUT_MOUSE;
            input.Anonymous.mi = MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: (delta_y * 120) as u32, // Windows标准滚轮增量是120
                dwFlags: MOUSEEVENTF_WHEEL,
                time: 0,
                dwExtraInfo: 0,
            };
            
            let result = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
            if result == 0 {
                return Err(anyhow::anyhow!("Failed to send vertical scroll"));
            }
        }
        
        if delta_x != 0 {
            let mut input = INPUT::default();
            input.r#type = INPUT_MOUSE;
            input.Anonymous.mi = MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: (delta_x * 120) as u32, // 水平滚轮
                dwFlags: MOUSEEVENTF_HWHEEL,
                time: 0,
                dwExtraInfo: 0,
            };
            
            let result = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
            if result == 0 {
                return Err(anyhow::anyhow!("Failed to send horizontal scroll"));
            }
        }
        
        Ok(())
    }
}


// ===== macOS平台实现 =====

#[cfg(target_os = "macos")]
pub struct PlatformInputController {
    event_source: core_graphics::event_source::CGEventSource,
}

#[cfg(target_os = "macos")]
impl PlatformInputController {
    pub fn new() -> Self {
        use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
        
        let event_source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
            .expect("Failed to create CGEventSource");
        
        Self { event_source }
    }
}

#[cfg(target_os = "macos")]
impl KeyboardController for PlatformInputController {
    fn send_key_down(&self, key_code: u32, _scan_code: u32) -> anyhow::Result<()> {
        use core_graphics::event::{CGEvent, CGKeyCode};
        
        let event = CGEvent::new_keyboard_event(
            self.event_source.clone(),
            key_code as CGKeyCode,
            true, // key down
        ).map_err(|e| anyhow::anyhow!("Failed to create key down event: {:?}", e))?;
        
        event.post(core_graphics::event::CGEventTapLocation::HID);
        Ok(())
    }
    
    fn send_key_up(&self, key_code: u32, _scan_code: u32) -> anyhow::Result<()> {
        use core_graphics::event::{CGEvent, CGKeyCode};
        
        let event = CGEvent::new_keyboard_event(
            self.event_source.clone(),
            key_code as CGKeyCode,
            false, // key up
        ).map_err(|e| anyhow::anyhow!("Failed to create key up event: {:?}", e))?;
        
        event.post(core_graphics::event::CGEventTapLocation::HID);
        Ok(())
    }
}

#[cfg(target_os = "macos")]
impl MouseController for PlatformInputController {
    fn move_mouse(&self, x: i32, y: i32) -> anyhow::Result<()> {
        use core_graphics::{event::{CGEvent, CGEventType}, geometry::CGPoint};
        
        let point = CGPoint::new(x as f64, y as f64);
        let event = CGEvent::new_mouse_event(
            self.event_source.clone(),
            CGEventType::MouseMoved,
            point,
            core_graphics::event::CGMouseButton::Left,
        ).map_err(|e| anyhow::anyhow!("Failed to create mouse move event: {:?}", e))?;
        
        event.post(core_graphics::event::CGEventTapLocation::HID);
        Ok(())
    }
    
    fn send_mouse_down(&self, button: u32, x: i32, y: i32) -> anyhow::Result<()> {
        use core_graphics::{event::{CGEvent, CGEventType, CGMouseButton}, geometry::CGPoint};
        
        let point = CGPoint::new(x as f64, y as f64);
        
        let (event_type, mouse_button) = match button {
            1 => (CGEventType::LeftMouseDown, CGMouseButton::Left),
            2 => (CGEventType::RightMouseDown, CGMouseButton::Right),
            3 => (CGEventType::OtherMouseDown, CGMouseButton::Center),
            _ => return Err(anyhow::anyhow!("Unsupported mouse button: {}", button)),
        };
        
        let event = CGEvent::new_mouse_event(
            self.event_source.clone(),
            event_type,
            point,
            mouse_button,
        ).map_err(|e| anyhow::anyhow!("Failed to create mouse down event: {:?}", e))?;
        
        event.post(core_graphics::event::CGEventTapLocation::HID);
        Ok(())
    }
    
    fn send_mouse_up(&self, button: u32, x: i32, y: i32) -> anyhow::Result<()> {
        use core_graphics::{event::{CGEvent, CGEventType, CGMouseButton}, geometry::CGPoint};
        
        let point = CGPoint::new(x as f64, y as f64);
        
        let (event_type, mouse_button) = match button {
            1 => (CGEventType::LeftMouseUp, CGMouseButton::Left),
            2 => (CGEventType::RightMouseUp, CGMouseButton::Right),
            3 => (CGEventType::OtherMouseUp, CGMouseButton::Center),
            _ => return Err(anyhow::anyhow!("Unsupported mouse button: {}", button)),
        };
        
        let event = CGEvent::new_mouse_event(
            self.event_source.clone(),
            event_type,
            point,
            mouse_button,
        ).map_err(|e| anyhow::anyhow!("Failed to create mouse up event: {:?}", e))?;
        
        event.post(core_graphics::event::CGEventTapLocation::HID);
        Ok(())
    }
    
    fn send_scroll(&self, delta_x: i32, delta_y: i32) -> anyhow::Result<()> {
        use core_graphics::event::{CGEvent, ScrollEventUnit};
        
        if delta_y != 0 || delta_x != 0 {
            let event = CGEvent::new_scroll_event(
                self.event_source.clone(),
                ScrollEventUnit::Pixel,
                2, // wheel count
                delta_y,
                delta_x,
                0, // delta3 (未使用)
            ).map_err(|e| anyhow::anyhow!("Failed to create scroll event: {:?}", e))?;
            
            event.post(core_graphics::event::CGEventTapLocation::HID);
        }
        
        Ok(())
    }
}


// ===== Linux平台实现 =====

#[cfg(target_os = "linux")]
pub struct PlatformInputController {
    display: *mut x11::xlib::Display,
    root_window: x11::xlib::Window,
}

#[cfg(target_os = "linux")]
unsafe impl Send for PlatformInputController {}
#[cfg(target_os = "linux")]
unsafe impl Sync for PlatformInputController {}

#[cfg(target_os = "linux")]
impl PlatformInputController {
    pub fn new() -> Self {
        use x11::xlib;
        use std::ptr;
        
        unsafe {
            let display = xlib::XOpenDisplay(ptr::null());
            if display.is_null() {
                panic!("Failed to open X11 display");
            }
            
            let screen = xlib::XDefaultScreen(display);
            let root_window = xlib::XRootWindow(display, screen);
            
            Self {
                display,
                root_window,
            }
        }
    }
    
    fn scan_code_to_keycode(&self, scan_code: u32) -> u32 {
        scan_code + 8
    }
}

#[cfg(target_os = "linux")]
impl Drop for PlatformInputController {
    fn drop(&mut self) {
        unsafe {
            if !self.display.is_null() {
                x11::xlib::XCloseDisplay(self.display);
            }
        }
    }
}

#[cfg(target_os = "linux")]
impl KeyboardController for PlatformInputController {
    fn send_key_down(&self, _key_code: u32, scan_code: u32) -> anyhow::Result<()> {
        use x11::{xlib, xtest::XTestFakeKeyEvent};
        
        unsafe {
            let keycode = self.scan_code_to_keycode(scan_code);
            let result = XTestFakeKeyEvent(
                self.display,
                keycode,
                xlib::True, // key down
                xlib::CurrentTime,
            );
            
            if result == 0 {
                return Err(anyhow::anyhow!("Failed to send key down event"));
            }
            
            xlib::XFlush(self.display);
        }
        
        Ok(())
    }
    
    fn send_key_up(&self, _key_code: u32, scan_code: u32) -> anyhow::Result<()> {
        use x11::{xlib, xtest::XTestFakeKeyEvent};
        
        unsafe {
            let keycode = self.scan_code_to_keycode(scan_code);
            let result = XTestFakeKeyEvent(
                self.display,
                keycode,
                xlib::False, // key up
                xlib::CurrentTime,
            );
            
            if result == 0 {
                return Err(anyhow::anyhow!("Failed to send key up event"));
            }
            
            xlib::XFlush(self.display);
        }
        
        Ok(())
    }
}

#[cfg(target_os = "linux")]
impl MouseController for PlatformInputController {
    fn move_mouse(&self, x: i32, y: i32) -> anyhow::Result<()> {
        use x11::{xlib, xtest::XTestFakeMotionEvent};
        
        unsafe {
            let result = XTestFakeMotionEvent(
                self.display,
                -1, // all screens
                x,
                y,
                xlib::CurrentTime,
            );
            
            if result == 0 {
                return Err(anyhow::anyhow!("Failed to move mouse"));
            }
            
            xlib::XFlush(self.display);
        }
        
        Ok(())
    }
    
    fn send_mouse_down(&self, button: u32, x: i32, y: i32) -> anyhow::Result<()> {
        use x11::{xlib, xtest::XTestFakeButtonEvent};
        
        self.move_mouse(x, y)?;
        
        unsafe {
            let result = XTestFakeButtonEvent(
                self.display,
                button,
                xlib::True, // button down
                xlib::CurrentTime,
            );
            
            if result == 0 {
                return Err(anyhow::anyhow!("Failed to send mouse down event"));
            }
            
            xlib::XFlush(self.display);
        }
        
        Ok(())
    }
    
    fn send_mouse_up(&self, button: u32, x: i32, y: i32) -> anyhow::Result<()> {
        use x11::{xlib, xtest::XTestFakeButtonEvent};
        
        self.move_mouse(x, y)?;
        
        unsafe {
            let result = XTestFakeButtonEvent(
                self.display,
                button,
                xlib::False, // button up
                xlib::CurrentTime,
            );
            
            if result == 0 {
                return Err(anyhow::anyhow!("Failed to send mouse up event"));
            }
            
            xlib::XFlush(self.display);
        }
        
        Ok(())
    }
    
    fn send_scroll(&self, delta_x: i32, delta_y: i32) -> anyhow::Result<()> {
        use x11::{xlib, xtest::XTestFakeButtonEvent};
        
        unsafe {
            // 垂直滚动
            if delta_y > 0 {
                for _ in 0..delta_y {
                    XTestFakeButtonEvent(self.display, 4, xlib::True, xlib::CurrentTime);
                    XTestFakeButtonEvent(self.display, 4, xlib::False, xlib::CurrentTime);
                }
            } else if delta_y < 0 {
                for _ in 0..(-delta_y) {
                    XTestFakeButtonEvent(self.display, 5, xlib::True, xlib::CurrentTime);
                    XTestFakeButtonEvent(self.display, 5, xlib::False, xlib::CurrentTime);
                }
            }
            
            // 水平滚动
            if delta_x > 0 {
                for _ in 0..delta_x {
                    XTestFakeButtonEvent(self.display, 7, xlib::True, xlib::CurrentTime);
                    XTestFakeButtonEvent(self.display, 7, xlib::False, xlib::CurrentTime);
                }
            } else if delta_x < 0 {
                for _ in 0..(-delta_x) {
                    XTestFakeButtonEvent(self.display, 6, xlib::True, xlib::CurrentTime);
                    XTestFakeButtonEvent(self.display, 6, xlib::False, xlib::CurrentTime);
                }
            }
            
            xlib::XFlush(self.display);
        }
        
        Ok(())
    }
}

// ===== 统一的公共接口 =====

/// 对外暴露的InputController，兼容原有API
pub struct InputController {
    controller: PlatformInputController,
}

impl InputController {
    pub fn new() -> Self {
        Self {
            controller: PlatformInputController::new(),
        }
    }

    /// 模拟鼠标移动
    pub fn move_mouse(&self, x: f64, y: f64) -> Result<()> {
        self.controller.move_mouse(x as i32, y as i32)?;
        Ok(())
    }

    /// 模拟鼠标按下（不释放）
    pub fn press_mouse_button(&self, x: f64, y: f64, button: &str) -> Result<()> {
        let button_code = match button {
            "left" => 1,
            "right" => 2, 
            "middle" => 3,
            _ => 1,
        };

        self.controller.send_mouse_down(button_code, x as i32, y as i32)?;
        Ok(())
    }

    /// 模拟鼠标释放
    pub fn release_mouse_button(&self, x: f64, y: f64, button: &str) -> Result<()> {
        let button_code = match button {
            "left" => 1,
            "right" => 2,
            "middle" => 3,
            _ => 1,
        };

        self.controller.send_mouse_up(button_code, x as i32, y as i32)?;
        Ok(())
    }

    /// 模拟鼠标滚轮
    pub fn scroll_mouse(&self, x: f64, y: f64, delta: i32) -> Result<()> {
        self.controller.move_mouse(x as i32, y as i32)?;
        self.controller.send_scroll(0, delta)?;
        Ok(())
    }

    /// 模拟按键按下
    pub fn press_key(&self, key_code: &str) -> Result<()> {
        if let Some((key_code, scan_code)) = self.parse_key(key_code) {
            self.controller.send_key_down(key_code, scan_code)?;
        }
        Ok(())
    }

    /// 模拟按键释放
    pub fn release_key(&self, key_code: &str) -> Result<()> {
        if let Some((key_code, scan_code)) = self.parse_key(key_code) {
            self.controller.send_key_up(key_code, scan_code)?;
        }
        Ok(())
    }

    /// 解析键码，返回(key_code, scan_code)
    fn parse_key(&self, key_code: &str) -> Option<(u32, u32)> {
        match key_code {
            // 字母键 (A-Z)
            "KeyA" => Some((0x41, 0x1E)),
            "KeyB" => Some((0x42, 0x30)),
            "KeyC" => Some((0x43, 0x2E)),
            "KeyD" => Some((0x44, 0x20)),
            "KeyE" => Some((0x45, 0x12)),
            "KeyF" => Some((0x46, 0x21)),
            "KeyG" => Some((0x47, 0x22)),
            "KeyH" => Some((0x48, 0x23)),
            "KeyI" => Some((0x49, 0x17)),
            "KeyJ" => Some((0x4A, 0x24)),
            "KeyK" => Some((0x4B, 0x25)),
            "KeyL" => Some((0x4C, 0x26)),
            "KeyM" => Some((0x4D, 0x32)),
            "KeyN" => Some((0x4E, 0x31)),
            "KeyO" => Some((0x4F, 0x18)),
            "KeyP" => Some((0x50, 0x19)),
            "KeyQ" => Some((0x51, 0x10)),
            "KeyR" => Some((0x52, 0x13)),
            "KeyS" => Some((0x53, 0x1F)),
            "KeyT" => Some((0x54, 0x14)),
            "KeyU" => Some((0x55, 0x16)),
            "KeyV" => Some((0x56, 0x2F)),
            "KeyW" => Some((0x57, 0x11)),
            "KeyX" => Some((0x58, 0x2D)),
            "KeyY" => Some((0x59, 0x15)),
            "KeyZ" => Some((0x5A, 0x2C)),
            
            // 数字键 (0-9)
            "Digit0" => Some((0x30, 0x0B)),
            "Digit1" => Some((0x31, 0x02)),
            "Digit2" => Some((0x32, 0x03)),
            "Digit3" => Some((0x33, 0x04)),
            "Digit4" => Some((0x34, 0x05)),
            "Digit5" => Some((0x35, 0x06)),
            "Digit6" => Some((0x36, 0x07)),
            "Digit7" => Some((0x37, 0x08)),
            "Digit8" => Some((0x38, 0x09)),
            "Digit9" => Some((0x39, 0x0A)),
            
            // 功能键 (F1-F12)
            "F1" => Some((0x70, 0x3B)),
            "F2" => Some((0x71, 0x3C)),
            "F3" => Some((0x72, 0x3D)),
            "F4" => Some((0x73, 0x3E)),
            "F5" => Some((0x74, 0x3F)),
            "F6" => Some((0x75, 0x40)),
            "F7" => Some((0x76, 0x41)),
            "F8" => Some((0x77, 0x42)),
            "F9" => Some((0x78, 0x43)),
            "F10" => Some((0x79, 0x44)),
            "F11" => Some((0x7A, 0x57)),
            "F12" => Some((0x7B, 0x58)),
            
            // 特殊键
            "Enter" => Some((0x0D, 0x1C)),
            "Space" => Some((0x20, 0x39)),
            "Backspace" => Some((0x08, 0x0E)),
            "Tab" => Some((0x09, 0x0F)),
            "Escape" => Some((0x1B, 0x01)),
            "ShiftLeft" => Some((0xA0, 0x2A)),
            "ShiftRight" => Some((0xA1, 0x36)),
            "ControlLeft" => Some((0xA2, 0x1D)),
            "ControlRight" => Some((0xA3, 0x1D)),
            "AltLeft" => Some((0xA4, 0x38)),
            "AltRight" => Some((0xA5, 0x38)),
            "MetaLeft" => Some((0x5B, 0x5B)),
            "MetaRight" => Some((0x5C, 0x5C)),
            
            // 方向键
            "ArrowUp" => Some((0x26, 0x48)),
            "ArrowDown" => Some((0x28, 0x50)),
            "ArrowLeft" => Some((0x25, 0x4B)),
            "ArrowRight" => Some((0x27, 0x4D)),
            
            // 其他常用键
            "Delete" => Some((0x2E, 0x53)),
            "Home" => Some((0x24, 0x47)),
            "End" => Some((0x23, 0x4F)),
            "PageUp" => Some((0x21, 0x49)),
            "PageDown" => Some((0x22, 0x51)),
            "Insert" => Some((0x2D, 0x52)),
            
            _ => {
                log::warn!("Unsupported key code: {}", key_code);
                None
            }
        }
    }
} 