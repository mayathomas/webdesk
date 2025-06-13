/**
 * H.264视频流处理模块
 * 专为WebRTC + H.264编码的远程桌面控制设计
 * 类似TeamViewer/Chrome Remote Desktop的高性能视频处理
 */

class H264VideoStreamReceiver {
    constructor() {
        this.remoteVideo = null;
        this.canvas = null;
        this.ctx = null;
        this.isActive = false;

        // H.264视频统计信息
        this.videoStats = {
            codec: 'H.264',
            frameCount: 0,
            bytesReceived: 0,
            lastFrameTime: 0,
            fps: 0,
            bitrate: 0,
            resolution: '0x0',
            latency: 0,
            packetLoss: 0,
            keyFrameCount: 0,
            startTime: Date.now()
        };

        // WebRTC统计收集器
        this.rtcStatsCollector = null;
        this.statsInterval = null;

        // 性能监控
        this.performanceMonitor = {
            renderTimes: [],
            averageRenderTime: 0,
            maxRenderTime: 0,
            decodingErrors: 0
        };
    }

    /**
     * 初始化H.264视频接收器
     * @param {HTMLVideoElement} videoElement - 视频显示元素
     * @param {HTMLCanvasElement} canvasElement - 鼠标交互画布
     * @param {RTCPeerConnection} peerConnection - WebRTC连接
     */
    initialize(videoElement, canvasElement, peerConnection) {
        console.log('🎬 初始化H.264视频流接收器...');

        this.remoteVideo = videoElement;
        this.canvas = canvasElement;
        this.ctx = this.canvas.getContext('2d');
        this.rtcStatsCollector = peerConnection;

        // 设置视频元素属性以优化H.264解码
        this.setupVideoElementForH264();

        // 设置视频事件监听器
        this.setupVideoEventListeners();

        // 启动统计信息收集
        this.startStatsCollection();

        console.log('✅ H.264视频流接收器初始化完成');
        return true;
    }

    /**
     * 优化视频元素以支持H.264解码
     */
    setupVideoElementForH264() {
        if (!this.remoteVideo) return;

        // 设置视频属性以优化性能和兼容性
        this.remoteVideo.setAttribute('playsinline', 'true');
        this.remoteVideo.setAttribute('autoplay', 'true');
        this.remoteVideo.setAttribute('muted', 'true');

        // 禁用视频控件
        this.remoteVideo.controls = false;

        // 设置预加载策略为auto以确保视频能正常播放
        this.remoteVideo.preload = 'auto';

        // 强制设置跨域属性
        this.remoteVideo.crossOrigin = 'anonymous';

        // 设置缓冲策略
        this.remoteVideo.setAttribute('buffered', 'true');

        // 启用硬件加速 (如果可用)
        if ('requestVideoFrameCallback' in this.remoteVideo) {
            console.log('🚀 检测到硬件加速支持 (requestVideoFrameCallback)');
        }

        // 强制视频解码参数
        this.remoteVideo.style.objectFit = 'contain';
        this.remoteVideo.style.backgroundColor = '#000000';

        // 设置初始显示样式
        this.remoteVideo.style.width = '100%';
        this.remoteVideo.style.height = '100%';
        this.remoteVideo.style.display = 'block';
        this.remoteVideo.style.position = 'relative';
        this.remoteVideo.style.zIndex = '1';

        console.log('🔧 视频元素已优化为H.264解码');
    }

