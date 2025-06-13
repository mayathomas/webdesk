import { NextResponse } from 'next/server'
import type { ApiResponse } from '@/types'

export async function GET() {
  const healthData = {
    status: 'ok',
    service: 'remote-control-panel',
    version: '1.0.0',
    timestamp: new Date().toISOString(),
    uptime: process.uptime(),
    environment: process.env.NODE_ENV || 'development',
    nodeVersion: process.version,
    platform: process.platform,
    memory: {
      used: Math.round(process.memoryUsage().heapUsed / 1024 / 1024),
      total: Math.round(process.memoryUsage().heapTotal / 1024 / 1024),
      external: Math.round(process.memoryUsage().external / 1024 / 1024)
    }
  }

  const response: ApiResponse<typeof healthData> = {
    success: true,
    data: healthData,
    message: '服务运行正常',
    timestamp: new Date().toISOString()
  }

  return NextResponse.json(response)
} 