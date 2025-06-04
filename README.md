# 远程桌面控制系统

这是一个基于Rust开发的远程桌面控制系统，允许通过浏览器远程控制客户端桌面。

## 功能特性

### 🌐 浏览器端
- 通过输入客户端ID和验证码连接到远程桌面
- 实时显示远程桌面画面
- 支持鼠标点击、移动、滚轮操作
- 支持键盘输入
- 一键断开连接

### 🖥️ 服务端
- WebSocket服务器处理客户端和浏览器连接
- HTTP服务器提供网页界面
- 根据MAC地址生成固定的客户端ID
- 消息转发和连接管理

### 💻 客户端
- 自动获取服务端颁发的客户端ID
- 生成随机验证码并保存到配置文件
- 实时屏幕截图传输
- 接收并执行远程鼠标和键盘操作

## 快速开始

### 1. 编译项目

```bash
# 编译整个工作空间
cargo build --release

# 或者分别编译
cd server && cargo build --release
cd ../client && cargo build --release
```

### 2. 启动服务端

```bash
# 使用默认配置启动
cd server
cargo run --release
```

服务端启动后会显示：
- 服务器地址: http://127.0.0.1:8080
- WebSocket路径: /ws（自动处理）

### 3. 启动客户端

```bash
cd client
cargo run --release
```

客户端启动后会显示：
- 客户端ID（用于浏览器连接）
- 验证码（用于认证）

### 4. 浏览器连接

1. 打开浏览器访问 http://127.0.0.1:8080
2. 输入客户端显示的客户端ID和验证码
3. 点击"连接"按钮
4. 成功连接后即可远程控制桌面

## 项目结构

```
remotecontrol/
├── Cargo.toml              # 工作空间配置
├── FOLLOWME.md            # 需求文档
├── README.md              # 项目说明
├── server/                # 服务端
│   ├── src/
│   │   ├── main.rs        # 服务端主程序
│   │   ├── server.rs      # WebSocket和HTTP服务器
│   │   └── types.rs       # 消息类型定义
│   ├── static/
│   │   └── index.html     # 网页界面
│   └── Cargo.toml
└── client/                # 客户端
    ├── src/
    │   ├── main.rs        # 客户端主程序
    │   ├── config.rs      # 配置文件处理
    │   ├── screen.rs      # 屏幕截图
    │   └── input.rs       # 输入控制
    └── Cargo.toml
```

## 配置文件

### 客户端配置 (client.conf)

客户端首次启动时会自动生成配置文件，保存在客户端程序的同级目录：`./client.conf`

配置内容：
```toml
server_url = "ws://127.0.0.1:8080/ws"
client_id = "0123456789abcdef"  # 服务端分配的ID（首次启动后自动获取）
auth_code = "1234567890"        # 随机生成的10位数字验证码
```

注意：
- MAC地址不会保存到配置文件中，而是在运行时自动获取
- 验证码为10位随机数字
- 客户端ID由服务端根据MAC地址生成并分配

## 命令行参数

### 服务端
```