    /**
     * 设置视频事件监听器
     */
    setupVideoEventListeners() {
        if (!this.remoteVideo) return;

        // 元数据加载完成
        this.remoteVideo.onloadedmetadata = () => {
            const width = this.remoteVideo.videoWidth;
            const height = this.remoteVideo.videoHeight;

            console.log('📺 H.264视频元数据加载完成:', {
                分辨率: `${width}x${height}`,
                持续时间: this.remoteVideo.duration,
                编解码器: 'H.264'
            });

            this.videoStats.resolution = `${width}x${height}`;
            this.updateCanvasSize();
            this.updateVideoState('已连接');
        };

        // 视频开始播放
        this.remoteVideo.onplay = () => {
            console.log('▶️ H.264视频开始播放');
            this.isActive = true;
            this.startFrameRendering();
            this.updateVideoState('播放中');
        };

        // 视频暂停
        this.remoteVideo.onpause = () => {
            console.log('⏸️ H.264视频暂停');
            this.isActive = false;
            this.updateVideoState('暂停');
        };

        // 视频结束
        this.remoteVideo.onended = () => {
            console.log('🔚 H.264视频流结束');
            this.isActive = false;
            this.updateVideoState('已结束');
        };

        // 视频错误
        this.remoteVideo.onerror = (error) => {
            console.error('❌ H.264视频解码错误:', error);
            this.performanceMonitor.decodingErrors++;
            this.updateVideoState('解码错误');
        };

        // 视频尺寸变化
        this.remoteVideo.onresize = () => {
            const width = this.remoteVideo.videoWidth;
            const height = this.remoteVideo.videoHeight;

            console.log('📐 H.264视频尺寸变化:', `${width}x${height}`);
            this.videoStats.resolution = `${width}x${height}`;
            this.updateCanvasSize();
        };

        // 视频等待数据
        this.remoteVideo.onwaiting = () => {
            console.log('⏳ H.264视频等待数据...');
            this.updateVideoState('缓冲中');
        };

        // 视频可以播放
        this.remoteVideo.oncanplay = () => {
            console.log('✅ H.264视频准备就绪');
            this.updateVideoState('准备就绪');
        };
    }

    /**
     * 更新Canvas大小以匹配视频
     */
    updateCanvasSize() {
        if (!this.remoteVideo || !this.canvas) return;

        const videoWidth = this.remoteVideo.videoWidth;
        const videoHeight = this.remoteVideo.videoHeight;

        if (videoWidth && videoHeight) {
            // 获取容器大小
            const containerRect = this.canvas.parentElement.getBoundingClientRect();
            const containerWidth = containerRect.width;
            const containerHeight = containerRect.height;

            // 计算保持宽高比的显示尺寸
            const videoAspect = videoWidth / videoHeight;
            const containerAspect = containerWidth / containerHeight;

            let displayWidth, displayHeight;

            if (videoAspect > containerAspect) {
                // 视频更宽，以宽度为准
                displayWidth = containerWidth;
                displayHeight = containerWidth / videoAspect;
            } else {
                // 视频更高，以高度为准
                displayHeight = containerHeight;
                displayWidth = containerHeight * videoAspect;
            }

            // 设置Canvas显示样式
            this.canvas.style.width = displayWidth + 'px';
            this.canvas.style.height = displayHeight + 'px';
            this.canvas.style.position = 'absolute';
            this.canvas.style.top = '50%';
            this.canvas.style.left = '50%';
            this.canvas.style.transform = 'translate(-50%, -50%)';

            // 设置Canvas内部分辨率 (用于精确的鼠标坐标映射)
            this.canvas.width = videoWidth;
            this.canvas.height = videoHeight;

            // 同时调整视频元素大小
            this.remoteVideo.style.width = displayWidth + 'px';
            this.remoteVideo.style.height = displayHeight + 'px';
            this.remoteVideo.style.position = 'absolute';
            this.remoteVideo.style.top = '50%';
            this.remoteVideo.style.left = '50%';
            this.remoteVideo.style.transform = 'translate(-50%, -50%)';
            this.remoteVideo.style.zIndex = '1';
            this.remoteVideo.style.display = 'block';

            console.log('📐 Canvas和视频大小已更新:', {
                原始分辨率: `${videoWidth}x${videoHeight}`,
                显示大小: `${Math.round(displayWidth)}x${Math.round(displayHeight)}`,
                容器大小: `${Math.round(containerWidth)}x${Math.round(containerHeight)}`
            });
        }
    }

