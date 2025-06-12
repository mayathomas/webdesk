// H.264 WebRTC相关变量
let peerConnection = null;
let dataChannel = null;
let signalingSocket = null;
let currentClientId = null;
let isConnected = false;

// H.264视频流相关变量
let h264VideoStreamWebRTC = null;
let remoteVideo = null;

// 鼠标焦点状态
window.mouseInCanvas = false;

// 性能监控变量
let performanceData = {
    frameCount: 0,
    totalBytes: 0,
    startTime: Date.now(),
    renderTimes: []
};

// WebRTC配置 - 从API动态加载
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
    const videoQuality = document.getElementById('videoQuality').value;

    if (!clientId || !authCode) {
        showStatus('请输入客户端ID和验证码', 'error');
        return;
    }

    // 保存当前客户端ID
    currentClientId = clientId;

    showStatus('正在建立VP8视频连接...', 'info');
    document.getElementById('connectionInfo').style.display = 'block';

    try {
        // 1. 加载WebRTC配置
        await loadWebRTCConfig();
        
        // 2. 连接信令服务器
        await connectSignalingServer(clientId, authCode);
        
        // 3. 初始化VP8 WebRTC
        await initVP8WebRTC();
        
        // 4. 发送视频质量配置
        await sendVideoQualityConfig(clientId, videoQuality);
        
        // 5. 创建Offer
        await createOffer(clientId);
        
    } catch (error) {
        console.error('VP8连接失败:', error);
        showStatus(`VP8连接失败: ${error.message}`, 'error');
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
                showStatus('信令连接成功，准备H.264视频流...', 'info');
                signalingSocket.onmessage = originalOnMessage;
                resolve();
            } else if (message.type === 'Error') {
                reject(new Error(message.message));
            }
        };
    });
}

async function initVP8WebRTC() {
    console.log('🎬 初始化VP8 WebRTC...');
    console.log('📋 ICE服务器配置:', rtcConfiguration.iceServers);
    
    // 添加ICE传输策略 - 允许所有连接方式
    if (!rtcConfiguration.iceTransportPolicy) {
        rtcConfiguration.iceTransportPolicy = 'all';
        console.log('🔧 设置ICE传输策略为: all (允许所有连接方式)');
    }
    
    // 创建PeerConnection
    peerConnection = new RTCPeerConnection(rtcConfiguration);
    
    // 初始化VP8视频流WebRTC管理器
    h264VideoStreamWebRTC = new H264VideoStreamWebRTC(); // 重用现有类，后续可重命名
    remoteVideo = document.getElementById('remoteVideo');
    const canvas = document.getElementById('desktopCanvas');
    
    // 初始化VP8视频接收器
    await h264VideoStreamWebRTC.initializeVideoReceiver(remoteVideo, canvas);
    h264VideoStreamWebRTC.setPeerConnection(peerConnection);
    
    // 设置WebRTC事件监听器
    setupWebRTCHandlers();
    
    console.log('✅ VP8 WebRTC初始化完成');
}

async function sendVideoQualityConfig(clientId, quality) {
    const qualityConfigs = {
        'low-latency': { width: 1280, height: 720, fps: 30, bitrate: 1500000 },
        'standard': { width: 1920, height: 1080, fps: 25, bitrate: 2500000 },
        'high-quality': { width: 1920, height: 1080, fps: 30, bitrate: 4000000 }
    };
    
    const config = qualityConfigs[quality] || qualityConfigs['standard'];
    
    const videoConfigMessage = {
        type: 'VideoStreamConfig',
        target_id: clientId,
        config: {
            width: config.width,
            height: config.height,
            fps: config.fps,
            bitrate: config.bitrate,
            codec: 'H264'
        }
    };
    
    console.log('🎬 发送H.264视频流配置:', config);
    signalingSocket.send(JSON.stringify(videoConfigMessage));
}

