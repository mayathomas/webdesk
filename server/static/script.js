// WebRTC相关变量
let peerConnection = null;
let dataChannel = null;
let signalingSocket = null;
let currentClientId = null; // 添加全局变量保存当前客户端ID
let screenCanvas = null;
let canvasContext = null;
let isConnected = false;

// 屏幕数据保存
let lastScreenData = null; // 保存最后一次的屏幕数据

// 分片重组相关变量
let chunkBuffers = new Map(); // 存储分片数据的Map: messageId -> {chunks: [], totalChunks: number, totalSize: number}

// 画布相关变量
let canvas = null;
let ctx = null;
let desktopCanvas = null;
let desktopCtx = null;

// 鼠标焦点状态
window.mouseInCanvas = false;

let frameCount = 0;
let lastStatsTime = Date.now();
let renderTimes = [];
let isRendering = false;
let pendingUpdate = false;
let lastRenderTime = 0;
const FRAME_TIME = 16.67; // 60fps = 16.67ms per frame
// 性能监控变量
let performanceData = {
    frameCount: 0,
    totalBytes: 0,
    startTime: Date.now(),
    renderTimes: []
};

// 分片消息处理
let messageChunks = new Map();
let expectedChunks = 0;
let receivedChunks = 0;

// WebRTC配置 - 将从API动态加载
let rtcConfiguration = null;

// 加载WebRTC配置
async function loadWebRTCConfig() {
    try {
        const response = await fetch('/api/webrtc-config');
        if (!response.ok) {
            throw new Error(`HTTP ${response.status}: ${response.statusText}`);
        }
        rtcConfiguration = await response.json();
        console.log('📋 已从服务器加载WebRTC配置:', rtcConfiguration);
        return rtcConfiguration;
    } catch (error) {
        console.error('❌ 加载WebRTC配置失败:', error);
        // 使用默认配置作为备份
        rtcConfiguration = {
            iceServers: [
                { urls: 'stun:stun.l.google.com:19302' },
                { urls: 'stun:stun.cloudflare.com:3478' }
            ],
            iceCandidatePoolSize: 10,
            bundlePolicy: 'max-bundle',
            rtcpMuxPolicy: 'require',
            iceTransportPolicy: 'all'
        };
        console.log('💡 使用默认WebRTC配置');
        return rtcConfiguration;
    }
}

function showStatus(message, type = 'success') {
    const status = document.getElementById('status');
    status.textContent = message;
    status.className = `status ${type}`;
    status.style.display = 'block';
    
    setTimeout(() => {
        status.style.display = 'none';
    }, 5000);
}

function updateConnectionInfo(field, value) {
    const element = document.getElementById(field);
    if (element) {
        element.textContent = value;
    }
}

async function connect() {
    const clientId = document.getElementById('clientId').value.trim();
    const authCode = document.getElementById('authCode').value.trim();

    if (!clientId || !authCode) {
        showStatus('请输入客户端ID和验证码', 'error');
        return;
    }

    // 保存当前客户端ID
    currentClientId = clientId;

    showStatus('正在加载配置...', 'info');
    document.getElementById('connectionInfo').style.display = 'block';

    try {
        // 1. 加载WebRTC配置
        await loadWebRTCConfig();
        
        // 2. 连接信令服务器
        await connectSignalingServer(clientId, authCode);
        
        // 3. 初始化WebRTC
        await initWebRTC();
        
        // 4. 创建Offer
        await createOffer(clientId);
        
    } catch (error) {
        console.error('连接失败:', error);
        showStatus(`连接失败: ${error.message}`, 'error');
    }
}

function connectSignalingServer(clientId, authCode) {
    return new Promise((resolve, reject) => {
        signalingSocket = new WebSocket(`ws://${location.host}/ws`);

        signalingSocket.onopen = function() {
            console.log('📡 信令服务器连接已建立');
            updateConnectionInfo('signalingState', '已连接');
            
            const connectMessage = {
                type: 'BrowserConnect',
                client_id: clientId,
                auth_code: authCode
            };

            signalingSocket.send(JSON.stringify(connectMessage));
        };

        signalingSocket.onmessage = function(event) {
            handleSignalingMessage(JSON.parse(event.data));
        };

        signalingSocket.onclose = function() {
            console.log('📡 信令服务器连接已关闭');
            updateConnectionInfo('signalingState', '已断开');
            showStatus('信令连接已断开', 'error');
            resetUI();
        };

        signalingSocket.onerror = function(error) {
            console.error('📡 信令服务器错误:', error);
            updateConnectionInfo('signalingState', '错误');
            reject(new Error('信令服务器连接失败'));
        };

        // 等待连接成功消息
        const originalOnMessage = signalingSocket.onmessage;
        signalingSocket.onmessage = function(event) {
            const message = JSON.parse(event.data);
            if (message.type === 'Connected' && message.success) {
                console.log('✅ 信令连接成功');
                showStatus('信令连接成功，开始WebRTC协商...', 'info');
                signalingSocket.onmessage = originalOnMessage;
                resolve();
            } else if (message.type === 'Error') {
                reject(new Error(message.message));
            }
        };
    });
}

