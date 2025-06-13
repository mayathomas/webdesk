'use client'

import { useState, useCallback } from 'react'
import { useActionState, useOptimistic } from 'react'
import ConnectionForm from '@/components/ConnectionForm'
import { RemoteDesktop } from '@/components/RemoteDesktop'
import { useWebRTCConnection } from '@/hooks/useWebRTCConnection'
import type { ConnectionState, VideoConfig, ActionState } from '@/types'

export default function HomePage() {
  const [isConnected, setIsConnected] = useState(false)
  const [clientId, setClientId] = useState('')
  const [authCode, setAuthCode] = useState('')
  const [connectionState, setConnectionState] = useState<ConnectionState>({
    signaling: 'disconnected',
    ice: 'disconnected',
    dataChannel: 'disconnected',
    video: 'disconnected'
  })

  // React 19 - useOptimistic for immediate UI feedback
  const [optimisticConnectionState, setOptimisticConnectionState] = useOptimistic(
    connectionState,
    (state, newState: Partial<ConnectionState>) => ({ ...state, ...newState })
  )

  const {
    connect: connectWebRTC,
    disconnect: disconnectWebRTC,
    sendVideoConfig,
    forceKeyframe,
    sendInputEvent,
    error: connectionError,
    videoRef
  } = useWebRTCConnection({
    onConnectionStateChange: (updater) => {
      setConnectionState(prev => updater(prev))
    },
    onConnected: () => setIsConnected(true),
    onDisconnected: () => setIsConnected(false)
  })

  // React 19 - useActionState for handling async connection
  const [connectionStatus, connectAction, isConnecting] = useActionState(
    async (_previousState: ActionState, formData: FormData): Promise<ActionState> => {
      try {
        const clientId = formData.get('clientId') as string
        const authCode = formData.get('authCode') as string
        const videoQuality = formData.get('videoQuality') as string

        if (!clientId || !authCode) {
          return { success: false, message: '请输入客户端ID和验证码' }
        }

        // Optimistic update
        setOptimisticConnectionState({ signaling: 'connecting' })
        setClientId(clientId)
        setAuthCode(authCode)

        await connectWebRTC(clientId, authCode, videoQuality)
        
        return { success: true, message: '连接成功' }
      } catch (error: any) {
        setOptimisticConnectionState({ signaling: 'disconnected' })
        return { success: false, message: error.message || '连接失败' }
      }
    },
    { success: false, message: '' }
  )

  const handleVideoConfigChange = useCallback((config: VideoConfig) => {
    if (isConnected && clientId) {
      sendVideoConfig(clientId, config)
    }
  }, [isConnected, clientId, sendVideoConfig])

  const handleForceKeyframe = useCallback(() => {
    if (isConnected && clientId) {
      forceKeyframe(clientId)
    }
  }, [isConnected, clientId, forceKeyframe])

  const handleDisconnect = useCallback(async () => {
    await disconnectWebRTC()
    setIsConnected(false)
    setClientId('')
    setAuthCode('')
  }, [disconnectWebRTC])

  if (isConnected) {
    return (
      <RemoteDesktop
        clientId={clientId}
        authCode={authCode}
      />
    )
  }

  return (
    <div className="min-h-screen bg-gradient-to-br from-purple-900 via-blue-900 to-indigo-900">
      <div className="container mx-auto px-4 py-8">
        <header className="text-center mb-12">
          <h1 className="text-5xl font-bold text-white mb-4">
            🎬 远程桌面控制面板
          </h1>
          <p className="text-xl text-white/80 mb-2">
            基于 React 19 + Next.js 15 的现代化远程控制解决方案
          </p>
          <p className="text-lg text-white/60">
            支持 H.264 硬件编码 • WebRTC 低延迟传输 • 实时输入控制
          </p>
        </header>

        <main className="max-w-4xl mx-auto">
          <div className="grid grid-cols-1 lg:grid-cols-2 gap-8 items-start">
            {/* 连接表单 */}
            <div>
              <ConnectionForm
                action={connectAction}
                isConnecting={isConnecting}
                connectionStatus={connectionStatus}
                connectionState={optimisticConnectionState}
        />
            </div>

            {/* 功能介绍 */}
            <div className="space-y-6">
              <div className="bg-white/10 backdrop-blur-lg rounded-2xl p-6 border border-white/20">
                <h3 className="text-xl font-semibold text-white mb-4">✨ 主要特性</h3>
                <ul className="space-y-3 text-white/80">
                  <li className="flex items-center">
                    <span className="text-green-400 mr-3">🚀</span>
                    React 19 新特性：useActionState、useOptimistic
                  </li>
                  <li className="flex items-center">
                    <span className="text-blue-400 mr-3">🎬</span>
                    H.264 硬件编码，低延迟高质量
                  </li>
                  <li className="flex items-center">
                    <span className="text-purple-400 mr-3">🔗</span>
                    WebRTC P2P 直连，端到端加密
                  </li>
                  <li className="flex items-center">
                    <span className="text-yellow-400 mr-3">⚡</span>
                    实时鼠标键盘控制
          </li>
                  <li className="flex items-center">
                    <span className="text-red-400 mr-3">📊</span>
                    实时连接状态监控
          </li>
                </ul>
              </div>

              <div className="bg-white/10 backdrop-blur-lg rounded-2xl p-6 border border-white/20">
                <h3 className="text-xl font-semibold text-white mb-4">🔧 技术栈</h3>
                <div className="grid grid-cols-2 gap-4 text-sm">
                  <div>
                    <h4 className="font-semibold text-white/90 mb-2">前端</h4>
                    <ul className="space-y-1 text-white/70">
                      <li>• React 19.1.0</li>
                      <li>• Next.js 15</li>
                      <li>• TypeScript</li>
                      <li>• Tailwind CSS</li>
                    </ul>
                  </div>
                  <div>
                    <h4 className="font-semibold text-white/90 mb-2">后端</h4>
                    <ul className="space-y-1 text-white/70">
                      <li>• Next.js API Routes</li>
                      <li>• WebRTC</li>
                      <li>• WebSocket信令</li>
                      <li>• H.264编码</li>
                    </ul>
                  </div>
                </div>
              </div>
            </div>
          </div>

          {/* 错误显示 */}
          {connectionError && (
            <div className="mt-8 max-w-md mx-auto">
              <div className="bg-red-500/20 border border-red-500/30 rounded-lg p-4">
                <div className="flex items-center">
                  <span className="text-red-400 text-xl mr-3">⚠️</span>
                  <div>
                    <h4 className="text-red-200 font-semibold">连接错误</h4>
                    <p className="text-red-300 text-sm">{connectionError}</p>
                  </div>
                </div>
              </div>
            </div>
          )}
        </main>

        {/* API 信息 */}
        <footer className="mt-16 text-center">
          <div className="bg-white/5 backdrop-blur-sm rounded-xl p-6 border border-white/10">
            <h3 className="text-lg font-semibold text-white mb-4">📡 API 端点</h3>
            <div className="flex justify-center space-x-8 text-sm">
              <a 
                href="/api/health" 
            target="_blank"
                className="text-green-400 hover:text-green-300 transition-colors"
              >
                GET /api/health
          </a>
          <a
                href="/api/webrtc-config" 
            target="_blank"
                className="text-blue-400 hover:text-blue-300 transition-colors"
          >
                GET /api/webrtc-config
          </a>
        </div>
          </div>
      </footer>
      </div>
    </div>
  )
}
