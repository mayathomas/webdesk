use std::sync::{Arc, Mutex};
use crate::config::ClientConfig;
use crate::types::ClientState;

/// 应用状态管理
#[derive(Debug)]
pub struct AppState {
    pub client_config: Arc<Mutex<ClientConfig>>,
    pub service_running: Arc<Mutex<bool>>,
    pub client_state: Arc<Mutex<ClientState>>,
    pub service_handle: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl AppState {
    /// 创建新的应用状态
    pub fn new(client_config: ClientConfig) -> Self {
        Self {
            client_config: Arc::new(Mutex::new(client_config)),
            service_running: Arc::new(Mutex::new(false)),
            client_state: Arc::new(Mutex::new(ClientState::Idle)),
            service_handle: Arc::new(Mutex::new(None)),
        }
    }
    
    /// 克隆状态（用于传递给异步任务）
    pub fn clone_state(&self) -> Self {
        Self {
            client_config: self.client_config.clone(),
            service_running: self.service_running.clone(),
            client_state: self.client_state.clone(),
            service_handle: self.service_handle.clone(),
        }
    }
} 