function setupWebRTCHandlers() {
    // 监听连接状态变化
    peerConnection.onconnectionstatechange = function() {
        const state = peerConnection.connectionState;
        console.log('🔗 WebRTC连接状态:', state);
        updateConnectionInfo('connectionState', state);
        
        switch(state) {
            case 'connected':
                console.log('🎉 WebRTC P2P连接已建立，H.264视频流正在传输！');
                showStatus('H.264视频连接成功！', 'success');
                showRemoteDesktop();
                
                // 延迟1秒后自动诊断video元素状态
                setTimeout(() => {
                    diagnoseVideoElement();
                }, 1000);
                break;
            case 'connecting':
                showStatus('正在建立H.264连接...', 'info');
                break;
            case 'disconnected':
                showStatus('H.264连接已断开', 'warning');
                break;
            case 'failed':
                showStatus('H.264连接失败', 'error');
                break;
        }
    };

    // 监听ICE连接状态
    peerConnection.oniceconnectionstatechange = function() {
        const state = peerConnection.iceConnectionState;
        console.log('🧊 ICE连接状态:', state);
        updateConnectionInfo('iceState', state);
    };
    
    // 监听接收到的视频轨道
    peerConnection.ontrack = function(event) {
        console.log('🎥 收到视频轨道:', event.track.kind);
        console.log('🎥 视频轨道设置:', event.track.getSettings());
        if (event.track.kind === 'video') {
            const videoElement = document.getElementById('remoteVideo');
            if (videoElement) {
                console.log('📺 设置H.264视频流到video元素...');
                videoElement.srcObject = event.streams[0];
                
                // 确保视频开始播放
                videoElement.play().then(() => {
                    console.log('✅ H.264视频流开始播放');
                    // 隐藏加载提示
                    const loading = document.getElementById('loading');
                    if (loading) {
                        loading.style.display = 'none';
                    }
                    // 更新视频状态
                    updateConnectionInfo('videoState', '播放中');
                    
                    // 延迟500ms后诊断video元素状态
                    setTimeout(() => {
                        diagnoseVideoElement();
                    }, 500);
                }).catch(error => {
                    console.error('❌ H.264视频播放失败:', error);
                    // 尝试静音播放（浏览器策略要求）
                    videoElement.muted = true;
                    videoElement.play().then(() => {
                        console.log('✅ H.264视频流静音播放成功');
                        updateConnectionInfo('videoState', '静音播放');
                    }).catch(err => {
                        console.error('❌ 静音播放也失败:', err);
                        updateConnectionInfo('videoState', '播放失败');
                    });
                });
                
                console.log('✅ H.264视频流已连接到video元素');
            } else {
                console.error('❌ 找不到video元素');
            }
        }
    };
    
    // 监听ICE候选
    peerConnection.onicecandidate = function(event) {
        if (event.candidate) {
            console.log('🧊 发送ICE候选到信令服务器');
            const candidateMessage = {
                type: 'WebRTCIceCandidate',
                target_id: currentClientId,
                ice_candidate: {
                    candidate: event.candidate.candidate,
                    sdp_mid: event.candidate.sdpMid,
                    sdp_mline_index: event.candidate.sdpMLineIndex
                }
            };
            signalingSocket.send(JSON.stringify(candidateMessage));
        } else {
            console.log('🏁 ICE候选收集完成');
        }
    };

    console.log('✅ WebRTC事件处理器已设置');
}

