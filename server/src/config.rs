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

    /// 转换为前端可用的格式
    pub fn to_frontend_format(&self) -> serde_json::Value {
        let adjusted = self.get_adjusted_config();
        
        let mut ice_servers = Vec::new();
        
        // 添加STUN服务器
        for stun in &adjusted.stun_servers {
            ice_servers.push(serde_json::json!({
                "urls": stun.url
            }));
        }
        
        // 添加TURN服务器
        for turn in &adjusted.turn_servers {
            ice_servers.push(serde_json::json!({
                "urls": turn.url,
                "username": turn.username,
                "credential": turn.credential
            }));
        }
        
        serde_json::json!({
            "iceServers": ice_servers,
            "iceCandidatePoolSize": adjusted.rtc_config.ice_candidate_pool_size,
            "bundlePolicy": adjusted.rtc_config.bundle_policy,
            "rtcpMuxPolicy": adjusted.rtc_config.rtcp_mux_policy
        })
    }
} 