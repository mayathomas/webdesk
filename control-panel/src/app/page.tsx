'use client'

import { useState, useCallback, useEffect } from 'react'
import { useActionState, useOptimistic } from 'react'
import ConnectionForm from '@/components/ConnectionForm'
import { RemoteDesktop } from '@/components/RemoteDesktop'
import { useWebRTCConnection } from '@/hooks/useWebRTCConnection'
import type { ConnectionState, VideoConfig, ActionState } from '@/types'

export default function HomePage() {
  const [isConnected, setIsConnected] = useState(false)
  const [clientId, setClientId] = useState('')
  const [authCode, setAuthCode] = useState('')
  const [remoteStream, setRemoteStream] = useState<MediaStream | null>(null)
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
    error: connectionError,
  } = useWebRTCConnection({
    onConnectionStateChange: (updater) => {
      setConnectionState(prev => updater(prev))
    },
    onConnected: () => setIsConnected(true),
    onDisconnected: () => {
      setIsConnected(false)
      setRemoteStream(null) // 清理视频流
      // 重置连接状态
      setConnectionState({
        signaling: 'disconnected',
        ice: 'disconnected',
        dataChannel: 'disconnected',
        video: 'disconnected'
      })
    },
    onStream: (stream) => setRemoteStream(stream)
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
  

  if (isConnected) {
    return (
      <RemoteDesktop
        clientId={clientId}
        authCode={authCode}
        connectionState={connectionState}
        isConnected={isConnected}
        disconnect={disconnectWebRTC}
        sendVideoConfig={sendVideoConfig}
        forceKeyframe={forceKeyframe}
        connectionError={connectionError}
        remoteStream={remoteStream}
      />
    )
  }

  // 断开连接后重置连接状态提示，避免显示"连接成功"
  const displayConnectionStatus = isConnected ? connectionStatus : { success: false, message: '' }

  return (
    <div className="min-h-screen bg-gradient-to-br from-purple-900 via-blue-900 to-indigo-900">
      <div className="container mx-auto px-4 py-8">
        <header className="text-center mb-12">
          <h1 className="text-5xl font-bold text-white mb-4">
            远程桌面控制
          </h1>
        </header>

        <main className="max-w-4xl mx-auto">
          <div className="grid grid-cols-1 lg:grid-cols-1 gap-8 items-start">
            {/* 连接表单 */}
            <div>
              <ConnectionForm
                action={connectAction}
                isConnecting={isConnecting}
                connectionStatus={displayConnectionStatus}
                connectionState={optimisticConnectionState}
              />
            </div>
          
          </div>
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
