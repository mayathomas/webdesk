// 使用Tauri全局API
console.log('🔍 检查Tauri API:', window.__TAURI__);

let invoke, listen;

if (window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.event) {
    invoke = window.__TAURI__.core.invoke;
    listen = window.__TAURI__.event.listen;
    console.log('✅ Tauri API 可用');
} else {
    console.error('❌ Tauri API 不可用');
    // 创建模拟函数避免崩溃
    invoke = async () => { throw new Error('Tauri API 不可用'); };
    listen = async () => { throw new Error('Tauri API 不可用'); };
    alert('Tauri API 不可用，请确保在Tauri应用中运行');
}

// 添加基本的控制台日志来验证脚本是否执行
console.log('🚀 script.js 开始执行');

// DOM元素
const elements = {
    clientId: document.getElementById('clientId'),
    authCode: document.getElementById('authCode'),
    macAddress: document.getElementById('macAddress'),
    statusDot: document.getElementById('statusDot'),
    statusText: document.getElementById('statusText'),
    statusDetails: document.getElementById('statusDetails'),
    logContainer: document.getElementById('logContainer')
};

console.log('📋 DOM元素获取完成');

// 状态检查间隔
let statusCheckInterval = null;

// 添加日志
function addLog(message, type = 'info') {
    const logItem = document.createElement('div');
    logItem.className = `log-item ${type}`;
    logItem.textContent = `[${new Date().toLocaleTimeString()}] ${message}`;
    elements.logContainer.appendChild(logItem);
    elements.logContainer.scrollTop = elements.logContainer.scrollHeight;
}

// 更新状态显示
function updateStatus(status, details) {
    elements.statusDot.className = `status-dot ${status}`;
    
    const statusTexts = {
        'offline': '离线',
        'connecting': '连接中',
        'waiting': '等待连接',
        'connected': '已连接'
    };
    
    elements.statusText.textContent = statusTexts[status] || '未知';
    elements.statusDetails.textContent = details;
}

// 加载客户端信息
async function loadClientInfo() {
    try {
        const info = await invoke('get_client_info');
        elements.clientId.textContent = info.clientId || '未分配';
        elements.authCode.textContent = info.authCode || '未设置';
        elements.macAddress.textContent = info.macAddress || '未知';
        addLog('✅ 客户端信息加载完成', 'success');
    } catch (error) {
        addLog(`❌ 加载客户端信息失败: ${error}`, 'error');
        elements.authCode.textContent = '加载失败';
        elements.macAddress.textContent = '加载失败';
    }
}

// 获取服务状态
async function getServiceStatus() {
    try {
        const status = await invoke('get_service_status');
        updateStatus(status.running ? 'connected' : 'offline', status.state);
    } catch (error) {
        addLog(`❌ 获取服务状态失败: ${error}`, 'error');
        updateStatus('offline', '状态未知');
    }
}

// 启动服务
async function startService() {
    try {
        addLog('🚀 正在启动远程控制服务...', 'info');
        updateStatus('connecting', '正在启动服务...');
        
        await invoke('start_remote_service');
        addLog('✅ 服务启动成功', 'success');
        
        // 开始定期检查状态
        statusCheckInterval = setInterval(getServiceStatus, 2000);
    } catch (error) {
        addLog(`❌ 启动服务失败: ${error}`, 'error');
        updateStatus('offline', '启动失败');
    }
}

// 停止服务
async function stopService() {
    try {
        addLog('⏹️ 正在停止远程控制服务...', 'info');
        
        await invoke('stop_remote_service');
        addLog('✅ 服务停止成功', 'success');
        updateStatus('offline', '服务已停止');
        
        // 停止状态检查
        if (statusCheckInterval) {
            clearInterval(statusCheckInterval);
            statusCheckInterval = null;
        }
    } catch (error) {
        addLog(`❌ 停止服务失败: ${error}`, 'error');
    }
}

// 监听后端事件
listen('status-update', (event) => {
    const { state, client_id } = event.payload;
    
    if (client_id && client_id !== '未分配') {
        elements.clientId.textContent = client_id;
    }
    
    if (state.includes('WaitingForRegistration')) {
        updateStatus('connecting', '正在注册客户端...');
        addLog('📡 正在向服务器注册...', 'info');
    } else if (state.includes('WaitingForBrowser')) {
        updateStatus('waiting', '等待浏览器连接...');
        addLog('⏳ 等待浏览器连接...', 'info');
    } else if (state.includes('BrowserConnected')) {
        updateStatus('connected', '浏览器已连接，远程控制活跃');
        addLog('🌐 浏览器已连接，开始远程控制', 'success');
    }
});

listen('client-info-update', (event) => {
    if (event.payload.clientId) {
        elements.clientId.textContent = event.payload.clientId;
        addLog(`🎉 获得客户端ID: ${event.payload.clientId}`, 'success');
    }
});

// 初始化
document.addEventListener('DOMContentLoaded', async () => {
    console.log('🖥️ DOMContentLoaded 事件触发');
    addLog('🖥️ 远程控制客户端界面已启动', 'info');
    
    try {
        console.log('📡 开始加载客户端信息...');
        await loadClientInfo();
        console.log('✅ 客户端信息加载完成');
    } catch (error) {
        console.error('❌ 加载客户端信息失败:', error);
        addLog(`❌ 加载客户端信息失败: ${error}`, 'error');
    }
    
    try {
        console.log('📊 开始获取服务状态...');
        await getServiceStatus();
        console.log('✅ 服务状态获取完成');
    } catch (error) {
        console.error('❌ 获取服务状态失败:', error);
        addLog(`❌ 获取服务状态失败: ${error}`, 'error');
    }
    
    // 自动启动远程控制服务
    try {
        console.log('🚀 开始自动启动远程控制服务...');
        addLog('🚀 自动启动远程控制服务...', 'info');
        await startService();
        console.log('✅ 远程控制服务启动完成');
    } catch (error) {
        console.error('❌ 自动启动服务失败:', error);
        addLog(`❌ 自动启动服务失败: ${error}`, 'error');
    }
    
    // 定期更新客户端信息
    console.log('⏰ 设置定期更新客户端信息...');
    setInterval(loadClientInfo, 30000);
    
    console.log('🎉 客户端初始化完成');
}); 