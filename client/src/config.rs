use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

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