async function initWebRTC() {
    console.log('🌐 初始化WebRTC...');
    console.log('📋 ICE服务器配置:', rtcConfiguration.iceServers);
    
    // 添加ICE传输策略 - 允许所有连接方式
    if (!rtcConfiguration.iceTransportPolicy) {
        rtcConfiguration.iceTransportPolicy = 'all'; // 允许host、srflx、relay所有候选
        console.log('🔧 设置ICE传输策略为: all (允许所有连接方式)');
    }
    
    // 创建PeerConnection
    peerConnection = new RTCPeerConnection(rtcConfiguration);
    
    // 监听连接状态变化
    peerConnection.onconnectionstatechange = function() {
        console.log('🔗 WebRTC连接状态:', peerConnection.connectionState);
        updateConnectionInfo('connectionState', peerConnection.connectionState);
        
        if (peerConnection.connectionState === 'connected') {
            showStatus('WebRTC P2P连接已建立！', 'success');
            console.log('🎉 P2P连接成功建立');
            showRemoteDesktop();
            initCanvas();
        } else if (peerConnection.connectionState === 'failed') {
            showStatus('WebRTC连接失败', 'error');
            console.error('❌ WebRTC连接失败，检查网络和防火墙设置');
            console.log('💡 建议：1. 检查防火墙设置 2. 确认TURN服务器可用 3. 检查网络连接');
        } else if (peerConnection.connectionState === 'disconnected') {
            console.warn('⚠️ WebRTC连接断开');
            showStatus('连接断开，尝试重连...', 'info');
        }
    };
    
    // 监听ICE连接状态
    peerConnection.oniceconnectionstatechange = function() {
        console.log('🧊 ICE连接状态:', peerConnection.iceConnectionState);
        updateConnectionInfo('iceState', peerConnection.iceConnectionState);
        
        if (peerConnection.iceConnectionState === 'failed') {
            console.error('❌ ICE连接失败');
            console.log('💡 可能原因：1. NAT类型不兼容 2. 防火墙阻止 3. TURN服务器不可用');
            
            // 获取详细的连接统计信息
            getConnectionStats();
        } else if (peerConnection.iceConnectionState === 'connected') {
            console.log('✅ ICE连接建立成功');
            
            // 显示成功的连接统计
            getConnectionStats();
        } else if (peerConnection.iceConnectionState === 'checking') {
            console.log('🔍 正在检查ICE连接...');
        }
    };
    
    // 监听ICE收集状态
    peerConnection.onicegatheringstatechange = function() {
        console.log('📦 ICE收集状态:', peerConnection.iceGatheringState);
        if (peerConnection.iceGatheringState === 'complete') {
            console.log('✅ ICE候选收集完成');
        }
    };
    
    // 监听ICE候选
    peerConnection.onicecandidate = function(event) {
        if (event.candidate) {
            const candidate = event.candidate;
            console.log('🧊 收集到ICE候选:', {
                candidate: candidate.candidate,
                type: candidate.type,
                protocol: candidate.protocol,
                address: candidate.address,
                port: candidate.port
            });
            
            const message = {
                type: 'WebRTCIceCandidate',
                target_id: currentClientId,
                ice_candidate: {
                    candidate: candidate.candidate,
                    sdp_mid: candidate.sdpMid,
                    sdp_mline_index: candidate.sdpMLineIndex
                }
            };
            
            signalingSocket.send(JSON.stringify(message));
        } else {
            console.log('🏁 ICE候选收集结束');
        }
    };
    
    // 监听数据通道
    peerConnection.ondatachannel = function(event) {
        const channel = event.channel;
        console.log('📡 收到数据通道:', channel.label);
        setupDataChannel(channel);
    };
}

async function createOffer(clientId) {
    console.log('📤 创建WebRTC Offer...');
    
    // 创建数据通道（用于控制信号）
    dataChannel = peerConnection.createDataChannel('control', {
        ordered: true
    });
    
    setupDataChannel(dataChannel);
    
    // 创建Offer
    const offer = await peerConnection.createOffer();
    await peerConnection.setLocalDescription(offer);
    
    // 发送Offer到客户端
    const message = {
        type: 'WebRTCOffer',
        target_id: clientId,
        session_description: {
            sdp_type: 'offer',
            sdp: offer.sdp
        }
    };
    
    signalingSocket.send(JSON.stringify(message));
    console.log('📤 Offer已发送');
}

