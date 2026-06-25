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

/// 数据分片头信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkHeader {
    pub message_id: String,      // 唯一消息ID
    pub chunk_index: usize,      // 当前分片索引
    pub total_chunks: usize,     // 总分片数量
    pub chunk_size: usize,       // 当前分片大小
    pub total_size: usize,       // 总数据大小
}

/// 屏幕数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenData {
    pub image_data: String,
    pub width: u32,          // 缩放后的宽度
    pub height: u32,         // 缩放后的高度
    pub original_width: u32, // 原始屏幕宽度
    pub original_height: u32, // 原始屏幕高度
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

/// 视频流配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoStreamConfig {
    pub width: u32,
    pub height: u32,
    pub fps: f32,
    pub bitrate: u32,
    pub codec: String,
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
    BrowserDisconnect,
    
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
    
    // 视频流控制消息
    VideoStreamConfig { 
        config: VideoStreamConfig
    },
    ForceKeyframe,
    
    // 控制消息
    Disconnect,
    Ping,
    Pong,
    Error { message: String },
}

/// 客户端状态
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum ClientState {
    WaitingForRegistration,
    WaitingForBrowser,
    WebRTCConnecting, // 新增：WebRTC连接中
    Idle,
}

/// 线程控制信号
#[derive(Debug, Clone)]
pub enum ThreadControlSignal {
    Start,
    Stop,
} 