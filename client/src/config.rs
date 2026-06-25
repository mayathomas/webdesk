use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use webrtc::ice_transport::ice_server::RTCIceServer;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientConfig {
    pub server_url: String,
    pub client_id: Option<String>,
    pub auth_code: String,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            server_url: "ws://0.0.0.0:3000/ws".to_string(),
            client_id: None,
            auth_code: generate_auth_code(),
        }
    }
}

impl ClientConfig {
    /// 获取配置文件路径
    pub fn get_config_path() -> Result<PathBuf> {
        // 直接使用当前目录下的webrtc-config.yaml文件
        Ok(PathBuf::from("webrtc-config.yaml"))
    }

    /// 从webrtc-config.yaml加载配置
    pub fn load() -> Result<Self> {
        let config_path = Self::get_config_path()?;
        
        if config_path.exists() {
            // 从WebRTC配置文件中读取server_config部分
            let webrtc_config = WebRtcConfig::load_from_file("webrtc-config.yaml")?;
            Ok(ClientConfig {
                server_url: webrtc_config.server_config.server_url,
                client_id: Some(webrtc_config.server_config.client_id),
                auth_code: webrtc_config.server_config.auth_code,
            })
        } else {
            let config = Self::default();
            Ok(config)
        }
    }

    /// 更新客户端ID
    pub fn update_client_id(&mut self, client_id: String) -> Result<()> {
        self.client_id = Some(client_id);
        // 这里可以实现更新webrtc-config.yaml中的client_id
        Ok(())
    }

    /// 获取MAC地址（运行时获取，不保存）
    pub fn get_mac_address(&self) -> Result<String> {
        if let Ok(mac) = mac_address::get_mac_address() {
            if let Some(mac) = mac {
                return Ok(mac.to_string());
            }
        }
        
        // 如果获取失败，生成一个随机的MAC地址
        Ok(format!("00:00:00:{:02x}:{:02x}:{:02x}", 
            rand::random::<u8>(), 
            rand::random::<u8>(), 
            rand::random::<u8>()))
    }
}

/// 生成10位数字验证码
fn generate_auth_code() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    
    // 生成10位数字验证码
    (0..10)
        .map(|_| rng.gen_range(0..10).to_string())
        .collect::<String>()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StunServer {
    pub url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TurnServer {
    pub url: String,
    pub username: String,
    pub credential: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RtcConfig {
    pub ice_candidate_pool_size: u32,
    pub bundle_policy: String,
    pub rtcp_mux_policy: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerConfig {
    pub server_url: String,
    pub client_id: String,
    pub auth_code: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WebRtcConfig {
    pub stun_servers: Vec<StunServer>,
    pub turn_servers: Vec<TurnServer>,
    pub rtc_config: RtcConfig,
    pub server_config: ServerConfig,
}

impl WebRtcConfig {
    /// 从YAML文件加载配置
    pub fn load_from_file(path: &str) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        let config: WebRtcConfig = serde_yaml::from_str(&content)?;
        Ok(config)
    }

    /// 转换为webrtc-rs的RTCIceServer格式
    pub fn to_ice_servers(&self) -> Vec<RTCIceServer> {
        let mut ice_servers = Vec::new();
        
        // 添加 STUN 服务器
        for stun in &self.stun_servers {
            let mut server = RTCIceServer::default();
            server.urls = vec![stun.url.clone()];
            ice_servers.push(server);
        }
        
        // 添加 TURN 服务器
        for turn in &self.turn_servers {
            let mut server = RTCIceServer::default();
            server.urls = vec![turn.url.clone()];
            server.username = turn.username.clone();
            server.credential = turn.credential.clone();
            ice_servers.push(server);
        }
        
        ice_servers
    }
} 