function setupDataChannel(channel) {
    channel.onopen = function() {
        console.log('✅ 数据通道已打开:', channel.label);
        updateConnectionInfo('dataChannelState', '已连接');
        isConnected = true;
    };
    
    channel.onclose = function() {
        console.log('❌ 数据通道已关闭:', channel.label);
        updateConnectionInfo('dataChannelState', '已断开');
        isConnected = false;
    };
    
    channel.onmessage = function(event) {
        // 处理来自客户端的屏幕数据
        try {
            if (event.data instanceof ArrayBuffer) {
                // 判断是否是分片数据格式
                if (isChunkedMessage(event.data)) {
                    // 分片数据处理
                    handleChunkedMessage(event.data);
                } else {
                    // 尝试解析为JSON
                    try {
                        const jsonText = new TextDecoder().decode(event.data);
                        const message = JSON.parse(jsonText);
                        console.log('📥 收到WebRTC数据通道消息，类型:', message.type);
                        
                        if (message.type === 'ScreenData') {
                            console.log('🖼️ 处理屏幕数据:', message.width + 'x' + message.height);
                            handleScreenData(message);
                        } else {
                            console.log('🔍 未知的数据通道消息类型:', message.type);
                        }
                    } catch (jsonError) {
                        // 不是JSON，可能是其他二进制数据（如截图）
                        console.log('📊 收到非JSON ArrayBuffer数据，大小:', event.data.byteLength);
                        // 这里可以添加处理其他类型二进制数据的逻辑
                    }
                }
            } else {
                // 原有的字符串消息处理
                const message = JSON.parse(event.data);
                console.log('📥 收到WebRTC数据通道消息，类型:', message.type);
                
                if (message.type === 'ScreenData') {
                    // 提取实际的屏幕数据 (message 是 WebSocketMessage，实际数据在其字段中)
                    const screenData = message; // message 本身就包含了所有 ScreenData 字段
                    console.log('🖼️ 处理屏幕数据:', screenData.width + 'x' + screenData.height);
                    handleScreenData(screenData);
                } else {
                    console.log('🔍 未知的数据通道消息类型:', message.type);
                }
            }
        } catch (error) {
            console.error('❌ 处理数据通道消息失败:', error);
            console.error('原始数据:', event.data);
        }
    };
    
    channel.onerror = function(error) {
        console.error('数据通道错误:', error);
        updateConnectionInfo('dataChannelState', '错误');
    };
}

function handleSignalingMessage(message) {
    console.log('📥 收到信令消息:', message.type);
    
    switch (message.type) {
        case 'WebRTCAnswer':
            handleAnswer(message.session_description);
            break;
            
        case 'WebRTCIceCandidate':
            handleIceCandidate(message.ice_candidate);
            break;
            
        case 'Error':
            showStatus(message.message, 'error');
            break;
            
        default:
            console.log('未知信令消息类型:', message.type);
    }
}

async function handleAnswer(sessionDescription) {
    console.log('📥 处理WebRTC Answer');
    
    const answer = new RTCSessionDescription({
        type: 'answer',
        sdp: sessionDescription.sdp
    });
    
    await peerConnection.setRemoteDescription(answer);
    console.log('✅ Answer已设置');
}

async function handleIceCandidate(iceCandidate) {
    console.log('🧊 添加ICE候选:', iceCandidate.candidate);
    
    const candidate = new RTCIceCandidate({
        candidate: iceCandidate.candidate,
        sdpMid: iceCandidate.sdp_mid,
        sdpMLineIndex: iceCandidate.sdp_mline_index
    });
    
    await peerConnection.addIceCandidate(candidate);
}

function handleScreenData(screenData) {
    updateDesktopAsync(screenData);
}

function showRemoteDesktop() {
    document.getElementById('loginSection').style.display = 'none';
    document.getElementById('remoteDesktop').classList.add('active');
}

function initCanvas() {
    canvas = document.getElementById('desktopCanvas');
    ctx = canvas.getContext('2d');
    
    // 启用图像平滑处理
    ctx.imageSmoothingEnabled = true;
    ctx.imageSmoothingQuality = 'high';

    // 添加鼠标事件监听
    canvas.addEventListener('click', handleMouseClick);
    canvas.addEventListener('mousemove', throttle(handleMouseMove, 16)); // 限制到60fps
    canvas.addEventListener('contextmenu', e => e.preventDefault());
    
    // 添加键盘事件监听
    document.addEventListener('keydown', handleKeyDown);
    document.addEventListener('keyup', handleKeyUp);

    // 隐藏加载提示
    document.getElementById('loading').style.display = 'none';
    
    // 初始化鼠标焦点状态
    window.mouseInCanvas = false;
    console.log('🎯 初始化鼠标焦点状态:', window.mouseInCanvas);
    
    console.log('画布初始化完成，开始性能监控');
    startPerformanceMonitoring();
}

// 异步版本的桌面更新
async function updateDesktopAsync(screenData) {
    const startTime = performance.now();
    
    // 初始化桌面画布（如果需要）
    if (!desktopCanvas) {
        desktopCanvas = document.createElement('canvas');
        desktopCtx = desktopCanvas.getContext('2d');
        desktopCtx.imageSmoothingEnabled = false;
    }
    
    if (screenData.full_frame && screenData.image_data) {
        await updateFullFrameAsync(screenData, startTime);
    } else if (screenData.changed_regions && screenData.changed_regions.length > 0) {
        await updateDifferentialFrameAsync(screenData, startTime);
    }
}