    /**
     * 开始帧渲染循环 (主要用于性能监控)
     */
    startFrameRendering() {
        const renderFrame = (timestamp) => {
            if (!this.isActive || !this.remoteVideo) return;

            const startTime = performance.now();

            // 更新统计信息
            this.updateVideoStats();

            // 记录渲染时间
            const renderTime = performance.now() - startTime;
            this.recordRenderTime(renderTime);

            // 继续下一帧
            if (this.isActive) {
                requestAnimationFrame(renderFrame);
            }
        };

        requestAnimationFrame(renderFrame);
    }

    /**
     * 更新视频统计信息
     */
    updateVideoStats() {
        const now = performance.now();
        this.videoStats.frameCount++;

        // 计算FPS
        if (this.videoStats.lastFrameTime > 0) {
            const deltaTime = now - this.videoStats.lastFrameTime;
            this.videoStats.fps = 1000 / deltaTime;
        }

        this.videoStats.lastFrameTime = now;

        // 每60帧更新一次显示的统计信息
        if (this.videoStats.frameCount % 60 === 0) {
            this.updateStatsDisplay();
        }
    }

    /**
     * 启动WebRTC统计信息收集
     */
    startStatsCollection() {
        if (!this.rtcStatsCollector) return;

        this.statsInterval = setInterval(async () => {
            try {
                const stats = await this.rtcStatsCollector.getStats();
                this.processRTCStats(stats);
            } catch (error) {
                console.error('❌ 获取WebRTC统计失败:', error);
            }
        }, 1000); // 每秒更新一次

        console.log('📊 WebRTC统计信息收集已启动');
    }

    /**
     * 处理WebRTC统计数据
     */
    processRTCStats(stats) {
        stats.forEach((report) => {
            if (report.type === 'inbound-rtp' && report.mediaType === 'video') {
                // 视频接收统计
                if (report.bytesReceived !== undefined) {
                    const currentBytes = report.bytesReceived - this.videoStats.bytesReceived;
                    this.videoStats.bitrate = (currentBytes * 8) / 1000; // kbps
                    this.videoStats.bytesReceived = report.bytesReceived;
                }

                if (report.packetsLost !== undefined && report.packetsReceived !== undefined) {
                    const totalPackets = report.packetsLost + report.packetsReceived;
                    this.videoStats.packetLoss = totalPackets > 0 ?
                        (report.packetsLost / totalPackets * 100) : 0;
                }

                if (report.keyFramesDecoded !== undefined) {
                    this.videoStats.keyFrameCount = report.keyFramesDecoded;
                }
            } else if (report.type === 'remote-candidate') {
                // 网络延迟信息
                if (report.currentRoundTripTime !== undefined) {
                    this.videoStats.latency = report.currentRoundTripTime * 1000; // 转换为毫秒
                }
            }
        });
    }

    /**
     * 更新统计信息显示
     */
    updateStatsDisplay() {
        const elements = {
            resolution: document.getElementById('resolution'),
            framerate: document.getElementById('framerate'),
            bitrate: document.getElementById('bitrate'),
            latency: document.getElementById('latency'),
            packetLoss: document.getElementById('packetLoss')
        };

        if (elements.resolution) elements.resolution.textContent = this.videoStats.resolution;
        if (elements.framerate) elements.framerate.textContent = `${this.videoStats.fps.toFixed(1)} fps`;
        if (elements.bitrate) elements.bitrate.textContent = `${(this.videoStats.bitrate / 1000).toFixed(1)} Mbps`;
        if (elements.latency) elements.latency.textContent = `${this.videoStats.latency.toFixed(1)} ms`;
        if (elements.packetLoss) elements.packetLoss.textContent = `${this.videoStats.packetLoss.toFixed(2)}%`;
    }

