use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::ice_transport::ice_credential_type::RTCIceCredentialType;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientConfig {
    pub server_url: String,
    pub client_id: Option<String>,
    pub auth_code: String,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            server_url: "ws://127.0.0.1:3000/ws".to_string(),
            client_id: None,
            auth_code: generate_auth_code(),
        }
    }
}

impl ClientConfig {
    /// 获取配置文件路径
    pub fn get_config_path() -> Result<PathBuf> {
        // 直接使用当前目录下的client.conf文件
        Ok(PathBuf::from("client.conf"))
    }

    /// 加载配置文件
    pub fn load() -> Result<Self> {
        let config_path = Self::get_config_path()?;
        
        if config_path.exists() {
            let content = fs::read_to_string(&config_path)?;
            let config: ClientConfig = toml::from_str(&content)?;
            Ok(config)
        } else {
            let config = Self::default();
            config.save()?;
            Ok(config)
        }
    }

    /// 保存配置文件
    pub fn save(&self) -> Result<()> {
        let config_path = Self::get_config_path()?;
        let content = toml::to_string_pretty(self)?;
        fs::write(&config_path, content)?;
        println!("📝 配置已保存到: {}", config_path.display());
        Ok(())
    }

    /// 更新客户端ID
    pub fn update_client_id(&mut self, client_id: String) -> Result<()> {
        self.client_id = Some(client_id);
        self.save()
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
pub struct Deployment {
    pub environment: String,
    pub cloud_ip: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WebRtcConfig {
    pub stun_servers: Vec<StunServer>,
    pub turn_servers: Vec<TurnServer>,
    pub rtc_config: RtcConfig,
    pub deployment: Deployment,
}

impl WebRtcConfig {
    /// 从YAML文件加载配置
    pub fn load_from_file(path: &str) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        let config: WebRtcConfig = serde_yaml::from_str(&content)?;
        Ok(config)
    }

    /// 获取调整后的配置（根据部署环境）
    pub fn get_adjusted_config(&self) -> Self {
        let mut config = self.clone();
        
        // 如果是云服务器环境，替换localhost为云服务器IP
        if self.deployment.environment == "cloud" {
            for turn_server in &mut config.turn_servers {
                if turn_server.url.contains("127.0.0.1") {
                    turn_server.url = turn_server.url.replace("127.0.0.1", &self.deployment.cloud_ip);
                }
            }
        }
        
        config
    }

    /// 转换为webrtc-rs的RTCIceServer格式
    pub fn to_ice_servers(&self) -> Vec<RTCIceServer> {
        let adjusted = self.get_adjusted_config();
        let mut ice_servers = Vec::new();
        
        // 添加STUN服务器
        for stun in &adjusted.stun_servers {
            ice_servers.push(RTCIceServer {
                urls: vec![stun.url.clone()],
                username: "".to_owned(),
                credential: "".to_owned(),
                credential_type: RTCIceCredentialType::Unspecified,
            });
        }
        
        // 添加TURN服务器
        for turn in &adjusted.turn_servers {
            ice_servers.push(RTCIceServer {
                urls: vec![turn.url.clone()],
                username: turn.username.clone(),
                credential: turn.credential.clone(),
                credential_type: RTCIceCredentialType::Password,
            });
        }
        
        ice_servers
    }
} 