async function createOffer(clientId) {
    console.log('📡 创建WebRTC Offer...');
    
    // 添加视频接收器 - 告诉客户端我们想要接收视频
    console.log('📹 添加视频接收器（H.264）');
    peerConnection.addTransceiver('video', {
        direction: 'recvonly'  // 我们只接收，不发送
    });
    
    // 创建数据通道 (用于输入事件)
    // dataChannel = peerConnection.createDataChannel('input', {
    //     ordered: true
    // });
    // setupDataChannel(dataChannel);
    
    try {
        const offer = await peerConnection.createOffer();
        await peerConnection.setLocalDescription(offer);
        
        // 分析生成的Offer SDP
        console.log('📤 生成的Offer SDP分析:');
        const offerHasVideo = offer.sdp.includes('m=video');
        const offerHasH264 = offer.sdp.includes('h264') || offer.sdp.includes('H264');
        console.log('🎬 Offer SDP检查结果:');
        console.log('  - 包含视频媒体:', offerHasVideo);
        console.log('  - 包含H.264编解码器:', offerHasH264);
        
        if (!offerHasVideo) {
            console.error('❌ 生成的Offer SDP中没有视频媒体描述！');
        }
        
        const offerMessage = {
            type: 'WebRTCOffer',
            target_id: clientId,
            session_description: {
                sdp_type: 'offer',
                sdp: offer.sdp
            }
        };
        
        console.log('📤 发送Offer到信令服务器');
        signalingSocket.send(JSON.stringify(offerMessage));
        
        showStatus('已发送H.264连接请求，等待响应...', 'info');
        
    } catch (error) {
        console.error('❌ 创建Offer失败:', error);
        showStatus('创建H.264连接请求失败', 'error');
        throw error;
    }
}

function setupDataChannel(channel) {
    console.log('📊 设置数据通道:', channel.label);
    
    channel.onopen = function() {
        console.log('🚀 数据通道已打开:', channel.label);
        updateConnectionInfo('dataChannelState', '已连接');
        initCanvas(); // 初始化鼠标交互
    };
    
    channel.onclose = function() {
        console.log('🔒 数据通道已关闭:', channel.label);
        updateConnectionInfo('dataChannelState', '已断开');
    };
    
    channel.onerror = function(error) {
        console.error('❌ 数据通道错误:', error);
        updateConnectionInfo('dataChannelState', '错误');
    };
    
    channel.onmessage = function(event) {
        console.log('📨 收到数据通道消息:', event.data);
    };
}

function handleSignalingMessage(message) {
    console.log('📥 处理信令消息:', message.type);
    
    switch(message.type) {
        case 'Connected':
            // 连接确认已在connectSignalingServer中处理
            break;
            
        case 'WebRTCAnswer':
            handleAnswer(message.session_description);
            break;
            
        case 'WebRTCIceCandidate':
            handleIceCandidate(message.ice_candidate);
            break;
            
        case 'Error':
            console.error('❌ 服务器错误:', message.message);
            showStatus(`服务器错误: ${message.message}`, 'error');
            break;
            
        default:
            console.log('🔍 未处理的消息类型:', message.type);
    }
}

async function handleAnswer(sessionDescription) {
    console.log('📥 收到WebRTC Answer');
    try {
        // 详细分析Answer SDP
        console.log('📥 Answer SDP内容:', sessionDescription.sdp);
        
        // 检查SDP中的媒体流信息 - 现在检查VP8而不是H.264
        const videoMedia = sessionDescription.sdp.includes('m=video');
        const vp8Codec = sessionDescription.sdp.includes('vp8') || sessionDescription.sdp.includes('VP8');
        const h264Codec = sessionDescription.sdp.includes('h264') || sessionDescription.sdp.includes('H264');
        const rtpMapVP8 = sessionDescription.sdp.match(/a=rtpmap:(\d+) [Vv][Pp]8\/90000/i);
        const rtpMapH264 = sessionDescription.sdp.match(/a=rtpmap:(\d+) [Hh]264\/90000/i);
        
        console.log('🎬 Answer SDP媒体分析:', {
            包含视频媒体: videoMedia,
            包含VP8编解码器: vp8Codec,
            包含H264编解码器: h264Codec,
            VP8_RTP映射: rtpMapVP8 ? rtpMapVP8[0] : '未找到',
            H264_RTP映射: rtpMapH264 ? rtpMapH264[0] : '未找到',
            SDP长度: sessionDescription.sdp.length
        });
        
        if (!videoMedia) {
            console.error('❌ Answer SDP中没有视频媒体描述！这就是黑屏的原因！');
        }
        if (!vp8Codec && !h264Codec) {
            console.error('❌ Answer SDP中没有VP8或H.264编解码器！');
        } else if (vp8Codec) {
            console.log('✅ 检测到VP8编解码器支持');
        } else if (h264Codec) {
            console.log('✅ 检测到H.264编解码器支持'); 
        }
        
        const answer = new RTCSessionDescription({
            type: 'answer',
            sdp: sessionDescription.sdp
        });
        
        await peerConnection.setRemoteDescription(answer);
        console.log('✅ 远程描述已设置');
        showStatus('VP8连接协商完成，等待视频流...', 'info');
        
    } catch (error) {
        console.error('❌ 处理Answer失败:', error);
        showStatus('处理连接响应失败', 'error');
    }
}

