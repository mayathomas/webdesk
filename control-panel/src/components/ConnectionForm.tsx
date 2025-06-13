'use client'

import { useFormStatus } from 'react-dom'
import type { ActionState, ConnectionState } from '@/types'

interface ConnectionFormProps {
  action: (formData: FormData) => void
  isConnecting: boolean
  connectionStatus: ActionState
  connectionState: ConnectionState
}

function SubmitButton() {
  const { pending } = useFormStatus()
  
  return (
    <button
      type="submit"
      disabled={pending}
      className="w-full bg-blue-600 hover:bg-blue-700 disabled:bg-gray-400 text-white font-semibold py-3 px-6 rounded-lg transition-colors duration-200"
    >
      {pending ? '🔗 连接中...' : '🚀 连接远程桌面'}
    </button>
  )
}

export default function ConnectionForm({
  action,
  isConnecting,
  connectionStatus,
  connectionState
}: ConnectionFormProps) {
  return (
    <div className="max-w-md mx-auto">
      <div className="bg-white/10 backdrop-blur-lg rounded-2xl p-8 border border-white/20">
        <div className="text-center mb-8">
          <h2 className="text-2xl font-bold text-white mb-2">🎬 远程连接</h2>
          <p className="text-white/80">连接到远程H.264视频流</p>
        </div>

        <form action={action} className="space-y-6">
          <div>
            <label htmlFor="clientId" className="block text-sm font-medium text-white/90 mb-2">
              客户端ID
            </label>
            <input
              type="text"
              id="clientId"
              name="clientId"
              required
              placeholder="输入远程客户端ID"
              className="w-full px-4 py-3 bg-white/10 border border-white/20 rounded-lg text-white placeholder-white/50 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent"
            />
          </div>

          <div>
            <label htmlFor="authCode" className="block text-sm font-medium text-white/90 mb-2">
              验证码
            </label>
            <input
              type="password"
              id="authCode"
              name="authCode"
              required
              placeholder="输入验证码"
              className="w-full px-4 py-3 bg-white/10 border border-white/20 rounded-lg text-white placeholder-white/50 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent"
            />
          </div>

          <div>
            <label htmlFor="videoQuality" className="block text-sm font-medium text-white/90 mb-2">
              视频质量
            </label>
            <select
              id="videoQuality"
              name="videoQuality"
              className="w-full px-4 py-3 bg-white/10 border border-white/20 rounded-lg text-white focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent"
            >
              <option value="720p30" className="bg-gray-800">720p 30fps (推荐)</option>
              <option value="1080p25" className="bg-gray-800">1080p 25fps (平衡)</option>
              <option value="1080p30" className="bg-gray-800">1080p 30fps (高质量)</option>
            </select>
          </div>

          <SubmitButton />
        </form>

        {/* 连接状态显示 */}
        {connectionStatus.message && (
          <div className={`mt-4 p-3 rounded-lg ${
            connectionStatus.success 
              ? 'bg-green-500/20 border border-green-500/30' 
              : 'bg-red-500/20 border border-red-500/30'
          }`}>
            <p className={`text-sm ${
              connectionStatus.success ? 'text-green-200' : 'text-red-200'
            }`}>
              {connectionStatus.message}
            </p>
          </div>
        )}

        {/* 连接进度 */}
        {isConnecting && (
          <div className="mt-6 space-y-3">
            <div className="text-center text-white/80 text-sm">连接进度</div>
            <div className="space-y-2">
              <div className="flex items-center justify-between">
                <span className="text-sm text-white/70">信令服务器</span>
                <StatusDot status={connectionState.signaling} />
              </div>
              <div className="flex items-center justify-between">
                <span className="text-sm text-white/70">ICE连接</span>
                <StatusDot status={connectionState.ice} />
              </div>
              <div className="flex items-center justify-between">
                <span className="text-sm text-white/70">数据通道</span>
                <StatusDot status={connectionState.dataChannel} />
              </div>
              <div className="flex items-center justify-between">
                <span className="text-sm text-white/70">视频流</span>
                <StatusDot status={connectionState.video} />
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  )
}

function StatusDot({ status }: { status: string }) {
  const getStatusInfo = (status: string) => {
    switch (status) {
      case 'connected':
        return { color: 'bg-green-500', pulse: false, text: '已连接' }
      case 'connecting':
        return { color: 'bg-yellow-500', pulse: true, text: '连接中' }
      case 'error':
        return { color: 'bg-red-500', pulse: false, text: '错误' }
      default:
        return { color: 'bg-gray-500', pulse: false, text: '未连接' }
    }
  }

  const { color, pulse, text } = getStatusInfo(status)

  return (
    <div className="flex items-center space-x-2">
      <div className={`w-3 h-3 rounded-full ${color} ${pulse ? 'animate-pulse' : ''}`}></div>
      <span className="text-xs text-white/60">{text}</span>
    </div>
  )
} 