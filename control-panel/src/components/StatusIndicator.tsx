interface StatusIndicatorProps {
  label: string
  status: 'disconnected' | 'connecting' | 'connected' | 'error'
}

export function StatusIndicator({ label, status }: StatusIndicatorProps) {
  const getStatusColor = (status: string) => {
    switch (status) {
      case 'connected':
        return 'bg-green-500'
      case 'connecting':
        return 'bg-yellow-500 animate-pulse'
      case 'error':
        return 'bg-red-500'
      default:
        return 'bg-gray-500'
    }
  }

  const getStatusText = (status: string) => {
    switch (status) {
      case 'connected':
        return '已连接'
      case 'connecting':
        return '连接中'
      case 'error':
        return '错误'
      default:
        return '未连接'
    }
  }

  return (
    <div className="flex items-center space-x-2">
      <div className={`w-3 h-3 rounded-full ${getStatusColor(status)}`}></div>
      <div className="text-sm">
        <div className="text-white font-medium">{label}</div>
        <div className="text-gray-400">{getStatusText(status)}</div>
      </div>
    </div>
  )
} 