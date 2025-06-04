use anyhow::Result;
use tauri::{AppHandle, State};
use crate::state::AppState;
use crate::types::ClientState;
use crate::network;

/// Tauri命令：获取客户端信息
#[tauri::command]
pub async fn get_client_info(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let config = state.client_config.lock().map_err(|e| e.to_string())?;
    
    let mac_address = config.get_mac_address().map_err(|e| e.to_string())?;
    
    // 如果没有客户端ID，尝试生成一个基于MAC地址的ID
    let client_id = if let Some(id) = &config.client_id {
        id.clone()
    } else {
        // 临时显示基于MAC地址的ID，直到服务器分配正式ID
        format!("temp-{}", &mac_address.replace(":", "")[0..8])
    };
    
    Ok(serde_json::json!({
        "clientId": client_id,
        "authCode": config.auth_code.clone(),
        "macAddress": mac_address,
        "version": "1.0.0"
    }))
}

/// Tauri命令：获取服务状态
#[tauri::command]
pub async fn get_service_status(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let config = state.client_config.lock().map_err(|e| e.to_string())?;
    let client_state = state.client_state.lock().map_err(|e| e.to_string())?;
    let service_running = state.service_running.lock().map_err(|e| e.to_string())?;
    
    Ok(serde_json::json!({
        "client_id": config.client_id.clone(),
        "state": format!("{:?}", *client_state),
        "running": *service_running
    }))
}

/// Tauri命令：启动远程控制服务
#[tauri::command]
pub async fn start_remote_service(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut service_running = state.service_running.lock().map_err(|e| e.to_string())?;
    
    if *service_running {
        return Err("服务已在运行中".to_string());
    }
    
    // 启动服务
    let app_state = state.clone_state();
    let service_app = app.clone();
    
    let handle = tokio::spawn(async move {
        if let Err(e) = network::run_remote_service(app_state, service_app).await {
            eprintln!("远程控制服务错误: {}", e);
        }
    });
    
    *state.service_handle.lock().map_err(|e| e.to_string())? = Some(handle);
    *service_running = true;
    
    Ok(())
}

/// Tauri命令：停止远程控制服务
#[tauri::command]
pub async fn stop_remote_service(state: State<'_, AppState>) -> Result<(), String> {
    let mut service_running = state.service_running.lock().map_err(|e| e.to_string())?;
    let mut service_handle = state.service_handle.lock().map_err(|e| e.to_string())?;
    
    if let Some(handle) = service_handle.take() {
        handle.abort();
    }
    
    *service_running = false;
    *state.client_state.lock().map_err(|e| e.to_string())? = ClientState::Idle;
    
    Ok(())
} 