    /**
     * 更新视频状态显示
     */
    updateVideoState(state) {
        const element = document.getElementById('videoState');
        if (element) {
            element.textContent = state;
        }
    }

    /**
     * 记录渲染时间 (性能监控)
     */
    recordRenderTime(time) {
        this.performanceMonitor.renderTimes.push(time);

        // 保持最近100个记录
        if (this.performanceMonitor.renderTimes.length > 100) {
            this.performanceMonitor.renderTimes.shift();
        }

        // 计算平均渲染时间
        this.performanceMonitor.averageRenderTime =
            this.performanceMonitor.renderTimes.reduce((a, b) => a + b, 0) /
            this.performanceMonitor.renderTimes.length;

        // 更新最大渲染时间
        this.performanceMonitor.maxRenderTime = Math.max(
            this.performanceMonitor.maxRenderTime,
            time
        );
    }

    /**
     * 获取Canvas元素 (用于鼠标事件)
     */
    getCanvas() {
        return this.canvas;
    }

    /**
     * 获取完整的统计信息
     */
    getStats() {
        return {
            ...this.videoStats,
            performance: {
                ...this.performanceMonitor,
                averageRenderTime: this.performanceMonitor.averageRenderTime.toFixed(2) + 'ms',
                maxRenderTime: this.performanceMonitor.maxRenderTime.toFixed(2) + 'ms'
            },
            uptime: ((Date.now() - this.videoStats.startTime) / 1000).toFixed(1) + 's'
        };
    }

    /**
     * 停止视频接收
     */
    stop() {
        console.log('🛑 停止H.264视频流接收');
        this.isActive = false;

        // 清理统计收集器
        if (this.statsInterval) {
            clearInterval(this.statsInterval);
            this.statsInterval = null;
        }

        // 清理视频元素
        if (this.remoteVideo) {
            this.remoteVideo.srcObject = null;
            this.remoteVideo.load(); // 重置视频元素
        }

        // 清理Canvas
        if (this.ctx && this.canvas) {
            this.ctx.clearRect(0, 0, this.canvas.width, this.canvas.height);
        }

        this.updateVideoState('已断开');
        console.log('✅ H.264视频流接收器已停止');
    }

    /**
     * 重试视频设置 (处理Chrome H.264黑屏问题)
     */
    retryVideoSetup(stream) {
        if (!this.remoteVideo || !stream) return;

        console.log('🔄 重试设置H.264视频流...');

        // 暂停当前视频
        this.remoteVideo.pause();

        // 清空当前流
        this.remoteVideo.srcObject = null;

        // 等待一段时间后重新设置
        setTimeout(() => {
            console.log('🔄 重新应用视频流...');
            this.remoteVideo.srcObject = stream;

            // 强制重新播放
            this.remoteVideo.load(); // 重载video元素

            this.remoteVideo.play().then(() => {
                console.log('✅ 重试成功：视频流已恢复');
            }).catch(err => {
                console.error('❌ 重试失败:', err);
                // 最后一次尝试：强制重新创建video元素
                this.recreateVideoElement(stream);
            });
        }, 500);
    }

    /**
     * 重新创建video元素 (最后的修复手段)
     */
    recreateVideoElement(stream) {
        if (!this.remoteVideo) return;

        console.log('🔄 重新创建video元素...');

        const parent = this.remoteVideo.parentElement;
        const oldVideo = this.remoteVideo;

        // 创建新的video元素
        const newVideo = document.createElement('video');
        newVideo.id = oldVideo.id;
        newVideo.className = oldVideo.className;

        // 复制样式
        newVideo.style.cssText = oldVideo.style.cssText;

        // 替换元素
        parent.replaceChild(newVideo, oldVideo);
        this.remoteVideo = newVideo;

        // 重新设置video元素
        this.setupVideoElementForH264();
        this.setupVideoEventListeners();

        // 设置流
        this.remoteVideo.srcObject = stream;
        this.remoteVideo.play().then(() => {
            console.log('✅ 重新创建video元素成功');
        }).catch(err => {
            console.error('❌ 重新创建video元素失败:', err);
        });
    }
}

