'use client'

import { useRef, useEffect, useState, useCallback } from 'react'
import type { ConnectionState, VideoConfig, VideoProfile, MouseEvent as CustomMouseEvent, WheelEvent as CustomWheelEvent } from '@/types'
import { useActionState, useOptimistic } from 'react'
import { useWebRTCConnection } from '@/hooks/useWebRTCConnection'
import { StatusIndicator } from './StatusIndicator'

interface RemoteDesktopProps {
  clientId: string
  authCode: string
}

interface FormState {
  isConnecting: boolean
  error: string | null
  success: boolean
}

const initialFormState: FormState = {
  isConnecting: false,
  error: null,
  success: false
}

const initialConnectionState: ConnectionState = {
  signaling: 'disconnected',
  ice: 'disconnected',
  video: 'disconnected',
  dataChannel: 'disconnected'
}

export function RemoteDesktop({ clientId, authCode }: RemoteDesktopProps) {
  const [formState, formAction] = useActionState(
    async (prevState: FormState, formData: FormData): Promise<FormState> => {
      const videoQuality = formData.get('videoQuality') as string
      
      try {
        console.log('🎬 开始连接远程桌面...', { clientId, authCode, videoQuality })
        await connect(clientId, authCode, videoQuality)
        console.log('✅ 连接方法调用成功')
        
        return {
          isConnecting: false,
          error: null,
          success: true
        }
      } catch (error) {
        console.error('❌ 连接失败:', error)
        return {
          isConnecting: false,
          error: error instanceof Error ? error.message : '连接失败',
          success: false
        }
      }
    },
    initialFormState
  )

  const [optimisticState, addOptimistic] = useOptimistic(
    formState,
    (state: FormState, optimisticValue: Partial<FormState>): FormState => ({
      ...state,
      ...optimisticValue
    })
  )

  const [connectionState, setConnectionState] = useState<ConnectionState>(initialConnectionState)
  const [videoProfiles, setVideoProfiles] = useState<VideoProfile[]>([])
  const [isConnected, setIsConnected] = useState(false)
  const [connectionLogs, setConnectionLogs] = useState<string[]>([])
  const [remoteStream, setRemoteStream] = useState<MediaStream | null>(null)
  const [videoReady, setVideoReady] = useState(false)
  const [videoElementMounted, setVideoElementMounted] = useState(false)

  // 添加连接日志
  const addLog = (message: string) => {
    const timestamp = new Date().toLocaleTimeString()
    setConnectionLogs(prev => [...prev.slice(-9), `${timestamp}: ${message}`])
  }

  const {
    connect,
    disconnect,
    sendVideoConfig,
    forceKeyframe,
    error: connectionError,
    videoRef,
    setVideoElement
  } = useWebRTCConnection({
    onConnectionStateChange: setConnectionState,
    onConnected: () => {
      console.log('🎉 WebRTC连接完全建立!')
      setIsConnected(true)
      addLog('✅ 远程桌面连接成功')
    },
    onDisconnected: () => {
      console.log('👋 WebRTC连接已断开')
      setIsConnected(false)
      addLog('🔌 远程桌面连接断开')
    },
    onStream: (s) => {
      console.log('📡 收到 onStream 回调')
      setRemoteStream(s)
    }
  })

  // 获取视频配置文件
  useEffect(() => {
    const fetchVideoProfiles = async () => {
      try {
        console.log('📺 获取视频配置文件...')
        const response = await fetch('/api/video-profiles')
        const result = await response.json()
        
        if (result.success) {
          console.log('✅ 视频配置文件获取成功:', result.data)
          setVideoProfiles(result.data)
        } else {
          console.error('❌ 获取视频配置文件失败:', result.message)
        }
      } catch (error) {
        console.error('❌ 获取视频配置文件失败:', error)
      }
    }

    fetchVideoProfiles()
  }, [])

  // 监控连接状态变化
  useEffect(() => {
    const states = Object.entries(connectionState)
    const connectedStates = states.filter(([_, state]) => state === 'connected')
    const errorStates = states.filter(([_, state]) => state === 'error')
    
    if (errorStates.length > 0) {
      addLog(`❌ 连接错误: ${errorStates.map(([key]) => key).join(', ')}`)
    } else if (connectedStates.length > 0) {
      addLog(`✅ ${connectedStates.map(([key]) => key).join(', ')} 已连接`)
    }
  }, [connectionState])

  // 监控连接错误
  useEffect(() => {
    if (connectionError) {
      addLog(`❌ 连接错误: ${connectionError}`)
    }
  }, [connectionError])

  // useEffect to bind stream to video
  const handleSetVideoElement = useCallback((node: HTMLVideoElement | null) => {
    setVideoElement(node)
    setVideoElementMounted(!!node)
    // 如果此时已经有流，立即绑定
    if (node && remoteStream) {
      // @ts-ignore
      node.srcObject = remoteStream
    }
  }, [setVideoElement, remoteStream])

  useEffect(() => {
    if (remoteStream && videoElementMounted) {
      const node = videoRef.current
      if (!node) return
      console.log('🔗 将remoteStream绑定到video.srcObject')
      // @ts-ignore
      node.srcObject = remoteStream
      const handleCanPlay = () => {
        console.log('▶️ canplay 事件触发')
        setVideoReady(true)
        node.removeEventListener('canplay', handleCanPlay)
      }
      node.addEventListener('canplay', handleCanPlay)
    }
  }, [remoteStream, videoElementMounted])

  const handleSubmit = (formData: FormData) => {
    console.log('📤 提交连接表单')
    addLog('🚀 开始连接远程桌面...')
    addOptimistic({ isConnecting: true, error: null, success: false })
    formAction(formData)
  }

  const handleDisconnect = async () => {
    console.log('🔌 手动断开连接')
    await disconnect()
    setIsConnected(false)
    setConnectionState(initialConnectionState)
    setVideoReady(false)
    addLog('🔌 手动断开连接')
  }

  const handleVideoConfigChange = (profileName: string) => {
    const profile = videoProfiles.find(p => p.name === profileName)
    if (profile && isConnected) {
      console.log('📺 切换视频配置:', profile)
      // 将VideoProfile转换为VideoConfig
      const videoConfig: VideoConfig = {
        width: profile.width,
        height: profile.height,
        fps: profile.fps,
        bitrate: profile.bitrate,
        codec: 'H264'
      }
      sendVideoConfig(clientId, videoConfig)
      addLog(`📺 切换视频质量: ${profile.name}`)
    }
  }

  const handleForceKeyframe = () => {
    if (isConnected) {
      console.log('🔄 强制关键帧')
      forceKeyframe(clientId)
      addLog('🔄 请求关键帧')
    }
  }

  return (
    <div className="space-y-6">
      {/* 连接表单 */}
      <div className="bg-white/10 backdrop-blur-sm border border-white/20 rounded-lg p-6">
        <h2 className="text-xl font-semibold text-white mb-4">远程桌面连接</h2>
        
        <form action={handleSubmit} className="space-y-4">
          <div>
            <label htmlFor="videoQuality" className="block text-sm font-medium text-gray-300 mb-2">
              视频质量
            </label>
            <select
              id="videoQuality"
              name="videoQuality"
              className="w-full px-3 py-2 bg-white/10 border border-white/20 rounded-md text-white placeholder-gray-400 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent"
              disabled={optimisticState.isConnecting || isConnected}
            >
                             {videoProfiles.map((profile) => (
                 <option key={profile.name} value={profile.name} className="bg-gray-800">
                   {profile.name} ({profile.width}x{profile.height}@{profile.fps}fps)
                 </option>
               ))}
            </select>
          </div>

          <div className="flex gap-3">
            <button
              type="submit"
              disabled={optimisticState.isConnecting || isConnected}
              className="flex-1 bg-gradient-to-r from-blue-500 to-purple-600 hover:from-blue-600 hover:to-purple-700 disabled:from-gray-500 disabled:to-gray-600 text-white font-medium py-2 px-4 rounded-md transition-colors duration-200"
            >
              {optimisticState.isConnecting ? '连接中...' : isConnected ? '已连接' : '连接远程桌面'}
            </button>

            {isConnected && (
              <button
                type="button"
                onClick={handleDisconnect}
                className="bg-red-500 hover:bg-red-600 text-white font-medium py-2 px-4 rounded-md transition-colors duration-200"
              >
                断开连接
              </button>
            )}
          </div>
        </form>

        {/* 错误提示 */}
        {(optimisticState.error || connectionError) && (
          <div className="mt-4 p-3 bg-red-500/20 border border-red-500/50 rounded-md">
            <p className="text-red-200 text-sm">
              {optimisticState.error || connectionError}
            </p>
          </div>
        )}

        {/* 成功提示 */}
        {optimisticState.success && !connectionError && (
          <div className="mt-4 p-3 bg-green-500/20 border border-green-500/50 rounded-md">
            <p className="text-green-200 text-sm">连接请求已发送，等待远程桌面响应...</p>
          </div>
        )}
      </div>

      {/* 连接状态 */}
      <div className="bg-white/10 backdrop-blur-sm border border-white/20 rounded-lg p-6">
        <h3 className="text-lg font-semibold text-white mb-4">连接状态</h3>
        <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
          <StatusIndicator
            label="信令服务器"
            status={connectionState.signaling}
          />
          <StatusIndicator
            label="ICE连接"
            status={connectionState.ice}
          />
          <StatusIndicator
            label="视频流"
            status={connectionState.video}
          />
          <StatusIndicator
            label="数据通道"
            status={connectionState.dataChannel}
          />
        </div>
      </div>

      {/* 连接日志 */}
      <div className="bg-white/10 backdrop-blur-sm border border-white/20 rounded-lg p-6">
        <h3 className="text-lg font-semibold text-white mb-4">连接日志</h3>
        <div className="bg-black/30 rounded-md p-4 max-h-48 overflow-y-auto">
          {connectionLogs.length === 0 ? (
            <p className="text-gray-400 text-sm">暂无日志</p>
          ) : (
            <div className="space-y-1">
              {connectionLogs.map((log, index) => (
                <p key={index} className="text-gray-300 text-sm font-mono">
                  {log}
                </p>
              ))}
            </div>
          )}
        </div>
      </div>

      {/* 视频控制 */}
      {isConnected && (
        <div className="bg-white/10 backdrop-blur-sm border border-white/20 rounded-lg p-6">
          <h3 className="text-lg font-semibold text-white mb-4">视频控制</h3>
          <div className="flex gap-3 flex-wrap">
            <select
              onChange={(e) => handleVideoConfigChange(e.target.value)}
              className="px-3 py-2 bg-white/10 border border-white/20 rounded-md text-white focus:outline-none focus:ring-2 focus:ring-blue-500"
            >
              <option value="" className="bg-gray-800">选择视频质量</option>
              {videoProfiles.map((profile) => (
                <option key={profile.name} value={profile.name} className="bg-gray-800">
                  {profile.name}
                </option>
              ))}
            </select>

            <button
              onClick={handleForceKeyframe}
              className="bg-blue-500 hover:bg-blue-600 text-white px-4 py-2 rounded-md transition-colors duration-200"
            >
              强制关键帧
            </button>
          </div>
        </div>
      )}

      {/* 视频显示 */}
      <div className="bg-white/10 backdrop-blur-sm border border-white/20 rounded-lg p-6">
        <h3 className="text-lg font-semibold text-white mb-4">远程桌面画面</h3>
        <div className="relative bg-black rounded-lg overflow-hidden">
          <video
            ref={handleSetVideoElement}
            autoPlay
            playsInline
            muted
            onPlay={() => setVideoReady(true)}
            className="w-full h-auto max-h-[600px] object-contain"
            style={{ minHeight: '300px' }}
          />
          {!videoReady && (
            <div className="absolute inset-0 flex items-center justify-center bg-black/50">
              <p className="text-gray-400 text-lg">等待连接远程桌面...</p>
            </div>
          )}
          {isConnected && !videoReady && (
            <div className="absolute inset-0 flex items-center justify-center bg-black/50">
              <p className="text-gray-400 text-lg">等待视频流...</p>
            </div>
          )}
        </div>
      </div>
    </div>
  )
} 