async function handleIceCandidate(iceCandidate) {
    console.log('🧊 收到ICE候选');
    try {
    const candidate = new RTCIceCandidate({
        candidate: iceCandidate.candidate,
        sdpMid: iceCandidate.sdp_mid,
        sdpMLineIndex: iceCandidate.sdp_mline_index
    });
    
    await peerConnection.addIceCandidate(candidate);
        console.log('✅ ICE候选已添加');

    } catch (error) {
        console.error('❌ 添加ICE候选失败:', error);
    }
}

function showRemoteDesktop() {
    console.log('🖥️ 显示H.264远程桌面');
    
    // 隐藏登录表单和加载提示
    document.getElementById('loginSection').style.display = 'none';
    document.getElementById('loading').style.display = 'none';
    
    // 显示远程桌面区域
    const remoteDesktop = document.getElementById('remoteDesktop');
    remoteDesktop.style.display = 'block';
    remoteDesktop.classList.add('active'); // 添加active类以正确显示flex布局
    
    // 调试：检查video元素状态
    const videoElement = document.getElementById('remoteVideo');
    if (videoElement) {
        console.log('📺 Video元素状态检查:', {
            存在: !!videoElement,
            显示状态: window.getComputedStyle(videoElement).display,
            可见性: window.getComputedStyle(videoElement).visibility,
            宽度: window.getComputedStyle(videoElement).width,
            高度: window.getComputedStyle(videoElement).height,
            位置: window.getComputedStyle(videoElement).position,
            zIndex: window.getComputedStyle(videoElement).zIndex,
            父容器: videoElement.parentElement?.tagName,
            srcObject: !!videoElement.srcObject,
            videoWidth: videoElement.videoWidth,
            videoHeight: videoElement.videoHeight,
            readyState: videoElement.readyState,
            paused: videoElement.paused
        });
        
        // 强制确保video元素可见
        videoElement.style.display = 'block';
        videoElement.style.visibility = 'visible';
        videoElement.style.opacity = '1';
    } else {
        console.error('❌ 找不到video元素！');
    }
    
    isConnected = true;
}

function initCanvas() {
    // 获取Canvas元素（现在用作鼠标交互层）
    canvas = h264VideoStreamWebRTC ? h264VideoStreamWebRTC.getCanvas() : document.getElementById('desktopCanvas');
    if (canvas) {
        ctx = canvas.getContext('2d');
        
        // 设置鼠标事件监听器
        canvas.addEventListener('mouseenter', handleMouseEnter);
        canvas.addEventListener('mouseleave', handleMouseLeave);
        canvas.addEventListener('mousemove', handleMouseMove);
        canvas.addEventListener('mousedown', handleMouseDown);
        canvas.addEventListener('mouseup', handleMouseUp);
        canvas.addEventListener('wheel', handleMouseWheel);
        
        // 设置键盘事件监听器
        canvas.setAttribute('tabindex', '0');
        canvas.addEventListener('keydown', handleKeyDown);
        canvas.addEventListener('keyup', handleKeyUp);
        
        console.log('✅ H.264视频交互Canvas已初始化');
    } else {
        console.error('❌ 无法找到Canvas元素');
    }
}

// 强制生成关键帧
function forceKeyframe() {
    if (!signalingSocket || !currentClientId) {
        showStatus('连接未建立', 'error');
        return;
    }
    
    const keyframeMessage = {
        type: 'ForceKeyframe',
        target_id: currentClientId
    };
    
    console.log('🔑 请求强制生成H.264关键帧');
    signalingSocket.send(JSON.stringify(keyframeMessage));
    showStatus('已请求生成关键帧', 'info');
}