// 异步完整帧更新
function updateFullFrameAsync(screenData, startTime) {
    return new Promise((resolve, reject) => {
        console.log(`🖼️ 处理完整帧: ${screenData.width}x${screenData.height}`);
        
        const img = new Image();
        img.onload = function() {
            try {
                // 保存屏幕数据
                lastScreenData = screenData;
                
                // 计算自适应显示尺寸
                const container = canvas.parentElement;
                const containerRect = container.getBoundingClientRect();
                const availableWidth = containerRect.width - 20; // 留出一些边距
                const availableHeight = window.innerHeight * 0.8; // 使用80%的视窗高度
                
                console.log(`📐 可用显示区域: ${availableWidth.toFixed(0)}x${availableHeight.toFixed(0)}`);
                console.log(`📷 截屏尺寸: ${screenData.width}x${screenData.height}`);
                
                let canvasWidth, canvasHeight;
                
                // 自适应窗口模式：计算最优显示尺寸
                if (screenData.width <= availableWidth && screenData.height <= availableHeight) {
                    canvasWidth = screenData.width;
                    canvasHeight = screenData.height;
                    console.log(`✅ 自适应模式（原始尺寸）: ${canvasWidth}x${canvasHeight}`);
                } else {
                    const scaleForWidth = availableWidth / screenData.width;
                    const scaleForHeight = availableHeight / screenData.height;
                    const scale = Math.min(scaleForWidth, scaleForHeight);
                    
                    canvasWidth = Math.round(screenData.width * scale);
                    canvasHeight = Math.round(screenData.height * scale);
                    console.log(`🔄 自适应模式（缩放）: ${canvasWidth}x${canvasHeight}, 比例=${scale.toFixed(3)}`);
                }
                
                // 设置canvas的像素尺寸
                canvas.width = canvasWidth;
                canvas.height = canvasHeight;
                
                // 设置CSS显示尺寸（1:1显示）
                canvas.style.width = canvasWidth + 'px';
                canvas.style.height = canvasHeight + 'px';
                
                console.log(`📐 Canvas设置: 像素=${canvas.width}x${canvas.height}, CSS=${canvasWidth}x${canvasHeight}`);
                
                // 初始化离屏画布（与显示canvas相同尺寸）
                if (!desktopCanvas) {
                    desktopCanvas = document.createElement('canvas');
                    desktopCtx = desktopCanvas.getContext('2d');
                    desktopCtx.imageSmoothingEnabled = true; // 启用图像平滑，提高缩放质量
                    desktopCtx.imageSmoothingQuality = 'high';
                }
                desktopCanvas.width = canvasWidth;
                desktopCanvas.height = canvasHeight;
                
                // 7. 将截屏图像缩放绘制到离屏画布，完全填充
                desktopCtx.drawImage(img, 0, 0, screenData.width, screenData.height, 0, 0, canvasWidth, canvasHeight);
                
                console.log(`🎯 图像缩放: ${screenData.width}x${screenData.height} -> ${canvasWidth}x${canvasHeight}`);
                
                // 8. 立即更新显示
                scheduleRender();
                
                const renderTime = performance.now() - startTime;
                recordRenderTime(renderTime);
                performanceData.frameCount++;
                
                console.log(`✅ 完整帧完成，耗时: ${renderTime.toFixed(2)}ms`);
                resolve();
                
            } catch (error) {
                console.error('完整帧渲染失败:', error);
                reject(error);
            }
        };
        
        img.onerror = () => {
            console.error('❌ 完整帧图像加载失败');
            reject(new Error('图像加载失败'));
        };
        
        const mimeType = screenData.format === 'jpeg' ? 'image/jpeg' : 'image/png';
        img.src = `data:${mimeType};base64,${screenData.image_data}`;
    });
}

// 优化的异步差分帧更新 - 支持缩放
function updateDifferentialFrameAsync(screenData, startTime) {
    return new Promise((resolve) => {
        const regions = screenData.changed_regions;
        console.log(`🔄 处理差分更新: ${regions.length} 个区域`);
        
        // 如果没有上一帧数据，无法进行差分更新
        if (!lastScreenData) {
            console.warn('⚠️ 没有基础帧数据，跳过差分更新');
            resolve();
            return;
        }
        
        // 计算缩放比例
        const scaleX = canvas.width / lastScreenData.width;
        const scaleY = canvas.height / lastScreenData.height;
        
        console.log(`📏 差分更新缩放比例: X=${scaleX.toFixed(3)}, Y=${scaleY.toFixed(3)}`);
        
        // 并行加载所有区域
        const promises = regions.map(region => loadRegionImage(region));
        
        // 等待所有区域加载完成
        Promise.allSettled(promises).then(results => {
            let successCount = 0;
            let errorCount = 0;
            
            // 绘制成功加载的区域
            results.forEach((result, index) => {
                if (result.status === 'fulfilled') {
                    const { img, region } = result.value;
                    try {
                        // 计算缩放后的区域位置和尺寸
                        const scaledX = region.x * scaleX;
                        const scaledY = region.y * scaleY;
                        const scaledWidth = region.width * scaleX;
                        const scaledHeight = region.height * scaleY;
                        
                        // 将区域图像缩放绘制到对应位置
                        desktopCtx.drawImage(img, 
                            0, 0, region.width, region.height,  // 源图像尺寸
                            scaledX, scaledY, scaledWidth, scaledHeight  // 目标位置和尺寸
                        );
                        successCount++;
                    } catch (error) {
                        console.error('绘制区域失败:', error);
                        errorCount++;
                    }
                } else {
                    errorCount++;
                }
            });
            
            // 更新显示
            scheduleRender();
            
            const renderTime = performance.now() - startTime;
            recordRenderTime(renderTime);
            performanceData.frameCount++;
            
            console.log(`✅ 差分更新完成: ${successCount}/${regions.length} 区域，耗时: ${renderTime.toFixed(2)}ms`);
            
            if (errorCount > 0) {
                console.warn(`⚠️ ${errorCount} 个区域加载失败`);
            }
            
            resolve();
        });
    });
}

