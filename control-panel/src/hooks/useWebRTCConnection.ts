import { useCallback, useEffect, useRef, useState, useLayoutEffect } from 'react'
import type { ConnectionState, VideoConfig, WebRTCConfig } from '@/types'

// 克隆 MediaStream，保证每次都是新引用 → React 一定会重新渲染
function cloneMediaStream(src: MediaStream): MediaStream {
  const clone = new MediaStream();
  src.getTracks().forEach(t => clone.addTrack(t));
  return clone;
}

interface UseWebRTCConnectionProps {
  onConnectionStateChange: (updater: (prev: ConnectionState) => ConnectionState) => void
  onConnected: () => void
  onDisconnected: () => void
  onStream: (stream: MediaStream) => void
}

export function useWebRTCConnection({
  onConnectionStateChange,
  onConnected,
  onDisconnected,
  onStream = () => {}
}: UseWebRTCConnectionProps) {
  const [error, setError] = useState<string | null>(null)
  const [webrtcConfig, setWebrtcConfig] = useState<WebRTCConfig | null>(null)
  
  const peerConnectionRef = useRef<RTCPeerConnection | null>(null)
  const dataChannelRef = useRef<RTCDataChannel | null>(null)
  const websocketRef = useRef<WebSocket | null>(null)
  const videoRef = useRef<HTMLVideoElement>(null)
  const remoteStreamRef = useRef<MediaStream | null>(null)
  const videoReceiverRef = useRef<RTCRtpReceiver | null>(null)
  const pendingCandidatesRef = useRef<RTCIceCandidateInit[]>([])
  const connectedRef = useRef(false)
  // 缓存最新的 onStream，避免组件重挂载导致旧回调失效
  const onStreamRef = useRef(onStream)
  useEffect(() => {
    onStreamRef.current = onStream
  }, [onStream])

  // 公共绑定函数：将远程流绑定到video元素
  const bindStreamToVideo = useCallback((retry = 0): boolean => {
    console.log('🧩 尝试bindStreamToVideo', {
      retry,
      hasVideoElement: !!videoRef.current,
      hasStream: !!remoteStreamRef.current,
      videoSrcObject: videoRef.current?.srcObject ?? null
    })
    if (videoRef.current && remoteStreamRef.current) {
      console.log('📺 绑定远程流到 video 元素')
      videoRef.current.srcObject = remoteStreamRef.current
      const v = videoRef.current
      v.onloadedmetadata = () => {
        console.log('📺 视频元数据加载完成，再次 play()')
        v.play().catch(err => console.warn('⚠️ 第二次 video.play() 失败:', err))
      }
      v.oncanplay = () => {
        console.log('▶️ 视频可以播放')
      }
      onConnectionStateChange(prev => ({ ...prev, video: 'connected' }))
      return true
    } else if (retry < 10) {
      setTimeout(() => bindStreamToVideo(retry + 1), 300)
    }
    return false
  }, [onConnectionStateChange])

  // 获取WebRTC配置
  const fetchWebRTCConfig = useCallback(async () => {
    try {
      console.log('📋 加载WebRTC配置...')
      const response = await fetch('/api/webrtc-config')
      const result = await response.json()
      
      if (result.success) {
        console.log('✅ WebRTC配置加载成功:', result.data)
        setWebrtcConfig(result.data)
      } else {
        throw new Error(result.message || '获取WebRTC配置失败')
      }
    } catch (err) {
      console.error('❌ 获取WebRTC配置失败:', err)
      setError(err instanceof Error ? err.message : '获取配置失败')
    }
  }, [])

  // 初始化WebRTC连接
  const initPeerConnection = useCallback(() => {
    if (!webrtcConfig) return null

    console.log('🔧 初始化PeerConnection...')
    const pc = new RTCPeerConnection({
      iceServers: webrtcConfig.iceServers
    })

    // 暴露到全局方便调试和手动绑定
    ;(window as any).__pc = pc
    ;(window as any).__videoReceiverRef = videoReceiverRef
    ;(window as any).__remoteStreamRef = remoteStreamRef

    // 主动创建接收方向的 transceiver，确保后续 ontrack 能触发
    try {
      pc.addTransceiver('video', { direction: 'recvonly' })
      pc.addTransceiver('audio', { direction: 'recvonly' })
      console.log('📡 已添加 recvonly transceivers (video/audio)')
    } catch (err) {
      console.warn('⚠️ 添加 transceiver 失败(可能浏览器不支持):', err)
    }

    // ICE连接状态监控
    pc.oniceconnectionstatechange = () => {
      const state = pc.iceConnectionState
      console.log('🧊 ICE连接状态变化:', state)
      onConnectionStateChange(prev => ({
        ...prev,
        ice: state === 'connected' || state === 'completed' ? 'connected' : 
             state === 'checking' ? 'connecting' : 
             state === 'disconnected' ? 'disconnected' : 'error'
      }))

      // 在某些平台上，connectionState 可能长期停留在 "connecting"，
      // 但 ICE 已进入 connected / completed，此时可视为已连接。
      if ((state === 'connected' || state === 'completed') && !connectedRef.current) {
        console.log('🎉 ICE 已连接，视为 WebRTC 完全建立')
        connectedRef.current = true
        
        // 查找并保存 video receiver
        const findVideoReceiver = () => {
          const pc: RTCPeerConnection = (window as any).__pc
          if (!pc) {
            setTimeout(findVideoReceiver, 500)
            return
          }

          const receivers = pc.getReceivers()
          console.log('🔍 检查 receivers:', receivers.length, receivers.map(r => ({
            kind: r.track?.kind,
            readyState: r.track?.readyState,
            id: r.track?.id
          })))

          const receiver = receivers.find(r => r.track && r.track.kind === 'video')
          if (receiver && receiver.track) {
            console.log('✅ 找到并保存 video receiver:', receiver.track.readyState)
            videoReceiverRef.current = receiver
            const stream = new MediaStream([receiver.track])
            remoteStreamRef.current = stream
            onStreamRef.current(cloneMediaStream(stream))
            ;(window as any).__remoteStream = stream
            // 立即尝试绑定
            bindStreamToVideo()
            return
          }
          // 若未取到，0.5 秒后重试
          setTimeout(findVideoReceiver, 500)
        }
        findVideoReceiver()
        
        onConnected()
      }
    }

    // 连接状态监控
    pc.onconnectionstatechange = () => {
      const state = pc.connectionState
      console.log('🔄 PeerConnection状态变化:', state)
      
      if (state === 'connected' && !connectedRef.current) {
        console.log('✅ WebRTC连接已建立!')
        connectedRef.current = true
        
        // 查找并保存 video receiver
        const findVideoReceiver = () => {
          const pc: RTCPeerConnection = (window as any).__pc
          if (!pc) return

          const receiver = pc.getReceivers().find(r => r.track && r.track.kind === 'video')
          if (receiver && receiver.track) {
            console.log('✅ 找到并保存 video receiver:', receiver.track.readyState)
            videoReceiverRef.current = receiver
            const stream = new MediaStream([receiver.track])
            remoteStreamRef.current = stream
            onStreamRef.current(cloneMediaStream(stream))
            ;(window as any).__remoteStream = stream
            // 立即尝试绑定
            bindStreamToVideo()
            return
          }
          // 若未取到，0.5 秒后重试
          setTimeout(findVideoReceiver, 500)
        }
        findVideoReceiver()
        
        onConnected()
      } else if (state === 'disconnected' || state === 'failed') {
        console.log('❌ WebRTC连接断开或失败')
        connectedRef.current = false
        onDisconnected()
      }
    }

    // 视频轨道监控
    pc.ontrack = (event) => {
      console.log('🎥 收到媒体轨道:', event.track.kind, event.streams.length)

      // 兼容不同浏览器: 有时 event.streams 为空，需要手动构造 MediaStream
      let stream: MediaStream | null = null
      if (event.streams && event.streams[0]) {
        stream = event.streams[0]
      } else {
        stream = new MediaStream([event.track])
      }

      if (stream) {
        remoteStreamRef.current = stream
        onStreamRef.current(cloneMediaStream(stream))
        ;(window as any).__remoteStream = stream
        console.log('✅ 已缓存远程视频流 (ontrack)')
        
        // 立即尝试绑定
        bindStreamToVideo()

        // 监听 track unmute 事件（有帧到来时会触发）
        event.track.onunmute = () => {
          console.log('🔊 track onunmute – 首帧已到');
          if (remoteStreamRef.current) {
            // console.log(remoteStreamRef.current);
            const v = document.querySelector('video');
            v && (v.srcObject = remoteStreamRef.current);
            onStreamRef.current(cloneMediaStream(remoteStreamRef.current))
          }
          bindStreamToVideo()
        }
      }
    }

    // ICE候选事件
    pc.onicecandidate = (event) => {
      if (event.candidate) {
        console.log('🧊 本地ICE候选生成:', event.candidate.type)
      } else {
        console.log('🧊 ICE候选收集完成')
      }
    }

    return pc
  }, [webrtcConfig, onConnectionStateChange, onConnected, onDisconnected, onStreamRef])

  // 初始化数据通道
  const initDataChannel = useCallback((pc: RTCPeerConnection) => {
    console.log('📡 初始化数据通道...')
    const dataChannel = pc.createDataChannel('input', webrtcConfig?.dataChannelConfig)
    
    dataChannel.onopen = () => {
      console.log('✅ 数据通道已打开')
      onConnectionStateChange(prev => ({ ...prev, dataChannel: 'connected' }))
    }
    
    dataChannel.onclose = () => {
      console.log('🔌 数据通道已关闭')
      onConnectionStateChange(prev => ({ ...prev, dataChannel: 'disconnected' }))
    }
    
    dataChannel.onerror = (error) => {
      console.error('❌ 数据通道错误:', error)
      onConnectionStateChange(prev => ({ ...prev, dataChannel: 'error' }))
    }

    return dataChannel
  }, [webrtcConfig, onConnectionStateChange])

  // 初始化WebSocket连接
  const initWebSocket = useCallback((signalUrl: string) => {
    console.log('🔗 连接信号服务器:', signalUrl)
    const ws = new WebSocket(signalUrl)
    
    ws.onopen = () => {
      console.log('✅ WebSocket连接成功')
      onConnectionStateChange(prev => ({ ...prev, signaling: 'connected' }))
    }
    
    ws.onclose = (event) => {
      console.log('🔌 WebSocket连接关闭:', event.code, event.reason)
      onConnectionStateChange(prev => ({ ...prev, signaling: 'disconnected' }))
    }
    
    ws.onerror = (error) => {
      console.error('❌ WebSocket连接错误:', error)
      onConnectionStateChange(prev => ({ ...prev, signaling: 'error' }))
    }

    return ws
  }, [onConnectionStateChange])

  // 连接到远程桌面
  const connect = useCallback(async (clientId: string, authCode: string, videoQuality: string) => {
    try {
      setError(null)
      console.log('🚀 开始WebRTC连接...', { clientId, authCode, videoQuality })
      
      if (!webrtcConfig) {
        throw new Error('WebRTC配置未加载')
      }

      console.log('📋 使用WebRTC配置:', webrtcConfig)

      // 初始化连接组件
      const pc = initPeerConnection()
      if (!pc) throw new Error('无法创建PeerConnection')
      
      const dataChannel = initDataChannel(pc)
      const ws = initWebSocket(webrtcConfig.signalsServerUrl)
      
      // 保存引用
      peerConnectionRef.current = pc
      dataChannelRef.current = dataChannel
      websocketRef.current = ws

      // 设置WebSocket消息处理
      ws.onmessage = async (event) => {
        try {
          const message = JSON.parse(event.data)
          console.log('📥 收到信令消息:', message.type, message)
          
          switch (message.type) {
            case 'WebRTCOffer': {
              console.log('📨 处理WebRTC Offer...')
              const { session_description } = message
              await pc.setRemoteDescription(new RTCSessionDescription({ type: session_description.sdp_type, sdp: session_description.sdp }))
              const answer = await pc.createAnswer()
              await pc.setLocalDescription(answer)

              const answerMessage = {
                type: 'WebRTCAnswer',
                target_id: clientId,
                session_description: {
                  sdp_type: 'answer',
                  sdp: answer.sdp || ''
                }
              }
              console.log('📤 发送WebRTC Answer')
              ws.send(JSON.stringify(answerMessage))
              break
            }

            case 'WebRTCIceCandidate':
              console.log('🧊 添加远程ICE候选')
              try {
                const candInit: RTCIceCandidateInit = {
                  candidate: message.ice_candidate.candidate,
                  sdpMid: message.ice_candidate.sdp_mid ?? '0',
                  sdpMLineIndex: message.ice_candidate.sdp_mline_index ?? 0
                }
                if (pc.remoteDescription) {
                  await pc.addIceCandidate(new RTCIceCandidate(candInit))
                  console.log('✅ 立即添加远端ICE候选成功')
                } else {
                  console.log('⏳ 远端描述尚未设置，缓存ICE候选')
                  pendingCandidatesRef.current.push(candInit)
                }
              } catch (err) {
                console.error('❌ 添加ICE候选失败:', err)
              }
              break

            case 'Connected':
              console.log('✅ 服务器确认信令连接成功')
              // 在信令连接成功后，发送视频流配置给客户端，触发其生成Offer
              try {
                const profilesResp = await fetch('/api/video-profiles')
                const profilesJson = await profilesResp.json()
                let selectedProfile = null
                if (profilesJson.success) {
                  selectedProfile = (profilesJson.data as any[]).find((p) => p.name === videoQuality)
                }

                if (!selectedProfile) {
                  // 回退到720p30
                  selectedProfile = { name: '720p30', width: 1280, height: 720, fps: 30, bitrate: 1500000, codec: 'H264' }
                }

                const videoConfigMsg = {
                  type: 'VideoStreamConfig',
                  target_id: clientId,
                  config: {
                    width: selectedProfile.width,
                    height: selectedProfile.height,
                    fps: selectedProfile.fps,
                    bitrate: selectedProfile.bitrate,
                    codec: 'H264'
                  }
                }
                console.log('📤 发送VideoStreamConfig:', videoConfigMsg)
                ws.send(JSON.stringify(videoConfigMsg))

                // 创建Offer (recvonly)
                try {
                  if (!pc.getTransceivers().some(t => t.receiver.track && t.receiver.track.kind === 'video')) {
                    console.log('➕ 添加video recvonly transceiver')
                    pc.addTransceiver('video', { direction: 'recvonly' })
                  }
                  const offer = await pc.createOffer({ offerToReceiveAudio: true, offerToReceiveVideo: true })
                  await pc.setLocalDescription(offer)

                  const offerMsg = {
                    type: 'WebRTCOffer',
                    target_id: clientId,
                    session_description: {
                      sdp_type: 'offer',
                      sdp: offer.sdp || ''
                    }
                  }
                  console.log('📤 发送WebRTC Offer', offerMsg)
                  ws.send(JSON.stringify(offerMsg))
                } catch (err) {
                  console.error('❌ 创建/发送Offer失败:', err)
                }
              } catch (err) {
                console.error('❌ 发送VideoStreamConfig失败:', err)
              }
              break

            case 'Error':
              console.error('❌ 服务器错误:', message.message)
              setError(message.message || '服务器连接错误')
              break

            case 'WebRTCAnswer': {
              console.log('📨 收到WebRTC Answer')
              const ans = new RTCSessionDescription({ type: message.session_description.sdp_type, sdp: message.session_description.sdp })
              await pc.setRemoteDescription(ans)
              console.log('✅ 已设置远端描述，开始处理缓存ICE候选，共', pendingCandidatesRef.current.length)
              for (const c of pendingCandidatesRef.current) {
                try {
                  await pc.addIceCandidate(new RTCIceCandidate(c))
                } catch(e){ console.error('❌ 添加缓存ICE失败',e) }
              }
              pendingCandidatesRef.current = []
              break
            }

            default:
              console.log('❓ 未知消息类型:', message.type)
          }
        } catch (err) {
          console.error('❌ 处理信令消息失败:', err)
        }
      }

      // 设置ICE候选发送
      pc.onicecandidate = (event) => {
        if (event.candidate && ws.readyState === WebSocket.OPEN) {
          console.log('🧊 发送ICE候选')
          ws.send(JSON.stringify({
            type: 'WebRTCIceCandidate',
            target_id: clientId,
            ice_candidate: {
              candidate: event.candidate.candidate,
              sdp_mid: event.candidate.sdpMid,
              sdp_mline_index: event.candidate.sdpMLineIndex
            }
          }))
        }
      }

      // WebSocket连接成功后发送连接请求
      const sendConnectRequest = () => {
        if (ws.readyState === WebSocket.OPEN) {
          const connectMessage = {
            type: 'BrowserConnect',
            client_id: clientId,
            auth_code: authCode
          }
          console.log('📤 发送BrowserConnect 请求:', connectMessage)
          ws.send(JSON.stringify(connectMessage))
        } else {
          console.log('⏳ 等待WebSocket连接...')
        }
      }

      // 如果已经连接就直接发送，否则等待连接
      if (ws.readyState === WebSocket.OPEN) {
        sendConnectRequest()
      } else {
        ws.addEventListener('open', sendConnectRequest)
      }

      console.log('⏳ WebRTC连接初始化完成，等待信令交换...')

    } catch (err) {
      console.error('❌ WebRTC连接失败:', err)
      setError(err instanceof Error ? err.message : '连接失败')
      throw err
    }
  }, [webrtcConfig, initPeerConnection, initDataChannel, initWebSocket])

  // 断开连接
  const disconnect = useCallback(async () => {
    console.log('🔌 断开WebRTC连接...')
    
    if (peerConnectionRef.current) {
      peerConnectionRef.current.close()
      peerConnectionRef.current = null
    }
    
    if (dataChannelRef.current) {
      dataChannelRef.current.close()
      dataChannelRef.current = null
    }
    
    if (websocketRef.current) {
      websocketRef.current.close()
      websocketRef.current = null
    }

    if (videoRef.current) {
      videoRef.current.srcObject = null
    }
    
    connectedRef.current = false

    onDisconnected()
  }, [onDisconnected])

  // 发送视频配置
  const sendVideoConfig = useCallback((clientId: string, config: VideoConfig) => {
    if (dataChannelRef.current?.readyState === 'open') {
      console.log('📤 发送视频配置:', config)
      dataChannelRef.current.send(JSON.stringify({
        type: 'video-config',
        config: config
      }))
    } else {
      console.warn('⚠️ 数据通道未开启，无法发送视频配置')
    }
  }, [])

  // 强制关键帧
  const forceKeyframe = useCallback((clientId: string) => {
    if (dataChannelRef.current?.readyState === 'open') {
      console.log('📤 发送强制关键帧请求')
      dataChannelRef.current.send(JSON.stringify({
        type: 'force-keyframe'
      }))
    } else {
      console.warn('⚠️ 数据通道未开启，无法发送关键帧请求')
    }
  }, [])

  // 发送输入事件
  const sendInputEvent = useCallback((event: any) => {
    if (dataChannelRef.current?.readyState === 'open') {
      dataChannelRef.current.send(JSON.stringify({
        type: 'input-event',
        event: event
      }))
    } else {
      console.warn('⚠️ 数据通道未开启，无法发送输入事件')
    }
  }, [])

  // 当 video 元素挂载时，尝试绑定已存在的远程流
  useLayoutEffect(() => {
    if (videoRef.current) {
      console.log('🎬 video元素已挂载')
      
      // 如果已有流，直接绑定
      if (remoteStreamRef.current) {
        bindStreamToVideo()
      }
      // 如果没有流但有 receiver，创建流并绑定
      else if (videoReceiverRef.current && videoReceiverRef.current.track) {
        console.log('📺 从 receiver 创建视频流')
        const stream = new MediaStream([videoReceiverRef.current.track])
        remoteStreamRef.current = stream
        onStreamRef.current(cloneMediaStream(stream))
        bindStreamToVideo()
      }
    }
  }, [bindStreamToVideo, onStreamRef])

  // 初始化时获取配置
  useEffect(() => {
    fetchWebRTCConfig()
  }, [fetchWebRTCConfig])

  // Fallback轮询绑定，防止极端时序失配
  useEffect(() => {
    const interval = setInterval(() => {
      if (videoRef.current && !videoRef.current.srcObject && remoteStreamRef.current) {
        console.log('🔄 轮询绑定远程流')
        if (bindStreamToVideo()) {
          clearInterval(interval)
        }
      }
    }, 500)

    return () => clearInterval(interval)
  }, [bindStreamToVideo])

  const setVideoElement = useCallback((node: HTMLVideoElement | null) => {
    videoRef.current = node
    if (node) {
      console.log('🎯 setVideoElement 回调触发')
      bindStreamToVideo()
    }
  }, [bindStreamToVideo])

  return {
    connect,
    disconnect,
    sendVideoConfig,
    forceKeyframe,
    sendInputEvent,
    error,
    webrtcConfig,
    videoRef,
    setVideoElement
  }
} 