/**
 * H.264视频流WebRTC管理器
 * 负责WebRTC连接和H.264视频轨道管理
 */
class H264VideoStreamWebRTC {
    constructor() {
        this.peerConnection = null;
        this.videoReceiver = new H264VideoStreamReceiver();
        this.remoteVideo = null;
        this.canvas = null;
        this.remoteStream = null;
    }

    /**
     * 初始化H.264视频接收器
     */
    async initializeVideoReceiver(videoElement, canvasElement) {
        console.log('🎬 初始化H.264 WebRTC视频接收器...');

        this.remoteVideo = videoElement;
        this.canvas = canvasElement;

        // 等待PeerConnection设置完成
        if (this.peerConnection) {
            this.videoReceiver.initialize(videoElement, canvasElement, this.peerConnection);
        }

        return true;
    }

    /**
     * 设置PeerConnection引用
     */
    setPeerConnection(peerConnection) {
        this.peerConnection = peerConnection;

        if (this.remoteVideo && this.canvas) {
            this.videoReceiver.initialize(this.remoteVideo, this.canvas, peerConnection);
        }

        this.setupPeerConnectionHandlers();
    }

    /**
     * 设置PeerConnection事件处理器
     */
    setupPeerConnectionHandlers() {
        if (!this.peerConnection) return;

        // 处理远程媒体流
        this.peerConnection.ontrack = (event) => {
            console.log('📡 收到远程H.264视频轨道:', event.track.kind);
            console.log('📺 轨道详细信息:', {
                kind: event.track.kind,
                id: event.track.id,
                label: event.track.label,
                enabled: event.track.enabled,
                readyState: event.track.readyState
            });

            if (event.track.kind === 'video') {
                const stream = event.streams[0];
                this.remoteStream = stream;

                console.log('🎬 媒体流信息:', {
                    id: stream.id,
                    active: stream.active,
                    tracks: stream.getTracks().length
                });

                // 设置视频源
                if (this.remoteVideo) {
                    // 确保先清空之前的流
                    this.remoteVideo.srcObject = null;

                    // 等待一帧后设置新流
                    requestAnimationFrame(() => {
                        this.remoteVideo.srcObject = stream;
                        console.log('✅ H.264视频流已连接到播放器');

                        // 多重播放尝试策略
                        const playVideo = async () => {
                            try {
                                // 强制设置播放参数
                                this.remoteVideo.volume = 0; // 静音播放
                                this.remoteVideo.muted = true;

                                await this.remoteVideo.play();
                                console.log('▶️ 视频播放开始');

                                // 检查视频是否真的在播放
                                setTimeout(() => {
                                    if (this.remoteVideo.videoWidth > 0 && this.remoteVideo.videoHeight > 0) {
                                        console.log('✅ 视频解码成功:', `${this.remoteVideo.videoWidth}x${this.remoteVideo.videoHeight}`);
                                    } else {
                                        console.warn('⚠️ 视频元素无尺寸信息，可能解码失败');
                                        // 尝试重新设置流
                                        this.retryVideoSetup(stream);
                                    }
                                }, 1000);

                            } catch (err) {
                                console.error('❌ 视频播放失败:', err);

                                if (err.name === 'NotAllowedError') {
                                    console.log('🔧 尝试解决自动播放限制...');
                                    // 用户交互后重试
                                    document.addEventListener('click', () => {
                                        this.remoteVideo.play().then(() => {
                                            console.log('✅ 用户交互后视频播放成功');
                                        }).catch(e => console.error('❌ 用户交互后视频播放仍失败:', e));
                                    }, { once: true });
                                } else {
                                    // 其他错误，尝试重新设置
                                    this.retryVideoSetup(stream);
                                }
                            }
                        };

                        playVideo();
                    });
                } else {
                    console.error('❌ 远程视频元素不存在');
                }

                // 监听轨道状态
                event.track.onended = () => {
                    console.log('🔚 H.264视频轨道已结束');
                    this.videoReceiver.updateVideoState('轨道结束');
                };

                event.track.onmute = () => {
                    console.log('🔇 H.264视频轨道已静音');
                    this.videoReceiver.updateVideoState('轨道静音');

                    // 给Chrome一些时间来恢复
                    setTimeout(() => {
                        if (event.track.muted && this.remoteVideo && this.remoteVideo.srcObject) {
                            console.log('🔄 尝试恢复静音的视频轨道...');
                            this.retryVideoSetup(this.remoteVideo.srcObject);
                        }
                    }, 2000);
                };

                event.track.onunmute = () => {
                    console.log('🔊 H.264视频轨道已取消静音');
                    this.videoReceiver.updateVideoState('播放中');
                };
            }
        };

        // 监听SDP协商过程
        this.peerConnection.onnegotiationneeded = () => {
            console.log('🤝 需要重新协商SDP');
        };

        // 监听信令状态变化
        this.peerConnection.onsignalingstatechange = () => {
            console.log('📡 信令状态变化:', this.peerConnection.signalingState);

            // 在SDP协商完成后检查编解码器
            if (this.peerConnection.signalingState === 'stable') {
                this.logCodecInformation();
            }
        };
        this.peerConnection.onconnectionstatechange = () => {
            console.log('connectionState:', peerConnection.connectionState);
        };
        this.peerConnection.oniceconnectionstatechange = () => {
            console.log('iceConnectionState:', this.peerConnection.iceConnectionState);
        };

        console.log('✅ H.264 WebRTC事件处理器已设置');
    }