// 加载单个区域图像
function loadRegionImage(region) {
    return new Promise((resolve, reject) => {
        const img = new Image();
        
        img.onload = () => resolve({ img, region });
        img.onerror = () => reject(new Error(`区域加载失败: ${region.x},${region.y}`));
        
        img.src = `data:image/jpeg;base64,${region.data}`;
    });
}

// 节流函数
function throttle(func, limit) {
    let inThrottle;
    return function() {
        const args = arguments;
        const context = this;
        if (!inThrottle) {
            func.apply(context, args);
            inThrottle = true;
            setTimeout(() => inThrottle = false, limit);
        }
    }
}

// 智能渲染调度 - 避免过度重绘
function scheduleRender() {
    if (isRendering) {
        pendingUpdate = true;
        return;
    }
    
    const now = performance.now();
    const elapsed = now - lastRenderTime;
    
    if (elapsed >= FRAME_TIME) {
        // 立即渲染
        performRender();
    } else {
        // 延迟到下一个合适的时机
        setTimeout(() => {
            if (pendingUpdate) {
                performRender();
            }
        }, FRAME_TIME - elapsed);
    }
}

function performRender() {
    if (isRendering) return;
    
    isRendering = true;
    pendingUpdate = false;
    lastRenderTime = performance.now();
    
    requestAnimationFrame(() => {
        try {
            // 清除整个canvas
            ctx.clearRect(0, 0, canvas.width, canvas.height);
            // 将离屏canvas的内容完全填充到显示canvas
            ctx.drawImage(desktopCanvas, 0, 0, desktopCanvas.width, desktopCanvas.height, 0, 0, canvas.width, canvas.height);
        } catch (error) {
            console.error('渲染失败:', error);
        } finally {
            isRendering = false;
            
            // 如果有待处理的更新，继续处理
            if (pendingUpdate) {
                scheduleRender();
            }
        }
    });
}

// 性能监控
function recordRenderTime(time) {
    performanceData.renderTimes.push(time);
    if (performanceData.renderTimes.length > 100) {
        performanceData.renderTimes.shift();
    }
}

function startPerformanceMonitoring() {
    setInterval(() => {
        const now = Date.now();
        const elapsed = now - performanceData.startTime;
        
        if (elapsed >= 5000 && performanceData.frameCount > 0) {
            const fps = (performanceData.frameCount * 1000 / elapsed).toFixed(1);
            const avgRenderTime = performanceData.renderTimes.length > 0 ? 
                (performanceData.renderTimes.reduce((a, b) => a + b, 0) / performanceData.renderTimes.length).toFixed(2) : 0;
            
            console.log(`📊 WebRTC性能统计 [${elapsed/1000}秒]:
🎯 渲染帧率: ${fps} FPS
⏱️ 平均渲染时间: ${avgRenderTime}ms  
📦 总帧数: ${performanceData.frameCount}`);
            
            // 重置计数器
            performanceData.frameCount = 0;
            performanceData.startTime = now;
            performanceData.renderTimes.length = 0;
        }
    }, 1000);
}

function handleMouseClick(event) {
    if (!isConnected || !dataChannel || !lastScreenData) return;

    const rect = canvas.getBoundingClientRect();
    
    // 获取鼠标在canvas上的相对位置（CSS像素）
    const canvasX = event.clientX - rect.left;
    const canvasY = event.clientY - rect.top;
    
    // 转换为canvas内部坐标（考虑CSS缩放）
    const canvasPixelX = canvasX * (canvas.width / rect.width);
    const canvasPixelY = canvasY * (canvas.height / rect.height);
    
    // 坐标转换链条：Canvas像素坐标 -> 客户端缩放后坐标 -> 客户端原始坐标
    
    // 第1步：Canvas像素坐标 -> 客户端缩放后坐标
    const scaledX = canvasPixelX * (lastScreenData.width / canvas.width);
    const scaledY = canvasPixelY * (lastScreenData.height / canvas.height);
    
    // 第2步：客户端缩放后坐标 -> 客户端原始坐标（这是关键！）
    const clientScaleX = lastScreenData.original_width / lastScreenData.width;
    const clientScaleY = lastScreenData.original_height / lastScreenData.height;
    
    const originalX = scaledX * clientScaleX;
    const originalY = scaledY * clientScaleY;
    
    // 确保坐标在原始屏幕范围内
    const clampedX = Math.max(0, Math.min(originalX, lastScreenData.original_width - 1));
    const clampedY = Math.max(0, Math.min(originalY, lastScreenData.original_height - 1));
    
    console.log(`🖱️ 鼠标点击坐标转换链条:
    📍 浏览器鼠标位置: (${event.clientX}, ${event.clientY})
    📐 Canvas CSS尺寸: ${rect.width.toFixed(0)}x${rect.height.toFixed(0)}
    📐 Canvas像素尺寸: ${canvas.width}x${canvas.height}
    📍 Canvas像素坐标: (${canvasPixelX.toFixed(1)}, ${canvasPixelY.toFixed(1)})
    📍 客户端缩放后坐标: (${scaledX.toFixed(1)}, ${scaledY.toFixed(1)})
    📏 客户端缩放后尺寸: ${lastScreenData.width}x${lastScreenData.height}
    📏 客户端原始尺寸: ${lastScreenData.original_width}x${lastScreenData.original_height}
    📏 客户端缩放比例: (${clientScaleX.toFixed(3)}, ${clientScaleY.toFixed(3)})
    🎯 最终原始屏幕坐标: (${clampedX.toFixed(0)}, ${clampedY.toFixed(0)})`);

    const mouseEvent = {
        type: 'MouseEvent',
        x: Math.round(clampedX),
        y: Math.round(clampedY),
        button: event.button === 0 ? 'left' : (event.button === 2 ? 'right' : 'middle'),
        event_type: 'click'
    };

    dataChannel.send(JSON.stringify(mouseEvent));
}

