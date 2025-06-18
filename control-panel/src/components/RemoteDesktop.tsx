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
  sendInputEvent: (event: any) => void
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
  sendInputEvent, 
  connectionError, 
  remoteStream 
}: RemoteDesktopProps) {
  // 连接已在 page.tsx 中处理，这里不需要连接表单

  const [videoProfiles, setVideoProfiles] = useState<VideoProfile[]>([])
  const [connectionLogs, setConnectionLogs] = useState<string[]>([])
  const [videoReady, setVideoReady] = useState(false)
  const [isControlEnabled, setIsControlEnabled] = useState(false)
  const [remoteCursor, setRemoteCursor] = useState<{x:number,y:number}|null>(null)
  const [isFullscreen, setIsFullscreen] = useState(false)
  const videoRef = useRef<HTMLVideoElement | null>(null)
  const videoContainerRef = useRef<HTMLDivElement | null>(null)
  const lastMouseMoveTime = useRef(0)

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

  // 切换全屏
  const toggleFullscreen = () => {
    const elem = videoContainerRef.current
    if (!elem) return

    if (!document.fullscreenElement) {
      elem.requestFullscreen?.().catch(err => console.error('进入全屏失败', err))
    } else {
      document.exitFullscreen?.()
    }
  }

  // 监听全屏变化 & ESC
  useEffect(() => {
    const handleFsChange = () => {
      setIsFullscreen(!!document.fullscreenElement)
    }
    document.addEventListener('fullscreenchange', handleFsChange)

    return () => {
      document.removeEventListener('fullscreenchange', handleFsChange)
    }
  }, [])

  /**
   * 计算鼠标在「实际视频区域」与「容器」中的归一化坐标。
   * - remoteX/remoteY: 以视频有效像素区域左上角为(0,0)，右下角为(1,1)
   * - displayX/displayY: 以外层容器左上角为(0,0)，右下角为(1,1)，用于本地光标渲染
   * 这样既能保证发送给远端的坐标准确，也能让本地光标在 letter-box 情况下对齐。
   */
  const getCoordinates = (event: React.MouseEvent<HTMLDivElement>) => {
    const containerRect = videoContainerRef.current?.getBoundingClientRect()
    const videoRect = videoRef.current?.getBoundingClientRect()

    if (!containerRect || !videoRect) {
      return { remoteX: 0, remoteY: 0, displayX: 0, displayY: 0 }
    }

    // 视频的实际像素尺寸
    const intrinsicW = videoRef.current?.videoWidth || videoRect.width
    const intrinsicH = videoRef.current?.videoHeight || videoRect.height

    const aspectIntrinsic = intrinsicW / intrinsicH
    const aspectDisplayed = videoRect.width / videoRect.height

    // 计算内部有效图像区域（去掉object-contain产生的黑边）
    let imgDisplayW = 0
    let imgDisplayH = 0
    let imgOffsetX = 0
    let imgOffsetY = 0

    if (aspectDisplayed > aspectIntrinsic) {
      // 左右有黑边
      imgDisplayH = videoRect.height
      imgDisplayW = imgDisplayH * aspectIntrinsic
      imgOffsetX = (videoRect.width - imgDisplayW) / 2
    } else {
      // 上下有黑边
      imgDisplayW = videoRect.width
      imgDisplayH = imgDisplayW / aspectIntrinsic
      imgOffsetY = (videoRect.height - imgDisplayH) / 2
    }

    // 计算基于图片区域的坐标
    const imgX = event.clientX - videoRect.left - imgOffsetX
    const imgY = event.clientY - videoRect.top - imgOffsetY

    const remoteX = Math.max(0, Math.min(1, imgX / imgDisplayW))
    const remoteY = Math.max(0, Math.min(1, imgY / imgDisplayH))

    // 显示用：转换到容器百分比
    const displayX = (videoRect.left - containerRect.left + imgOffsetX + remoteX * imgDisplayW) / containerRect.width
    const displayY = (videoRect.top - containerRect.top + imgOffsetY + remoteY * imgDisplayH) / containerRect.height

    return { remoteX, remoteY, displayX, displayY }
  }

  // 鼠标事件处理
  const handleMouseMove = useCallback((event: React.MouseEvent<HTMLDivElement>) => {
    if (!isControlEnabled || !isConnected) return
    
    // 节流处理：限制鼠标移动事件的发送频率（每16ms最多发送一次，约60fps）
    const now = Date.now()
    if (now - lastMouseMoveTime.current < 16) return
    lastMouseMoveTime.current = now
    
    const { remoteX, remoteY, displayX, displayY } = getCoordinates(event)
    const mouseEvent = {
      type: 'MouseEvent',
      x: remoteX,
      y: remoteY,
      button: 'none',
      event_type: 'move'
    }
    sendInputEvent(mouseEvent)

    // 更新本地光标位置用于显示
    setRemoteCursor({ x: displayX, y: displayY })
  }, [isControlEnabled, isConnected, sendInputEvent])

  const handleMouseDown = useCallback((event: React.MouseEvent<HTMLDivElement>) => {
    if (!isControlEnabled || !isConnected) return
    
    event.preventDefault()
    const { remoteX, remoteY } = getCoordinates(event)
    const button = event.button === 0 ? 'left' : event.button === 1 ? 'middle' : 'right'
    
    const mouseEvent = {
      type: 'MouseEvent',
      x: remoteX,
      y: remoteY,
      button,
      event_type: 'press'
    }
    sendInputEvent(mouseEvent)
  }, [isControlEnabled, isConnected, sendInputEvent])

  const handleMouseUp = useCallback((event: React.MouseEvent<HTMLDivElement>) => {
    if (!isControlEnabled || !isConnected) return
    
    event.preventDefault()
    const { remoteX, remoteY } = getCoordinates(event)
    const button = event.button === 0 ? 'left' : event.button === 1 ? 'middle' : 'right'
    
    const mouseEvent = {
      type: 'MouseEvent',
      x: remoteX,
      y: remoteY,
      button,
      event_type: 'release'
    }
    sendInputEvent(mouseEvent)
  }, [isControlEnabled, isConnected, sendInputEvent])

  const handleWheel = useCallback((event: React.WheelEvent<HTMLDivElement>) => {
    if (!isControlEnabled || !isConnected) return
    
    event.preventDefault()
    // 将滚轮事件转换为鼠标事件，使用scroll_delta
    const { remoteX, remoteY } = getCoordinates(event)
    const mouseEvent = {
      type: 'MouseEvent',
      x: remoteX,
      y: remoteY,
      button: 'wheel',
      event_type: 'scroll',
      scroll_delta: event.deltaY > 0 ? -1 : 1  // 标准化滚动方向
    }
    sendInputEvent(mouseEvent)
  }, [isControlEnabled, isConnected, sendInputEvent])

  // 键盘事件处理
  const handleKeyDown = useCallback((event: KeyboardEvent) => {
    if (!isControlEnabled || !isConnected) return
    
    event.preventDefault()
    const keyboardEvent = {
      type: 'KeyboardEvent',
      key: event.code,
      event_type: 'press'
    }
    sendInputEvent(keyboardEvent)
  }, [isControlEnabled, isConnected, sendInputEvent])

  const handleKeyUp = useCallback((event: KeyboardEvent) => {
    if (!isControlEnabled || !isConnected) return
    
    event.preventDefault()
    const keyboardEvent = {
      type: 'KeyboardEvent',
      key: event.code,
      event_type: 'release'
    }
    sendInputEvent(keyboardEvent)
  }, [isControlEnabled, isConnected, sendInputEvent])

  // 添加/移除键盘事件监听器
  useEffect(() => {
    if (isControlEnabled && isConnected) {
      document.addEventListener('keydown', handleKeyDown)
      document.addEventListener('keyup', handleKeyUp)
      
      return () => {
        document.removeEventListener('keydown', handleKeyDown)
        document.removeEventListener('keyup', handleKeyUp)
      }
    }
  }, [isControlEnabled, isConnected, handleKeyDown, handleKeyUp])

  const toggleControl = () => {
    setIsControlEnabled(prev => !prev)
    addLog(isControlEnabled ? '🚫 远程控制已禁用' : '🎮 远程控制已启用')
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

                  <button
                    onClick={toggleControl}
                    className={`px-3 py-2 rounded-md text-sm transition-colors duration-200 ${
                      isControlEnabled 
                        ? 'bg-green-500 hover:bg-green-600 text-white' 
                        : 'bg-gray-500 hover:bg-gray-600 text-white'
                    }`}
                  >
                    {isControlEnabled ? '🎮 控制已启用' : '🚫 控制已禁用'}
                  </button>

                  {/* 全屏按钮 */}
                  <button
                    onClick={toggleFullscreen}
                    className="bg-indigo-500 hover:bg-indigo-600 text-white px-3 py-2 rounded-md text-sm transition-colors duration-200"
                  >
                    {isFullscreen ? '退出全屏' : '全屏'}
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
        <div 
          ref={videoContainerRef}
          className={`relative bg-black rounded-lg overflow-hidden border border-white/20 shadow-2xl ${
            isControlEnabled ? 'cursor-none' : 'cursor-default'
          }`}
          onMouseMove={handleMouseMove}
          onMouseDown={handleMouseDown}
          onMouseUp={handleMouseUp}
          onWheel={handleWheel}
          onContextMenu={(e) => e.preventDefault()}
          tabIndex={isControlEnabled ? 0 : -1} // 使div能够获得焦点以接收键盘事件
        >
          <video
            ref={videoRef}
            autoPlay
            playsInline
            muted
            onPlay={() => setVideoReady(true)}
            className="w-full object-contain pointer-events-none"
            style={isFullscreen ? { width: '100%', height: '100vh' } : { minHeight: '400px', maxHeight: 'calc(100vh - 180px)', height: 'auto' }}
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
          
          {/* 控制状态指示 */}
          {isControlEnabled && (
            <div className="absolute top-4 right-4 bg-green-500/80 text-white px-3 py-1 rounded-full text-sm">
              🎮 远程控制已启用
            </div>
          )}

          {/* 远程鼠标光标可视化 */}
          {remoteCursor && isControlEnabled && (
            <div
              className="absolute w-4 h-4 pointer-events-none select-none"
              style={{
                left: `${remoteCursor.x * 100}%`,
                top: `${remoteCursor.y * 100}%`,
                transform: 'translate(0, 0)',
                width: '16px',
                height: '16px',
                backgroundImage: 'url("data:image/svg+xml;utf8,<svg xmlns=\'http://www.w3.org/2000/svg\' width=\'16\' height=\'16\' viewBox=\'0 0 16 16\'><path d=\'M1 1 L1 10 L4 7 L7 15 L9 14 L6 6 L10 6 Z\' fill=\'white\' stroke=\'black\' stroke-width=\'0.5\'/></svg>")',
                backgroundRepeat: 'no-repeat',
                backgroundSize: '16px 16px'
              }}
            />
          )}
        </div>
      </div>
    </div>
  )
} 