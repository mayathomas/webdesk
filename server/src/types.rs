use serde::{Deserialize, Serialize};

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

/// 浏览器连接请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserConnectRequest {
    pub client_id: String,
    pub auth_code: String,
}

/// 屏幕截图数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenData {
    pub image_data: String, // base64编码的图片数据
    pub width: u32,
    pub height: u32,
}

/// 鼠标事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MouseEvent {
    pub x: f64,
    pub y: f64,
    pub button: String, // "left", "right", "middle"
    pub event_type: String, // "click", "move", "scroll"
    pub scroll_delta: Option<i32>,
}

/// 键盘事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyboardEvent {
    pub key: String,
    pub event_type: String, // "press", "release"
}

/// WebSocket消息类型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WebSocketMessage {
    // 客户端消息
    Register(RegisterRequest),
    ScreenData(ScreenData),
    Ping,
    
    // 浏览器消息
    BrowserConnect(BrowserConnectRequest),
    MouseEvent(MouseEvent),
    KeyboardEvent(KeyboardEvent),
    Disconnect,
    
    // 服务端响应
    RegisterResponse(RegisterResponse),
    Connected { success: bool, message: String },
    BrowserConnected { message: String },
    BrowserDisconnected { message: String },
    Error { message: String },
    Pong,
}

/// 客户端状态
#[derive(Debug, Clone)]
pub struct ClientState {
    pub client_id: String,
    pub mac_address: String,
    pub auth_code: String,
    pub is_connected: bool,
    pub browser_connected: bool,
} 