function handleMouseMove(event) {
    if (!isConnected || !dataChannel || !lastScreenData) return;

    const rect = canvas.getBoundingClientRect();
    
    // 获取鼠标在canvas上的相对位置（CSS像素）
    const canvasX = event.clientX - rect.left;
    const canvasY = event.clientY - rect.top;
    
    // 更新全局鼠标位置状态（用于键盘焦点管理）
    const prevMouseInCanvas = window.mouseInCanvas;
    window.mouseInCanvas = (canvasX >= 0 && canvasX <= rect.width && canvasY >= 0 && canvasY <= rect.height);
    
    // 当焦点状态改变时打印调试信息
    if (prevMouseInCanvas !== window.mouseInCanvas) {
        console.log(`🎯 鼠标焦点状态变更: ${prevMouseInCanvas} -> ${window.mouseInCanvas} 
        位置: (${canvasX.toFixed(1)}, ${canvasY.toFixed(1)}) 
        Canvas范围: 0~${rect.width.toFixed(1)} x 0~${rect.height.toFixed(1)}`);
    }
    
    // 转换为canvas内部坐标（考虑CSS缩放）
    const canvasPixelX = canvasX * (canvas.width / rect.width);
    const canvasPixelY = canvasY * (canvas.height / rect.height);
    
    // 坐标转换链条：Canvas像素坐标 -> 客户端缩放后坐标 -> 客户端原始坐标
    
    // 第1步：Canvas像素坐标 -> 客户端缩放后坐标
    const scaledX = canvasPixelX * (lastScreenData.width / canvas.width);
    const scaledY = canvasPixelY * (lastScreenData.height / canvas.height);
    
    // 第2步：客户端缩放后坐标 -> 客户端原始坐标
    const clientScaleX = lastScreenData.original_width / lastScreenData.width;
    const clientScaleY = lastScreenData.original_height / lastScreenData.height;
    
    const originalX = scaledX * clientScaleX;
    const originalY = scaledY * clientScaleY;
    
    // 确保坐标在原始屏幕范围内
    const clampedX = Math.max(0, Math.min(originalX, lastScreenData.original_width - 1));
    const clampedY = Math.max(0, Math.min(originalY, lastScreenData.original_height - 1));

    const mouseEvent = {
        type: 'MouseEvent',
        x: Math.round(clampedX),
        y: Math.round(clampedY),
        button: 'none',
        event_type: 'move'
    };

    dataChannel.send(JSON.stringify(mouseEvent));
}

function handleKeyDown(event) {
    if (!isConnected || !dataChannel) return;
    
    // 只有当鼠标在canvas范围内时才处理键盘事件
    if (!window.mouseInCanvas) {
        console.log(`⌨️ 键盘事件被忽略: ${event.code} (鼠标不在canvas内)`);
        return;
    }

    event.preventDefault();

    // 使用 event.code 来保证按键位置的一致性
    // event.code 代表物理按键位置，不受键盘布局影响
    const keyboardEvent = {
        type: 'KeyboardEvent',
        key: event.code,
        event_type: 'press'
    };

    console.log(`⌨️ 键盘按下: ${event.code} -> "${event.key}" (鼠标在canvas内: ${window.mouseInCanvas})`);
    dataChannel.send(JSON.stringify(keyboardEvent));
}

function handleKeyUp(event) {
    if (!isConnected || !dataChannel) return;
    
    // 只有当鼠标在canvas范围内时才处理键盘事件
    if (!window.mouseInCanvas) {
        console.log(`⌨️ 键盘释放被忽略: ${event.code} (鼠标不在canvas内)`);
        return;
    }

    event.preventDefault();

    const keyboardEvent = {
        type: 'KeyboardEvent',
        key: event.code,
        event_type: 'release'
    };

    console.log(`⌨️ 键盘释放: ${event.code} -> "${event.key}" (鼠标在canvas内: ${window.mouseInCanvas})`);
    dataChannel.send(JSON.stringify(keyboardEvent));
}

function disconnect() {
    if (dataChannel) {
        dataChannel.close();
    }
    
    if (peerConnection) {
        peerConnection.close();
    }
    
    if (signalingSocket) {
        const disconnectMessage = {
            type: 'Disconnect'
        };
        signalingSocket.send(JSON.stringify(disconnectMessage));
        signalingSocket.close();
    }
    
    resetUI();
    showStatus('已断开WebRTC连接', 'success');
}