    /**
     * 记录编解码器协商信息
     */
    logCodecInformation() {
        if (!this.peerConnection) return;

        console.log('🎥 检查H.264编解码器协商结果...');

        // 获取所有收发器
        const transceivers = this.peerConnection.getTransceivers();
        transceivers.forEach((transceiver, index) => {
            console.log(`📡 收发器 ${index}:`, {
                kind: transceiver.receiver.track?.kind,
                direction: transceiver.direction,
                currentDirection: transceiver.currentDirection
            });

            // 检查接收器的编解码器
            if (transceiver.receiver && transceiver.receiver.track?.kind === 'video') {
                const params = transceiver.receiver.getParameters();
                console.log('📥 视频接收器参数:', params);

                if (params.codecs) {
                    params.codecs.forEach((codec, idx) => {
                        console.log(`🎬 编解码器 ${idx}:`, {
                            mimeType: codec.mimeType,
                            clockRate: codec.clockRate,
                            sdpFmtpLine: codec.sdpFmtpLine
                        });

                        // 特别关注H.264编解码器
                        if (codec.mimeType?.toLowerCase().includes('h264')) {
                            console.log('✅ 发现H.264编解码器:', codec);
                        }
                    });
                }
            }
        });

        // 也检查SDP
        const localDesc = this.peerConnection.localDescription;
        const remoteDesc = this.peerConnection.remoteDescription;

        if (localDesc && localDesc.sdp.includes('H264')) {
            console.log('📤 本地SDP包含H.264编解码器');
        }
        if (remoteDesc && remoteDesc.sdp.includes('H264')) {
            console.log('📥 远程SDP包含H.264编解码器');
        }

        if (!localDesc?.sdp.includes('H264') && !remoteDesc?.sdp.includes('H264')) {
            console.error('❌ SDP中未找到H.264编解码器！');
        }
    }

    /**
     * 获取Canvas元素 (用于鼠标事件)
     */
    getCanvas() {
        return this.videoReceiver.getCanvas();
    }

    /**
     * 获取统计信息
     */
    getStats() {
        return this.videoReceiver.getStats();
    }

