import { NextResponse } from 'next/server'
import * as yaml from 'js-yaml'
import * as fs from 'fs'
import * as path from 'path'
import type { VideoProfile, WebRTCConfigYAML, ApiResponse } from '@/types'

export async function GET() {
  try {
    const configPath = path.join(process.cwd(), 'webrtc-config.yaml')
    
    if (!fs.existsSync(configPath)) {
      // 返回默认视频配置档案
      const defaultProfiles: VideoProfile[] = [
        { name: '720p30', width: 1280, height: 720, fps: 30, bitrate: 1500000 },
        { name: '1080p25', width: 1920, height: 1080, fps: 25, bitrate: 2500000 },
        { name: '1080p30', width: 1920, height: 1080, fps: 30, bitrate: 4000000 }
      ]

      const response: ApiResponse<VideoProfile[]> = {
        success: true,
        data: defaultProfiles,
        message: '使用默认视频配置档案',
        timestamp: new Date().toISOString()
      }

      return NextResponse.json(response)
    }

    // 读取YAML配置文件
    const configFile = fs.readFileSync(configPath, 'utf8')
    const yamlConfig = yaml.load(configFile) as WebRTCConfigYAML

    const videoProfiles: VideoProfile[] = yamlConfig.video_config.profiles.map(profile => ({
      name: profile.name,
      width: profile.width,
      height: profile.height,
      fps: profile.fps,
      bitrate: profile.bitrate
    }))

    const response: ApiResponse<VideoProfile[]> = {
      success: true,
      data: videoProfiles,
      message: '视频配置档案获取成功',
      timestamp: new Date().toISOString()
    }

    return NextResponse.json(response)

  } catch (error) {
    console.error('获取视频配置档案失败:', error)
    
    const errorResponse: ApiResponse = {
      success: false,
      message: error instanceof Error ? error.message : '获取视频配置档案失败',
      timestamp: new Date().toISOString()
    }

    return NextResponse.json(errorResponse, { status: 500 })
  }
} 