function resetUI() {
    isConnected = false;
    document.getElementById('loginSection').style.display = 'block';
    document.getElementById('remoteDesktop').classList.remove('active');
    document.getElementById('connectionInfo').style.display = 'none';
    document.getElementById('loading').style.display = 'block';
    
    // 清除输入框
    document.getElementById('clientId').value = '';
    document.getElementById('authCode').value = '';
    
    // 重置连接状态显示
    updateConnectionInfo('connectionState', '未连接');
    updateConnectionInfo('signalingState', '未连接');
    updateConnectionInfo('iceState', '未连接');
    updateConnectionInfo('dataChannelState', '未连接');
    
    // 重置性能监控
    performanceData.frameCount = 0;
    performanceData.startTime = Date.now();
    performanceData.renderTimes.length = 0;
    
    // 清理WebRTC对象
    peerConnection = null;
    dataChannel = null;
    signalingSocket = null;
}

// 判断是否是分片消息格式
function isChunkedMessage(arrayBuffer) {
    try {
        // 检查最小长度：至少需要2字节头长度 + 一些头数据
        if (arrayBuffer.byteLength < 4) {
            return false;
        }
        
        const dataView = new DataView(arrayBuffer);
        const headerLength = dataView.getUint16(0, true); // little-endian
        
        // 检查头长度是否合理
        if (headerLength <= 0 || headerLength > 1000 || headerLength >= arrayBuffer.byteLength) {
            return false;
        }
        
        // 检查是否有足够的数据包含头部
        if (arrayBuffer.byteLength < 2 + headerLength) {
            return false;
        }
        
        // 尝试解析头部JSON
        const headerBytes = new Uint8Array(arrayBuffer, 2, headerLength);
        const headerText = new TextDecoder().decode(headerBytes);
        const chunkHeader = JSON.parse(headerText);
        
        // 检查是否包含分片所需的字段
        return chunkHeader.hasOwnProperty('chunk_index') && 
               chunkHeader.hasOwnProperty('total_chunks') && 
               chunkHeader.hasOwnProperty('message_id') &&
               chunkHeader.hasOwnProperty('total_size');
    } catch (error) {
        // 解析失败，不是分片格式
        return false;
    }
}

// 处理分片消息
function handleChunkedMessage(arrayBuffer) {
    try {
        console.log(`🔍 开始处理分片消息，原始数据大小: ${arrayBuffer.byteLength} bytes`);
        
        const dataView = new DataView(arrayBuffer);
        
        // 🔧 增强调试：详细打印原始数据
        console.log('📋 原始数据前16字节:', Array.from(new Uint8Array(arrayBuffer, 0, Math.min(16, arrayBuffer.byteLength))).map(b => b.toString(16).padStart(2, '0')).join(' '));
        
        // 读取头长度（前2字节）
        if (arrayBuffer.byteLength < 2) {
            console.error('❌ 数据太短，无法读取头长度');
            return;
        }
        
        const headerLength = dataView.getUint16(0, true); // little-endian
        console.log(`📏 解析头长度: ${headerLength} bytes (原始字节: [${dataView.getUint8(0)}, ${dataView.getUint8(1)}])`);
        
        // 🔧 验证头长度是否合理
        if (headerLength <= 0 || headerLength > 1000) {
            console.error(`❌ 头长度异常: ${headerLength}, 可能数据损坏`);
            console.error('🔍 完整原始数据:', Array.from(new Uint8Array(arrayBuffer)).map(b => b.toString(16).padStart(2, '0')).join(' '));
            return;
        }
        
        // 验证数据完整性
        if (arrayBuffer.byteLength < 2 + headerLength) {
            console.error(`❌ 数据不完整，期望: ${2 + headerLength} bytes, 实际: ${arrayBuffer.byteLength} bytes`);
            return;
        }
        
        // 读取头数据
        console.log(`📦 准备读取头数据，偏移: 2, 长度: ${headerLength}`);
        const headerBytes = new Uint8Array(arrayBuffer, 2, headerLength);
        const headerText = new TextDecoder().decode(headerBytes);
        console.log(`📋 头部原始文本: ${headerText}`);
        
        const chunkHeader = JSON.parse(headerText);
        console.log(`✅ 解析的分片头:`, chunkHeader);
        
        // 读取分片数据
        const chunkDataOffset = 2 + headerLength;
        const chunkDataLength = arrayBuffer.byteLength - chunkDataOffset;
        console.log(`📦 准备读取分片数据，偏移: ${chunkDataOffset}, 长度: ${chunkDataLength}`);
        
        const chunkData = new Uint8Array(arrayBuffer, chunkDataOffset);
        console.log(`📊 实际分片数据大小: ${chunkData.length} bytes`);
        
        console.log(`📦 收到分片 ${chunkHeader.chunk_index + 1}/${chunkHeader.total_chunks}，头部: ${headerLength} bytes，数据: ${chunkData.length} bytes`);
        
        // 获取或创建分片缓冲区
        let chunkBuffer = chunkBuffers.get(chunkHeader.message_id);
        if (!chunkBuffer) {
            chunkBuffer = {
                chunks: new Array(chunkHeader.total_chunks),
                totalChunks: chunkHeader.total_chunks,
                totalSize: chunkHeader.total_size,
                receivedChunks: 0
            };
            chunkBuffers.set(chunkHeader.message_id, chunkBuffer);
            console.log(`🆕 创建新的分片缓冲区，消息ID: ${chunkHeader.message_id}，总分片数: ${chunkHeader.total_chunks}`);
        }
        
        // 存储当前分片
        if (!chunkBuffer.chunks[chunkHeader.chunk_index]) {
            chunkBuffer.chunks[chunkHeader.chunk_index] = chunkData;
            chunkBuffer.receivedChunks++;
            
            console.log(`📦 分片 ${chunkHeader.chunk_index + 1} 已存储，已收到 ${chunkBuffer.receivedChunks}/${chunkBuffer.totalChunks} 分片`);
        }
        
        // 检查是否接收完所有分片
        if (chunkBuffer.receivedChunks === chunkBuffer.totalChunks) {
            console.log('🔧 开始重组分片数据...');
            
            // 重组数据
            const totalBytes = new Uint8Array(chunkBuffer.totalSize);
            let offset = 0;
            
            for (let i = 0; i < chunkBuffer.totalChunks; i++) {
                const chunk = chunkBuffer.chunks[i];
                if (chunk) {
                    totalBytes.set(chunk, offset);
                    offset += chunk.length;
                } else {
                    console.error(`❌ 缺少分片 ${i + 1}`);
                    return;
                }
            }
            
            // 解析重组后的JSON数据
            const jsonText = new TextDecoder().decode(totalBytes);
            const message = JSON.parse(jsonText);
            
            console.log('✅ 分片重组完成，数据大小:', totalBytes.length, 'bytes');
            
            // 处理重组后的消息
            if (message.type === 'ScreenData') {
                const screenData = message;
                console.log('🖼️ 处理重组屏幕数据:', screenData.width + 'x' + screenData.height);
                handleScreenData(screenData);
            }
            
            // 清理缓冲区
            chunkBuffers.delete(chunkHeader.message_id);
        }
        
    } catch (error) {
        console.error('❌ 处理分片消息失败:', error);
    }
}

