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

/// WebRTC SDP 会话描述
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDescription {
    pub sdp_type: String, // "offer", "answer"
    pub sdp: String,
}

/// WebRTC ICE 候选
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IceCandidate {
    pub candidate: String,
    pub sdp_mid: Option<String>,
    pub sdp_mline_index: Option<u16>,
}

/// 屏幕截图数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenData {
    pub image_data: String, // base64编码的图片数据
    pub width: u32,          // 缩放后的宽度
    pub height: u32,         // 缩放后的高度
    pub original_width: u32, // 原始屏幕宽度
    pub original_height: u32, // 原始屏幕高度
    pub format: String, // "png", "jpeg", "diff"
    pub full_frame: bool, // 是否为完整帧
    pub changed_regions: Option<Vec<ChangedRegion>>, // 变化区域
}

/// 变化区域
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangedRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub data: String, // base64编码的区域数据
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

/// WebSocket消息类型（保持兼容，作为信令服务器）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WebSocketMessage {
    // 客户端注册和基础消息
    Register(RegisterRequest),
    RegisterResponse(RegisterResponse),
    
    // 浏览器连接消息
    BrowserConnect(BrowserConnectRequest),
    Connected { success: bool, message: String },
    BrowserConnected { message: String },
    BrowserDisconnected { message: String },
    
    // WebRTC 信令消息
    WebRTCOffer { 
        target_id: String, // 目标客户端ID
        session_description: SessionDescription 
    },
    WebRTCAnswer { 
        target_id: String, // 目标浏览器ID
        session_description: SessionDescription 
    },
    WebRTCIceCandidate { 
        target_id: String, // 目标ID
        ice_candidate: IceCandidate 
    },
    
    // 传统WebSocket消息（向后兼容）
    ScreenData(ScreenData),
    MouseEvent(MouseEvent),
    KeyboardEvent(KeyboardEvent),
    
    // 控制消息
    Disconnect,
    Ping,
    Pong,
    Error { message: String },
}

/// 客户端状态
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ClientState {
    pub client_id: String,
    pub mac_address: String,
    pub auth_code: String,
    pub is_connected: bool,
    pub browser_connected: bool,
}

impl ClientState {
    pub fn new(client_id: String, mac_address: String, auth_code: String, is_connected: bool, browser_connected: bool) -> Self {
        Self { client_id, mac_address, auth_code, is_connected, browser_connected }
    }
} 
