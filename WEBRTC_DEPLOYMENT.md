# WebRTC远程桌面部署故障排除指南

## 🚀 快速诊断

### 1. 网络连接测试
访问 `http://115.159.125.146:3000/network-test.html` 进行基础网络测试。

### 2. 服务器要求

#### 端口配置
```bash
# 服务器需要开放的端口
HTTP/WebSocket: 3000 (TCP)
STUN/TURN: 3478 (UDP/TCP) - 如果使用自建TURN服务器
```

#### 防火墙设置 (服务器端)
```bash
# Ubuntu/CentOS
sudo ufw allow 3000/tcp
sudo iptables -A INPUT -p tcp --dport 3000 -j ACCEPT

# 如果使用自建TURN服务器
sudo ufw allow 3478/udp
sudo ufw allow 3478/tcp
```

#### 云服务器安全组
确保在云服务器控制台开放：
- 入站：TCP 3000
- 出站：UDP 1024-65535 (ICE需要)

## 🔍 常见问题诊断

### 问题1：信令连接失败
**症状**: 浏览器无法连接到WebSocket服务器

**解决方案**:
```bash
# 1. 检查服务器是否运行
netstat -tlnp | grep 3000

# 2. 检查防火墙
sudo ufw status

# 3. 检查服务器日志
# 查看是否有连接日志
```

### 问题2：ICE连接失败 
**症状**: 信令成功，但WebRTC连接状态停留在`connecting`或变为`failed`

**可能原因和解决方案**:

#### A. NAT类型不兼容
```javascript
// 浏览器控制台查看ICE候选类型
// 正常应该看到：host, srflx, relay 类型
```

#### B. TURN服务器不可用
当前使用的免费TURN服务器可能不稳定，建议：

1. **使用Coturn自建TURN服务器**:
```bash
# 安装Coturn
sudo apt-get install coturn

# 配置 /etc/turnserver.conf
listening-port=3478
fingerprint
use-auth-secret
static-auth-secret=your-secret-key
realm=yourdomain.com
total-quota=100
bps-capacity=0
stale-nonce=600
cert=/etc/ssl/certs/turn_server_cert.pem
pkey=/etc/ssl/private/turn_server_pkey.pem

# 启动服务
sudo systemctl enable coturn
sudo systemctl start coturn
```

2. **更新客户端配置使用自建TURN**:
```rust
// 在 client/src/webrtc.rs 中更新
RTCIceServer {
    urls: vec!["turn:115.159.125.146:3478".to_owned()],
    username: "your-username".to_owned(),
    credential: "your-password".to_owned(),
    ..Default::default()
},
```

### 问题3：数据传输失败
**症状**: 连接建立但无屏幕数据

**检查步骤**:
1. 浏览器控制台是否收到数据通道消息
2. 客户端是否正常捕获屏幕
3. 数据是否正确序列化

## 🛠️ 部署脚本

### 自动部署脚本
```bash
#!/bin/bash
# deploy.sh - 一键部署脚本

echo "🚀 开始部署WebRTC远程桌面..."

# 1. 更新系统
sudo apt update && sudo apt upgrade -y

# 2. 安装依赖
sudo apt install -y build-essential pkg-config libssl-dev

# 3. 安装Rust (如果没有)
if ! command -v rustc &> /dev/null; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source $HOME/.cargo/env
fi

# 4. 克隆并构建项目
git clone [your-repo-url]
cd remotecontrol

# 5. 构建服务端
cd server
cargo build --release

# 6. 配置防火墙
sudo ufw allow 3000/tcp
sudo ufw allow 3478/udp
sudo ufw allow 3478/tcp

# 7. 创建服务配置
sudo tee /etc/systemd/system/remote-desktop.service > /dev/null <<EOF
[Unit]
Description=WebRTC Remote Desktop Server
After=network.target

[Service]
Type=simple
User=ubuntu
WorkingDirectory=$(pwd)/server
ExecStart=$(pwd)/server/target/release/server
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
EOF

# 8. 启动服务
sudo systemctl daemon-reload
sudo systemctl enable remote-desktop
sudo systemctl start remote-desktop

echo "✅ 部署完成！访问 http://$(curl -s ifconfig.me):3000"
```

## 📱 客户端连接步骤

### 1. 修改服务器地址
```bash
# 编辑客户端配置，将服务器地址改为你的服务器IP
# 在 client/src/network.rs 中修改 WebSocket 连接地址
```

### 2. 运行客户端
```bash
cd client
cargo run
```

### 3. 浏览器访问
```
http://115.159.125.146:3000
```

## 🔧 高级优化

### 1. 使用Nginx反向代理
```nginx
server {
    listen 80;
    server_name yourdomain.com;
    
    location / {
        proxy_pass http://127.0.0.1:3000;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
    }
}
```

### 2. 启用HTTPS (推荐)
```bash
# 安装certbot
sudo apt install certbot python3-certbot-nginx

# 获取SSL证书
sudo certbot --nginx -d yourdomain.com

# 更新代码支持HTTPS
# 需要修改服务器代码支持TLS
```

### 3. 性能监控
```bash
# 安装htop查看系统资源
sudo apt install htop

# 监控网络连接
sudo netstat -tulnp | grep 3000

# 查看服务日志
sudo journalctl -u remote-desktop -f
```

## 🐛 调试技巧

### 1. 浏览器控制台
```javascript
// 查看WebRTC统计信息
peerConnection.getStats().then(stats => {
    stats.forEach(report => {
        console.log(report.type, report);
    });
});
```

### 2. 网络抓包
```bash
# 使用tcpdump抓取STUN/TURN流量
sudo tcpdump -i any -n port 3478

# 分析WebSocket流量
sudo tcpdump -i any -A port 3000
```

### 3. 日志级别调整
```rust
// 在客户端main.rs中增加详细日志
env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug")).init();
```

## 📞 紧急救援

如果所有方法都不行，可以尝试：

1. **降级到WebSocket模式**：临时注释掉WebRTC代码，使用原始WebSocket传输
2. **使用TeamViewer等工具**：作为临时方案
3. **VPN连接**：如果是企业网络限制，可以尝试VPN
4. **联系网络管理员**：检查企业防火墙/代理设置

## 🎯 成功连接的标志

正常工作时，你应该看到：
- ✅ 信令服务器连接成功
- ✅ ICE候选收集完成（包含srflx类型）
- ✅ WebRTC连接状态变为`connected`
- ✅ 数据通道状态变为`已连接`
- ✅ 屏幕画面正常显示 