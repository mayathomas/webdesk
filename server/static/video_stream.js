 // 视频流处理模块 - 模仿Chrome Remote Desktop的视频接收

class VideoStreamReceiver {
    constructor() {
        this.remoteVideo = null;
        this.videoStats = {
            frameCount: 0,
            bytesReceived: 0,
            lastFrameTime: 0,
            fps: 0
        };
        this.isActive = false;
        this.canvas = null;
        this.ctx = null;
    }

    // 初始化视频接收器
    initialize(videoElement, canvasElement) {
        console.log('🎥 初始化视频流接收器');
        
        this.remoteVideo = videoElement;
        this.canvas = canvasElement;
        this.ctx = this.canvas.getContext('2d');
        
        // 设置视频事件监听器
        this.setupVideoEventListeners();
        
        return true;
    }

    // 设置视频事件监听器
    setupVideoEventListeners() {
        if (!this.remoteVideo) return;

        this.remoteVideo.onloadedmetadata = () => {
            console.log('📺 视频元数据加载完成:', {
                width: this.remoteVideo.videoWidth,
                height: this.remoteVideo.videoHeight,
                duration: this.remoteVideo.duration
            });
            
            // 调整canvas大小匹配视频
            this.updateCanvasSize();
        };

        this.remoteVideo.onplay = () => {
            console.log('▶️ 视频开始播放');
            this.isActive = true;
            this.startFrameRendering();
        };

        this.remoteVideo.onpause = () => {
            console.log('⏸️ 视频暂停');
            this.isActive = false;
        };

        this.remoteVideo.onerror = (error) => {
            console.error('❌ 视频播放错误:', error);
        };

        // 监听视频尺寸变化
        this.remoteVideo.onresize = () => {
            console.log('📐 视频尺寸变化:', {
                width: this.remoteVideo.videoWidth,
                height: this.remoteVideo.videoHeight
            });
            this.updateCanvasSize();
        };
    }

    // 更新Canvas大小以匹配视频
    updateCanvasSize() {
        if (!this.remoteVideo || !this.canvas) return;
        
        const videoWidth = this.remoteVideo.videoWidth;
        const videoHeight = this.remoteVideo.videoHeight;
        
        if (videoWidth && videoHeight) {
            // 保持宽高比的同时适配显示区域
            const containerWidth = this.canvas.parentElement.clientWidth;
            const containerHeight = this.canvas.parentElement.clientHeight;
            
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
            
            // 设置Canvas显示大小
            this.canvas.style.width = displayWidth + 'px';
            this.canvas.style.height = displayHeight + 'px';
            
            // 设置Canvas内部分辨率
            this.canvas.width = videoWidth;
            this.canvas.height = videoHeight;
            
            console.log('📐 Canvas大小已更新:', {
                video: `${videoWidth}x${videoHeight}`,
                display: `${displayWidth}x${displayHeight}`
            });
        }
    }

    // 开始帧渲染循环
    startFrameRendering() {
        const renderFrame = () => {
            if (!this.isActive || !this.remoteVideo || !this.canvas || !this.ctx) return;
            
            // 将视频帧绘制到Canvas
            try {
                this.ctx.drawImage(
                    this.remoteVideo, 
                    0, 0, 
                    this.canvas.width, 
                    this.canvas.height
                );
                
                // 更新统计信息
                this.updateStats();
                
            } catch (error) {
                console.error('❌ 渲染视频帧失败:', error);
            }
            
            // 继续下一帧
            if (this.isActive) {
                requestAnimationFrame(renderFrame);
            }
        };
        
        requestAnimationFrame(renderFrame);
    }

    // 更新统计信息
    updateStats() {
        const now = performance.now();
        this.videoStats.frameCount++;
        
        if (this.videoStats.lastFrameTime > 0) {
            const deltaTime = now - this.videoStats.lastFrameTime;
            this.videoStats.fps = 1000 / deltaTime;
        }
        
        this.videoStats.lastFrameTime = now;
        
        // 每秒输出一次统计信息
        if (this.videoStats.frameCount % 60 === 0) {
            console.log('📊 视频统计:', {
                帧数: this.videoStats.frameCount,
                FPS: this.videoStats.fps.toFixed(1),
                分辨率: `${this.canvas.width}x${this.canvas.height}`
            });
        }
    }

    // 获取Canvas元素用于鼠标事件
    getCanvas() {
        return this.canvas;
    }

    // 获取视频统计信息
    getStats() {
        return {
            ...this.videoStats,
            resolution: `${this.canvas?.width || 0}x${this.canvas?.height || 0}`,
            isActive: this.isActive
        };
    }

    // 停止视频接收
    stop() {
        console.log('🛑 停止视频流接收');
        this.isActive = false;
        
        if (this.remoteVideo) {
            this.remoteVideo.srcObject = null;
        }
        
        if (this.ctx && this.canvas) {
            this.ctx.clearRect(0, 0, this.canvas.width, this.canvas.height);
        }
    }
}

// 视频流WebRTC管理器
class VideoStreamWebRTC {
    constructor() {
        this.peerConnection = null;
        this.videoReceiver = new VideoStreamReceiver();
        this.remoteVideo = null;
        this.canvas = null;
        this.isConnected = false;
    }

    // 初始化WebRTC视频接收
    async initializeVideoReceiver(videoElement, canvasElement) {
        console.log('🔧 初始化WebRTC视频接收器');
        
        this.remoteVideo = videoElement;
        this.canvas = canvasElement;
        
        // 初始化视频接收器
        this.videoReceiver.initialize(videoElement, canvasElement);
        
        return true;
    }

    // 设置PeerConnection
    setPeerConnection(peerConnection) {
        this.peerConnection = peerConnection;
        this.setupPeerConnectionHandlers();
    }

    // 设置PeerConnection事件处理器
    setupPeerConnectionHandlers() {
        if (!this.peerConnection) return;

        // 监听远程流
        this.peerConnection.ontrack = (event) => {
            console.log('🎥 接收到远程视频轨道:', event.track.kind);
            
            if (event.track.kind === 'video') {
                console.log('📺 设置远程视频流');
                
                // 创建MediaStream并添加轨道
                const remoteStream = new MediaStream();
                remoteStream.addTrack(event.track);
                
                // 设置到video元素
                if (this.remoteVideo) {
                    this.remoteVideo.srcObject = remoteStream;
                    this.remoteVideo.play().catch(e => {
                        console.error('❌ 视频播放失败:', e);
                    });
                }
                
                this.isConnected = true;
                console.log('✅ 视频流连接成功');
            }
        };

        // 监听连接状态
        this.peerConnection.onconnectionstatechange = () => {
            console.log('🔗 WebRTC连接状态:', this.peerConnection.connectionState);
            
            if (this.peerConnection.connectionState === 'connected') {
                console.log('🎉 WebRTC视频连接建立！');
            } else if (this.peerConnection.connectionState === 'disconnected') {
                console.log('⚠️ WebRTC视频连接断开');
                this.isConnected = false;
            }
        };
    }

    // 获取Canvas用于事件处理
    getCanvas() {
        return this.videoReceiver.getCanvas();
    }

    // 获取统计信息
    getStats() {
        return this.videoReceiver.getStats();
    }

    // 停止连接
    stop() {
        console.log('🛑 停止WebRTC视频连接');
        this.isConnected = false;
        this.videoReceiver.stop();
    }
}

// 导出全局对象
window.VideoStreamReceiver = VideoStreamReceiver;
window.VideoStreamWebRTC = VideoStreamWebRTC;