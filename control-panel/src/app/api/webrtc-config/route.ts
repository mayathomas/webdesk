import { NextResponse } from 'next/server'
import * as yaml from 'js-yaml'
import * as fs from 'fs'
import * as path from 'path'
import type { WebRTCConfig, WebRTCConfigYAML, ApiResponse } from '@/types'

// 缓存配置以避免重复读取文件
let cachedConfig: WebRTCConfig | null = null
let configLastModified: number = 0

function loadWebRTCConfig(): WebRTCConfig {
  try {
    const configPath = path.join(process.cwd(), 'webrtc-config.yaml')
    
    // 检查文件是否存在
    if (!fs.existsSync(configPath)) {
      throw new Error(`配置文件不存在: ${configPath}`)
    }

    // 检查文件修改时间，如果没有变化则返回缓存
    const stats = fs.statSync(configPath)
    if (cachedConfig && stats.mtimeMs === configLastModified) {
      return cachedConfig
    }

    // 读取YAML文件
    const configFile = fs.readFileSync(configPath, 'utf8')
    const yamlConfig = yaml.load(configFile) as WebRTCConfigYAML

    // 构建ICE服务器列表
    const iceServers: RTCIceServer[] = []

    // 添加STUN服务器
    if (yamlConfig.stun_servers) {
      for (const stunServer of yamlConfig.stun_servers) {
        iceServers.push({
          urls: stunServer.url
        })
      }
    }

    // 添加TURN服务器
    if (yamlConfig.turn_servers) {
      for (const turnServer of yamlConfig.turn_servers) {
        iceServers.push({
          urls: turnServer.url,
          username: turnServer.username,
          credential: turnServer.credential
        })
      }
    }

    // 构建WebRTC配置
    const webrtcConfig: WebRTCConfig = {
      iceServers,
      signalsServerUrl: process.env.SIGNALS_SERVER_URL || yamlConfig.signals_server.url,
      dataChannelConfig: {
        ordered: yamlConfig.data_channel.ordered,
        maxRetransmits: yamlConfig.data_channel.max_retransmits,
        ...(yamlConfig.data_channel.max_packet_life_time && {
          maxPacketLifeTime: yamlConfig.data_channel.max_packet_life_time
        })
      }
    }

    // 更新缓存
    cachedConfig = webrtcConfig
    configLastModified = stats.mtimeMs

    console.log('✅ WebRTC配置加载成功:', {
      stunServers: yamlConfig.stun_servers?.length || 0,
      turnServers: yamlConfig.turn_servers?.length || 0,
      signalsServer: webrtcConfig.signalsServerUrl
    })

    return webrtcConfig

  } catch (error) {
    console.error('❌ 加载WebRTC配置失败:', error)
    
    // 返回默认配置作为备选
    const fallbackConfig: WebRTCConfig = {
      iceServers: [
        {
          urls: [
            'stun:stun.l.google.com:19302',
            'stun:stun1.l.google.com:19302',
            'stun:stun2.l.google.com:19302',
            'stun:stun3.l.google.com:19302',
          ]
        }
      ],
      signalsServerUrl: process.env.SIGNALS_SERVER_URL || 'ws://localhost:8081',
      dataChannelConfig: {
        ordered: true,
        maxRetransmits: 3
      }
    }

    console.log('⚠️ 使用默认WebRTC配置')
    return fallbackConfig
  }
}

export async function GET() {
  try {
    const webrtcConfig = loadWebRTCConfig()

    const response: ApiResponse<WebRTCConfig> = {
      success: true,
      data: webrtcConfig,
      message: 'WebRTC配置获取成功',
      timestamp: new Date().toISOString()
    }

    return NextResponse.json(response)
  } catch (error) {
    console.error('获取WebRTC配置失败:', error)
    
    const errorResponse: ApiResponse = {
      success: false,
      message: error instanceof Error ? error.message : '获取WebRTC配置失败',
      timestamp: new Date().toISOString()
    }

    return NextResponse.json(errorResponse, { status: 500 })
  }
} 