use serde::{Deserialize, Serialize};
use std::fs;
use anyhow::Result;

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
pub struct HttpsConfig {
    pub enabled: bool,
    pub http_port: u16,
    pub https_port: u16,
    pub cert_path: String,
    pub key_path: String,
    pub auto_generate_cert: bool,
}

impl Default for HttpsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            http_port: 3000,
            https_port: 3443,
            cert_path: "cert.pem".to_string(),
            key_path: "key.pem".to_string(),
            auto_generate_cert: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WebRtcConfig {
    pub stun_servers: Vec<StunServer>,
    pub turn_servers: Vec<TurnServer>,
    pub rtc_config: RtcConfig,
    #[serde(default)]
    pub https_config: HttpsConfig,
}

impl WebRtcConfig {
    /// 从YAML文件加载配置
    pub fn load_from_file(path: &str) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        let config: WebRtcConfig = serde_yaml::from_str(&content)?;
        Ok(config)
    }

    /// 转换为前端可用的格式
    pub fn to_frontend_format(&self) -> serde_json::Value {
        let mut ice_servers = Vec::new();
        
        // 添加STUN服务器
        for stun in &self.stun_servers {
            ice_servers.push(serde_json::json!({
                "urls": stun.url
            }));
        }
        
        // 添加TURN服务器
        for turn in &self.turn_servers {
            ice_servers.push(serde_json::json!({
                "urls": turn.url,
                "username": turn.username,
                "credential": turn.credential
            }));
        }
        
        serde_json::json!({
            "iceServers": ice_servers,
            "iceCandidatePoolSize": self.rtc_config.ice_candidate_pool_size,
            "bundlePolicy": self.rtc_config.bundle_policy,
            "rtcpMuxPolicy": self.rtc_config.rtcp_mux_policy,
            "iceTransportPolicy": "all"
        })
    }
} 