// 动态调整视频质量
function changeVideoQuality() {
    if (!signalingSocket || !currentClientId) {
        showStatus('连接未建立', 'error');
        return;
    }
    
    // 弹出质量选择对话框
    const newQuality = prompt('选择新的视频质量:\n1 - 低延迟 (720p@30fps)\n2 - 标准质量 (1080p@25fps)\n3 - 高质量 (1080p@30fps)', '2');
    
    const qualityMap = {
        '1': 'low-latency',
        '2': 'standard', 
        '3': 'high-quality'
    };
    
    const quality = qualityMap[newQuality];
    if (!quality) {
        showStatus('无效的质量选择', 'error');
        return;
    }
    
    sendVideoQualityConfig(currentClientId, quality);
    showStatus(`已切换到${quality}质量`, 'info');
}

// 获取视频统计信息
function getVideoStats() {
    if (h264VideoStreamWebRTC) {
        const stats = h264VideoStreamWebRTC.getStats();
        console.log('📊 H.264视频流统计:', stats);
        
        // 显示统计信息面板
        const statsPanel = document.getElementById('videoStats');
        if (statsPanel.style.display === 'none') {
            statsPanel.style.display = 'block';
        } else {
            statsPanel.style.display = 'none';
        }
        
        showStatus('统计信息已更新', 'info');
    } else {
        showStatus('视频流未激活', 'error');
    }
}

// 诊断和修复video元素显示问题
function diagnoseVideoElement() {
    console.log('🔍 开始诊断video元素...');
    
    const videoElement = document.getElementById('remoteVideo');
    const desktopContainer = document.querySelector('.desktop-container');
    const remoteDesktop = document.getElementById('remoteDesktop');
    
    if (!videoElement) {
        console.error('❌ 找不到video元素！');
        return false;
    }
    
    // 检查容器状态
    console.log('📦 容器状态检查:', {
        remoteDesktop存在: !!remoteDesktop,
        remoteDesktop显示: remoteDesktop ? window.getComputedStyle(remoteDesktop).display : 'N/A',
        remoteDesktop类: remoteDesktop ? remoteDesktop.className : 'N/A',
        desktopContainer存在: !!desktopContainer,
        desktopContainer显示: desktopContainer ? window.getComputedStyle(desktopContainer).display : 'N/A'
    });
    
    // 检查video元素详细状态
    const computedStyle = window.getComputedStyle(videoElement);
    console.log('📺 Video元素详细状态:', {
        display: computedStyle.display,
        visibility: computedStyle.visibility,
        opacity: computedStyle.opacity,
        width: computedStyle.width,
        height: computedStyle.height,
        position: computedStyle.position,
        top: computedStyle.top,
        left: computedStyle.left,
        transform: computedStyle.transform,
        zIndex: computedStyle.zIndex,
        backgroundColor: computedStyle.backgroundColor,
        objectFit: computedStyle.objectFit,
        srcObject: !!videoElement.srcObject,
        videoWidth: videoElement.videoWidth,
        videoHeight: videoElement.videoHeight,
        readyState: videoElement.readyState,
        paused: videoElement.paused,
        muted: videoElement.muted,
        autoplay: videoElement.autoplay
    });
    
    // 尝试修复常见问题
    let fixed = false;
    
    if (computedStyle.display === 'none') {
        console.log('🔧 修复：设置display为block');
        videoElement.style.display = 'block';
        fixed = true;
    }
    
    if (computedStyle.visibility === 'hidden') {
        console.log('🔧 修复：设置visibility为visible');
        videoElement.style.visibility = 'visible';
        fixed = true;
    }
    
    if (computedStyle.opacity === '0') {
        console.log('🔧 修复：设置opacity为1');
        videoElement.style.opacity = '1';
        fixed = true;
    }
    
    if (computedStyle.width === '0px' || computedStyle.height === '0px') {
        console.log('🔧 修复：设置最小尺寸');
        videoElement.style.width = '100%';
        videoElement.style.height = '100%';
        videoElement.style.minWidth = '400px';
        videoElement.style.minHeight = '300px';
        fixed = true;
    }
    
    // 确保容器正确显示
    if (remoteDesktop && window.getComputedStyle(remoteDesktop).display === 'none') {
        console.log('🔧 修复：显示remoteDesktop容器');
        remoteDesktop.style.display = 'block';
        remoteDesktop.classList.add('active');
        fixed = true;
    }
    
    if (fixed) {
        console.log('✅ 已尝试修复video元素显示问题');
        showStatus('已尝试修复视频显示问题', 'info');
    } else {
        console.log('ℹ️ video元素状态正常，无需修复');
    }
    
    return true;
}

