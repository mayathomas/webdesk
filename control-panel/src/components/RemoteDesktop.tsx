'use client'

import { useRef, useEffect, useState, useCallback } from 'react'
import type { ConnectionState, VideoConfig, VideoProfile, MouseEvent as CustomMouseEvent, WheelEvent as CustomWheelEvent } from '@/types'
import { StatusIndicator } from './StatusIndicator'

interface RemoteDesktopProps {
  clientId: string
  authCode: string
  connectionState: ConnectionState
  isConnected: boolean
  disconnect: () => Promise<void>
  sendVideoConfig: (clientId: string, config: VideoConfig) => void
  forceKeyframe: (clientId: string) => void
  connectionError: string | null
  remoteStream: MediaStream | null
}

export function RemoteDesktop({ 
  clientId, 
  authCode, 
  connectionState, 
  isConnected, 
  disconnect, 
  sendVideoConfig, 
  forceKeyframe, 
  connectionError, 
  remoteStream 
}: RemoteDesktopProps) {
  // 连接已在 page.tsx 中处理，这里不需要连接表单

  const [videoProfiles, setVideoProfiles] = useState<VideoProfile[]>([])
  const [connectionLogs, setConnectionLogs] = useState<string[]>([])
  const [videoReady, setVideoReady] = useState(false)
  const videoRef = useRef<HTMLVideoElement | null>(null)

  // 添加连接日志
  const addLog = (message: string) => {
    const timestamp = new Date().toLocaleTimeString()
    setConnectionLogs(prev => [...prev.slice(-9), `${timestamp}: ${message}`])
  }

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

  // 监控连接状态变化 - 使用 ref 存储之前的状态避免无限循环
  const prevConnectionStateRef = useRef<ConnectionState>(connectionState)
  useEffect(() => {
    const prev = prevConnectionStateRef.current
    const current = connectionState
    
    // 只有状态真正变化时才添加日志
    if (JSON.stringify(prev) !== JSON.stringify(current)) {
      const states = Object.entries(current)
      const connectedStates = states.filter(([_, state]) => state === 'connected')
      const errorStates = states.filter(([_, state]) => state === 'error')
      
      if (errorStates.length > 0) {
        addLog(`❌ 连接错误: ${errorStates.map(([key]) => key).join(', ')}`)
      } else if (connectedStates.length > 0) {
        addLog(`✅ ${connectedStates.map(([key]) => key).join(', ')} 已连接`)
      }
      
      prevConnectionStateRef.current = current
    }
  }, [connectionState])

  // 监控连接错误
  useEffect(() => {
    if (connectionError) {
      addLog(`❌ 连接错误: ${connectionError}`)
    }
  }, [connectionError])

  // 绑定video流
  useEffect(() => {
    if (remoteStream && videoRef.current) {
      const node = videoRef.current
      console.log('🔗 将remoteStream绑定到video.srcObject')
      node.srcObject = remoteStream
      
      const handleCanPlay = () => {
        console.log('▶️ canplay 事件触发')
        setVideoReady(true)
        node.removeEventListener('canplay', handleCanPlay)
      }
      node.addEventListener('canplay', handleCanPlay)
      
      // 清理函数
      return () => {
        node.removeEventListener('canplay', handleCanPlay)
      }
    }
  }, [remoteStream])

  const handleDisconnect = async () => {
    console.log('🔌 手动断开连接')
    await disconnect()
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
    <div className="min-h-screen bg-gradient-to-br from-purple-900 via-blue-900 to-indigo-900 p-4">
      <div className="max-w-7xl mx-auto space-y-4">
        {/* 顶部控制栏 */}
        <div className="bg-white/10 backdrop-blur-sm border border-white/20 rounded-lg p-4">
          <div className="flex items-center justify-between">
            <div className="flex items-center space-x-4">
              <div className={`w-3 h-3 rounded-full ${isConnected ? 'bg-green-500' : 'bg-gray-500'}`}></div>
              <span className="text-white font-medium">
                {isConnected ? '已连接到远程桌面' : '未连接'}
              </span>
              
              {/* 状态指示器 - 水平排列 */}
              <div className="flex items-center space-x-4 ml-8">
                <StatusIndicator
                  label="信令"
                  status={connectionState.signaling}
                />
                <StatusIndicator
                  label="ICE"
                  status={connectionState.ice}
                />
                <StatusIndicator
                  label="视频"
                  status={connectionState.video}
                />
                <StatusIndicator
                  label="数据"
                  status={connectionState.dataChannel}
                />
              </div>
            </div>
            
            <div className="flex items-center space-x-3">
              {/* 视频控制 */}
              {isConnected && (
                <>
                  <select
                    onChange={(e) => handleVideoConfigChange(e.target.value)}
                    className="px-3 py-2 bg-white/10 border border-white/20 rounded-md text-white text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
                  >
                    <option value="" className="bg-gray-800">选择画质</option>
                    {videoProfiles.map((profile) => (
                      <option key={profile.name} value={profile.name} className="bg-gray-800">
                        {profile.name}
                      </option>
                    ))}
                  </select>

                  <button
                    onClick={handleForceKeyframe}
                    className="bg-blue-500 hover:bg-blue-600 text-white px-3 py-2 rounded-md text-sm transition-colors duration-200"
                  >
                    刷新画面
                  </button>
                </>
              )}
              
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
          </div>

          {/* 错误提示 */}
          {connectionError && (
            <div className="mt-3 p-3 bg-red-500/20 border border-red-500/50 rounded-md">
              <p className="text-red-200 text-sm">{connectionError}</p>
            </div>
          )}
        </div>

        {/* 视频显示区域 */}
        <div className="relative bg-black rounded-lg overflow-hidden border border-white/20 shadow-2xl">
          <video
            ref={videoRef}
            autoPlay
            playsInline
            muted
            onPlay={() => setVideoReady(true)}
            className="w-full h-auto object-contain"
            style={{ minHeight: '400px', maxHeight: 'calc(100vh - 180px)' }}
          />
          {!videoReady && (
            <div className="absolute inset-0 flex items-center justify-center bg-black/50">
              <div className="text-center">
                <div className="animate-spin rounded-full h-12 w-12 border-b-2 border-white mb-4 mx-auto"></div>
                <p className="text-gray-300 text-lg">
                  {isConnected ? '等待视频流...' : '等待连接远程桌面...'}
                </p>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  )
} 