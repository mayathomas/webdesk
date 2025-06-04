mod config;
mod input;
mod screen;
mod types;
mod state;
mod commands;
mod network;
mod threads;

use anyhow::Result;
use config::ClientConfig;
use state::AppState;

fn main() -> Result<()> {
    // 初始化配置
    let client_config = ClientConfig::load()?;
    println!("{:?}", client_config);
    
    // 确保MAC地址能获取到
    if let Ok(mac) = client_config.get_mac_address() {
        println!("🏠 MAC地址: {}", mac);
    }
    
    // 确保验证码已生成
    println!("🔑 验证码: {}", client_config.auth_code);
    
    // 创建应用状态
    let app_state = AppState::new(client_config);
    
    // 启动Tauri 2.0应用
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            commands::get_client_info,
            commands::get_service_status,
            commands::start_remote_service,
            commands::stop_remote_service
        ])
        .run(tauri::generate_context!())
        .expect("启动Tauri应用失败");
    
    Ok(())
}
