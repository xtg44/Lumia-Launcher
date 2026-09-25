// 模组下载相关类型定义

export type ModSource = 'curseforge' | 'modrinth'
export type ModLoaderType = 'forge' | 'fabric' | 'neoforge' | 'quilt'

export interface ModInfo {
  /** 唯一标识（slug / project id） */
  id: string
  name: string
  summary: string
  iconUrl: string
  source: ModSource
  /** 支持的加载器（可能多个） */
  loaders: ModLoaderType[]
  /** 支持的 MC 版本 */
  gameVersions: string[]
  /** 总下载量 */
  downloads: number
  /** 最近更新时间（ISO 字符串） */
  updatedAt: string
}

export interface InstalledMod {
  /** 文件名（含 .jar） */
  fileName: string
  name: string
  iconUrl: string
  enabled: boolean
}

export interface ModSearchFilters {
  keyword: string
  loader: '' | ModLoaderType
  version: string
  source: '' | ModSource
}