function toggleFullscreen() {
    const desktopContainer = document.querySelector('.desktop-container');
    
    if (!document.fullscreenElement) {
        desktopContainer.requestFullscreen().then(() => {
            console.log('🖥️ 进入H.264全屏模式');
            showStatus('已进入全屏模式', 'info');
        }).catch(err => {
            console.error('❌ 进入全屏失败:', err);
            showStatus('进入全屏失败', 'error');
        });
                } else {
        document.exitFullscreen().then(() => {
            console.log('🖥️ 退出H.264全屏模式');
            showStatus('已退出全屏模式', 'info');
        });
    }
}

function disconnect() {
    console.log('🔌 断开H.264连接');
    
    // 关闭WebRTC连接
    if (peerConnection) {
        peerConnection.close();
        console.log('🔐 H.264 WebRTC连接已关闭');
    }
    
    // 停止H.264视频流
    if (h264VideoStreamWebRTC) {
        h264VideoStreamWebRTC.stop();
        console.log('🛑 H.264视频流已停止');
    }
    
    // 关闭信令连接
    if (signalingSocket) {
        signalingSocket.close();
        console.log('📡 信令连接已断开');
    }
    
    // 重置UI
    resetUI();
    
    showStatus('H.264连接已断开', 'info');
}

function resetUI() {
    console.log('🔄 重置UI界面');
    
    // 显示登录表单
    document.getElementById('loginSection').style.display = 'block';
    
    // 隐藏远程桌面
    const remoteDesktop = document.getElementById('remoteDesktop');
    remoteDesktop.style.display = 'none';
    remoteDesktop.classList.remove('active'); // 移除active类
    
    // 隐藏连接信息
    document.getElementById('connectionInfo').style.display = 'none';
    
    // 重置全局变量
    peerConnection = null;
    dataChannel = null;
    signalingSocket = null;
    h264VideoStreamWebRTC = null;
    remoteVideo = null;
    currentClientId = null;
    isConnected = false;
    
    // 重置连接状态显示
    updateConnectionInfo('connectionState', '未连接');
    updateConnectionInfo('signalingState', '未连接');
    updateConnectionInfo('iceState', '未连接');
    updateConnectionInfo('dataChannelState', '未连接');
    updateConnectionInfo('videoState', '未连接');
    
    // 重置性能监控
    performanceData = {
        frameCount: 0,
        totalBytes: 0,
        startTime: Date.now(),
        renderTimes: []
    };
}

// 鼠标事件处理
function handleMouseEnter(event) {
    window.mouseInCanvas = true;
    console.log('🖱️ 鼠标进入H.264视频区域');
}

function handleMouseLeave(event) {
    window.mouseInCanvas = false;
    console.log('🖱️ 鼠标离开H.264视频区域');
}

function handleMouseMove(event) {
    if (!isConnected || !dataChannel) return;

    const rect = canvas.getBoundingClientRect();
    const x = event.clientX - rect.left;
    const y = event.clientY - rect.top;
    
    // 转换为视频原始坐标
    const videoX = (x / rect.width) * canvas.width;
    const videoY = (y / rect.height) * canvas.height;

    const mouseEvent = {
        type: 'MouseEvent',
        x: Math.round(videoX),
        y: Math.round(videoY),
        button: 'none',
        event_type: 'move'
    };

    dataChannel.send(JSON.stringify(mouseEvent));
}

