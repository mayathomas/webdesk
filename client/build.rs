use std::env;

fn main() {
    // 解决Tauri 2.x DEP_TAURI_DEV环境变量问题
    // 需要为tauri-build crate设置正确的环境变量
    let is_dev = env::var("PROFILE").unwrap_or_default() == "debug";
    
    // 设置Tauri内部期望的环境变量
    unsafe {
        env::set_var("DEP_TAURI_DEV", if is_dev { "true" } else { "false" });
    }
    
    // 运行Tauri构建
    tauri_build::build()
} 