// 页面加载完成后的初始化
window.addEventListener('load', function() {
    console.log('🚀 WebRTC远程桌面控制页面已加载');
});

// 全屏状态变化监听器
document.addEventListener('fullscreenchange', function() {
    const button = document.querySelector('button[onclick="toggleFullscreen()"]');
    if (button) {
        if (document.fullscreenElement) {
            button.textContent = '🚪 退出全屏';
        } else {
            button.textContent = '🖥️ 全屏显示';
            // 退出全屏时重新渲染
            if (lastScreenData) {
                updateFullFrameAsync(lastScreenData, performance.now());
            }
        }
    }
}); 

// WebRTC连接统计信息
async function getConnectionStats() {
    if (!peerConnection) return;
    
    try {
        const stats = await peerConnection.getStats();
        console.group('📊 WebRTC连接统计');
        
        stats.forEach((report) => {
            if (report.type === 'candidate-pair' && report.state === 'succeeded') {
                console.log('✅ 成功的候选对:', {
                    localCandidateId: report.localCandidateId,
                    remoteCandidateId: report.remoteCandidateId,
                    state: report.state,
                    nominated: report.nominated,
                    bytesReceived: report.bytesReceived,
                    bytesSent: report.bytesSent
                });
            } else if (report.type === 'local-candidate') {
                console.log('📍 本地候选:', {
                    candidateType: report.candidateType,
                    ip: report.ip || report.address,
                    port: report.port,
                    protocol: report.protocol,
                    relayProtocol: report.relayProtocol
                });
            } else if (report.type === 'remote-candidate') {
                console.log('🌐 远程候选:', {
                    candidateType: report.candidateType,
                    ip: report.ip || report.address, 
                    port: report.port,
                    protocol: report.protocol
                });
            }
        });
        
        console.groupEnd();
    } catch (error) {
        console.error('❌ 获取连接统计失败:', error);
    }
} 

// 全屏显示切换
function toggleFullscreen() {
    const remoteDesktop = document.getElementById('remoteDesktop');
    
    if (!document.fullscreenElement) {
        // 进入全屏
        if (remoteDesktop.requestFullscreen) {
            remoteDesktop.requestFullscreen().then(() => {
                console.log('🖥️ 已进入全屏模式');
                showStatus('已进入全屏模式', 'success');
                
                // 更新按钮文本
                const button = document.querySelector('button[onclick="toggleFullscreen()"]');
                if (button) {
                    button.textContent = '🚪 退出全屏';
                }
                
                // 重新渲染以适应全屏尺寸
                if (lastScreenData) {
                    updateFullFrameAsync(lastScreenData, performance.now());
                }
            }).catch(error => {
                console.error('进入全屏失败:', error);
                showStatus('进入全屏失败', 'error');
            });
        } else {
            showStatus('浏览器不支持全屏功能', 'error');
        }
    } else {
        // 退出全屏
        document.exitFullscreen().then(() => {
            console.log('🚪 已退出全屏模式');
            showStatus('已退出全屏模式', 'success');
            
            // 更新按钮文本
            const button = document.querySelector('button[onclick="toggleFullscreen()"]');
            if (button) {
                button.textContent = '🖥️ 全屏显示';
            }
            
            // 重新渲染以适应窗口尺寸
            if (lastScreenData) {
                updateFullFrameAsync(lastScreenData, performance.now());
            }
        }).catch(error => {
            console.error('退出全屏失败:', error);
            showStatus('退出全屏失败', 'error');
        });
    }
} 