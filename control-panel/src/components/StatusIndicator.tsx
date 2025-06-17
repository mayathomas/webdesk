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

  const getTextColor = (status: string) => {
    switch (status) {
      case 'connected':
        return 'text-green-300'
      case 'connecting':
        return 'text-yellow-300'
      case 'error':
        return 'text-red-300'
      default:
        return 'text-gray-300'
    }
  }

  return (
    <div className="flex items-center space-x-2">
      <div className={`w-2 h-2 rounded-full ${getStatusColor(status)}`}></div>
      <div className="text-xs">
        <div className="text-white/80 font-medium">{label}</div>
        <div className={`${getTextColor(status)} font-medium`}>{getStatusText(status)}</div>
      </div>
    </div>
  )
} 