    /**
     * 停止视频流
     */
    stop() {
        console.log('🛑 停止H.264 WebRTC视频流');

        this.videoReceiver.stop();

        if (this.remoteStream) {
            this.remoteStream.getTracks().forEach(track => track.stop());
            this.remoteStream = null;
        }

        this.peerConnection = null;
        console.log('✅ H.264 WebRTC视频流已停止');
    }

    /**
     * 处理接收到的Offer SDP
     */
    async handleOffer(offer) {
        try {
            console.log('📩 处理H.264 Offer SDP');

            // 设置远程描述
            await this.peerConnection.setRemoteDescription(offer);
            console.log('✅ 远程描述已设置 (Offer)');

            // 在创建Answer之前，详细检查Offer
            console.log('📋 详细分析接收到的Offer SDP:');
            const sdpLines = offer.sdp.split('\n');
            let hasVideoMedia = false;
            let hasH264Support = false;
            let videoMediaIndex = -1;

            for (let i = 0; i < sdpLines.length; i++) {
                const line = sdpLines[i].trim();

                if (line.startsWith('m=video')) {
                    hasVideoMedia = true;
                    videoMediaIndex = i;
                    console.log(`  ✅ 发现视频媒体描述 (行${i + 1}): ${line}`);
                }

                if (line.includes('H264') || line.includes('h264')) {
                    hasH264Support = true;
                    console.log(`  ✅ 发现H.264编解码器支持 (行${i + 1}): ${line}`);
                }

                if (line.startsWith('a=rtpmap:') && line.includes('H264')) {
                    console.log(`  📊 H.264 RTP映射 (行${i + 1}): ${line}`);
                }

                if (line.startsWith('a=fmtp:') && line.includes('H264')) {
                    console.log(`  ⚙️ H.264格式参数 (行${i + 1}): ${line}`);
                }
            }

            console.log(`📈 SDP分析结果: 视频媒体=${hasVideoMedia}, H.264支持=${hasH264Support}`);

            if (!hasVideoMedia) {
                console.error('❌ 致命错误：Offer SDP中没有视频媒体描述！');
                console.error('💡 这通常表示客户端没有正确添加video transceiver');
                throw new Error('Offer SDP中没有视频媒体描述');
            }

            if (!hasH264Support) {
                console.warn('⚠️ 警告：Offer SDP中没有发现H.264编解码器支持');
            }

            // 创建Answer
            const answer = await this.peerConnection.createAnswer();
            console.log('📤 创建Answer SDP');

            // 分析Answer SDP
            console.log('📋 详细分析创建的Answer SDP:');
            const answerLines = answer.sdp.split('\n');
            let answerHasVideo = false;
            let answerHasH264 = false;

            for (let i = 0; i < answerLines.length; i++) {
                const line = answerLines[i].trim();

                if (line.startsWith('m=video')) {
                    answerHasVideo = true;
                    console.log(`  ✅ Answer包含视频媒体 (行${i + 1}): ${line}`);
                }

                if (line.includes('H264') || line.includes('h264')) {
                    answerHasH264 = true;
                    console.log(`  ✅ Answer支持H.264 (行${i + 1}): ${line}`);
                }
            }

            console.log(`📈 Answer SDP分析: 视频媒体=${answerHasVideo}, H.264支持=${answerHasH264}`);

            if (!answerHasVideo) {
                console.error('❌ 严重错误：Answer SDP中没有视频媒体！');
                throw new Error('Answer SDP中没有视频媒体');
            }

            if (!answerHasH264) {
                console.error('❌ 严重错误：Answer SDP中没有H.264支持！');
            }

            // 设置本地描述
            await this.peerConnection.setLocalDescription(answer);
            console.log('✅ 本地描述已设置 (Answer)');

            return answer;

        } catch (error) {
            console.error('❌ 处理Offer失败:', error);
            throw error;
        }
    }
}

// 导出给script.js使用
window.H264VideoStreamWebRTC = H264VideoStreamWebRTC; 