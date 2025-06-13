// WebRTC连接状态
export interface ConnectionState {
  signaling: 'disconnected' | 'connecting' | 'connected' | 'error'
  ice: 'disconnected' | 'connecting' | 'connected' | 'error'
  dataChannel: 'disconnected' | 'connecting' | 'connected' | 'error'
  video: 'disconnected' | 'connecting' | 'connected' | 'error'
}

// 视频配置
export interface VideoConfig {
  width: number
  height: number
  fps: number
  bitrate: number
  codec: 'H264' | 'VP8' | 'VP9' | 'AV1'
}

// 表单动作状态
export interface ActionState {
  success: boolean
  message: string
}

// 连接表单数据
export interface ConnectionFormData {
  clientId: string
  authCode: string
  videoQuality: string
}

// 鼠标事件
export interface MouseEvent {
  type: 'move' | 'down' | 'up' | 'click' | 'dblclick'
  x: number
  y: number
  button?: 'left' | 'right' | 'middle'
}

// 键盘事件
export interface KeyboardEvent {
  type: 'down' | 'up' | 'press'
  key: string
  code: string
  ctrlKey: boolean
  altKey: boolean
  shiftKey: boolean
  metaKey: boolean
}

// 滚轮事件
export interface WheelEvent {
  type: 'wheel'
  deltaX: number
  deltaY: number
  deltaZ: number
}

// WebRTC配置
export interface WebRTCConfig {
  iceServers: RTCIceServer[]
  signalsServerUrl: string
  dataChannelConfig: RTCDataChannelInit
}

// API响应
export interface ApiResponse<T = any> {
  success: boolean
  data?: T
  message?: string
  timestamp: string
}

// 连接统计
export interface ConnectionStats {
  resolution: string
  framerate: number
  bitrate: number
  latency: number
  packetLoss: number
  codec: string
}

// 服务器配置
export interface ServerConfig {
  signals_server_url: string
  port: number
  cors_origins: string[]
}

// YAML配置文件结构
export interface WebRTCConfigYAML {
  stun_servers: Array<{
    url: string
  }>
  turn_servers?: Array<{
    url: string
    username: string
    credential: string
  }>
  rtc_config: {
    ice_candidate_pool_size: number
    bundle_policy: string
    rtcp_mux_policy: string
    ice_transport_policy?: string
  }
  signals_server: {
    url: string
    reconnect_attempts: number
    reconnect_delay: number
  }
  data_channel: {
    ordered: boolean
    max_retransmits: number
    max_packet_life_time?: number
  }
  video_config: {
    codec: string
    profiles: Array<{
      name: string
      width: number
      height: number
      fps: number
      bitrate: number
    }>
  }
}

// 视频配置档案
export interface VideoProfile {
  name: string
  width: number
  height: number
  fps: number
  bitrate: number
  config?: VideoConfig
}

// 视频配置档案（包含配置对象）
export interface VideoProfileWithConfig {
  name: string
  config: VideoConfig
} 