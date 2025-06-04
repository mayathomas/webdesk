use serde::{Deserialize, Serialize};
use crate::screen::ChangedRegion;

/// 客户端注册请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterRequest {
    pub mac_address: String,
    pub auth_code: String,
}

/// 客户端注册响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterResponse {
    pub client_id: String,
    pub success: bool,
    pub message: String,
}

/// 屏幕数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenData {
    pub image_data: String,
    pub width: u32,
    pub height: u32,
    pub format: String, // "png", "jpeg", "diff"
    pub full_frame: bool, // 是否为完整帧
    pub changed_regions: Option<Vec<ChangedRegion>>, // 变化区域
}

/// 鼠标事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MouseEvent {
    pub x: f64,
    pub y: f64,
    pub button: String,
    pub event_type: String,
    pub scroll_delta: Option<i32>,
}

/// 键盘事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyboardEvent {
    pub key: String,
    pub event_type: String,
}

/// WebSocket消息类型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WebSocketMessage {
    Register(RegisterRequest),
    ScreenData(ScreenData),
    MouseEvent(MouseEvent),
    KeyboardEvent(KeyboardEvent),
    RegisterResponse(RegisterResponse),
    BrowserConnected { message: String },
    BrowserDisconnected { message: String },
    Error { message: String },
    Ping,
    Pong,
}

/// 客户端状态
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum ClientState {
    WaitingForRegistration,
    WaitingForBrowser,
    BrowserConnected,
    Idle,
}

/// 线程控制信号
#[derive(Debug, Clone)]
pub enum ThreadControlSignal {
    Start,
    Stop,
} 