function handleMouseDown(event) {
    if (!isConnected || !dataChannel) return;

    const rect = canvas.getBoundingClientRect();
    const x = event.clientX - rect.left;
    const y = event.clientY - rect.top;
    
    // 转换为视频原始坐标
    const videoX = (x / rect.width) * canvas.width;
    const videoY = (y / rect.height) * canvas.height;
    
    const button = event.button === 0 ? 'left' : (event.button === 2 ? 'right' : 'middle');

    const mouseEvent = {
        type: 'MouseEvent',
        x: Math.round(videoX),
        y: Math.round(videoY),
        button: button,
        event_type: 'press'
    };

    console.log(`🖱️ 鼠标按下: ${button} (${Math.round(videoX)}, ${Math.round(videoY)})`);
    dataChannel.send(JSON.stringify(mouseEvent));
}

function handleMouseUp(event) {
    if (!isConnected || !dataChannel) return;

    const rect = canvas.getBoundingClientRect();
    const x = event.clientX - rect.left;
    const y = event.clientY - rect.top;
    
    // 转换为视频原始坐标
    const videoX = (x / rect.width) * canvas.width;
    const videoY = (y / rect.height) * canvas.height;
    
    const button = event.button === 0 ? 'left' : (event.button === 2 ? 'right' : 'middle');

    const mouseEvent = {
        type: 'MouseEvent',
        x: Math.round(videoX),
        y: Math.round(videoY),
        button: button,
        event_type: 'release'
    };

    console.log(`🖱️ 鼠标释放: ${button} (${Math.round(videoX)}, ${Math.round(videoY)})`);
    dataChannel.send(JSON.stringify(mouseEvent));
}

function handleMouseWheel(event) {
    if (!isConnected || !dataChannel) return;
    
    event.preventDefault();

    const rect = canvas.getBoundingClientRect();
    const x = event.clientX - rect.left;
    const y = event.clientY - rect.top;
    
    // 转换为视频原始坐标
    const videoX = (x / rect.width) * canvas.width;
    const videoY = (y / rect.height) * canvas.height;
    
    const delta = Math.sign(event.deltaY) * -1; // 上滚为正，下滚为负

    const mouseEvent = {
        type: 'MouseEvent',
        x: Math.round(videoX),
        y: Math.round(videoY),
        button: 'none',
        event_type: 'scroll',
        scroll_delta: delta
    };

    console.log(`🎡 鼠标滚轮: ${delta} (${Math.round(videoX)}, ${Math.round(videoY)})`);
    dataChannel.send(JSON.stringify(mouseEvent));
}

// 键盘事件处理
function handleKeyDown(event) {
    if (!isConnected || !dataChannel || !window.mouseInCanvas) return;

    event.preventDefault();

    const keyboardEvent = {
        type: 'KeyboardEvent',
        key: event.code,
        event_type: 'press'
    };

    console.log(`⌨️ 键盘按下: ${event.code}`);
    dataChannel.send(JSON.stringify(keyboardEvent));
}

function handleKeyUp(event) {
    if (!isConnected || !dataChannel || !window.mouseInCanvas) return;

    event.preventDefault();

    const keyboardEvent = {
        type: 'KeyboardEvent',
        key: event.code,
        event_type: 'release'
    };

    console.log(`⌨️ 键盘释放: ${event.code}`);
    dataChannel.send(JSON.stringify(keyboardEvent));
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
    const desktopContainer = document.querySelector('.desktop-container');
    
            if (!document.fullscreenElement) {
            desktopContainer.requestFullscreen().then(() => {
            console.log('🖥️ 进入H.264全屏模式');
            showStatus('已进入全屏模式', 'info');
        }).catch(err => {
            console.error('❌ 进入全屏失败:', err);
                showStatus('进入全屏失败', 'error');
            });
        } else {
        document.exitFullscreen().then(() => {
            console.log('🖥️ 退出H.264全屏模式');
            showStatus('已退出全屏模式', 'info');
        });
    }
} 