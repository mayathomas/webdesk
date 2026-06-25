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

/// 鼠标事件 (通过WebRTC数据通道传输)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MouseEvent {
    pub x: f64,
    pub y: f64,
    pub button: String,     // "left", "right", "middle"
    pub event_type: String, // "click", "move", "scroll"
    pub scroll_delta: Option<i32>,
}

/// 键盘事件 (通过WebRTC数据通道传输)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyboardEvent {
    pub key: String,
    pub event_type: String, // "press", "release"
}

/// H.264视频流配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoStreamConfig {
    pub width: u32,
    pub height: u32,
    pub fps: f32,
    pub bitrate: u32,
    pub codec: String, // "H264"
}

/// WebSocket消息类型 (纯WebRTC信令服务器)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WebSocketMessage {
    // 客户端注册和基础消息
    Register(RegisterRequest),
    RegisterResponse(RegisterResponse),

    // 浏览器连接消息
    BrowserConnect(BrowserConnectRequest),
    Connected {
        success: bool,
        message: String,
    },
    BrowserConnected {
        message: String,
    },
    BrowserDisconnected {
        message: String,
    },

    // WebRTC 信令消息
    WebRTCOffer {
        target_id: String, // 目标客户端ID
        session_description: SessionDescription,
    },
    WebRTCAnswer {
        target_id: String, // 目标浏览器ID
        session_description: SessionDescription,
    },
    WebRTCIceCandidate {
        target_id: String, // 目标ID
        ice_candidate: IceCandidate,
    },

    // H.264视频流配置 (信令阶段)
    VideoStreamConfig {
        target_id: String,
        config: VideoStreamConfig,
    },

    // 强制生成关键帧
    ForceKeyframe {
        target_id: String,
    },

    // 输入事件 (通过WebRTC数据通道发送，这里仅用于调试)
    MouseEvent(MouseEvent),
    KeyboardEvent(KeyboardEvent),

    // 控制消息
    Disconnect,
    Ping,
    Pong,
    Error {
        message: String,
    },
}

/// 客户端状态
#[derive(Debug, Clone)]
pub struct ClientState {
    pub auth_code: String,
    pub is_connected: bool,
    pub browser_connected: bool,
    pub video_stream_active: bool, // H.264视频流是否激活
}

impl ClientState {
    pub fn new(
        client_id: String,
        mac_address: String,
        auth_code: String,
        is_connected: bool,
        browser_connected: bool,
    ) -> Self {
        Self {
            auth_code,
            is_connected,
            browser_connected,
            video_stream_active: false,
        }
    }
}
