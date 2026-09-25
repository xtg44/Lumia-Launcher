import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'

export async function cancelDownload(): Promise<void> {
  await invoke('cancel_download')
}

export async function getIconUrl(filename: string): Promise<string> {
  return `${import.meta.env.BASE_URL}icons/${filename}`
}

export interface VersionManifestItem {
  id: string
  releaseTime: string
  category: string
}

export async function getVersions(): Promise<VersionManifestItem[]> {
  return await invoke('get_versions')
}

export interface LocalVersionInfo {
  name: string
  loader: 'vanilla' | 'fabric' | 'forge' | 'neoforge' | 'quilt'
}

export async function getLocalVersions(): Promise<LocalVersionInfo[]> {
  return await invoke('get_local_versions')
}

export async function getRequiredJava(version: string): Promise<number> {
  return await invoke('get_required_java', { version })
}

export interface DownloadProgressPayload {
  stage: string
  progress: number
  downloaded?: number
  total?: number
}

export async function downloadGame(config: {
  version: string
  displayName: string
  loader: 'none' | 'fabric' | 'forge' | 'neoforge'
  loaderVersion: string
  installFabricApi: boolean
}, onProgress?: (payload: DownloadProgressPayload) => void): Promise<void> {
  // 组件层负责统一监听 download-progress（见 DownloadDetailPanel），这里只负责发起下载。
  // 兼容旧调用：若外部传入 onProgress，则临时注册一个监听器。
  let unlisten: (() => void) | undefined
  if (onProgress) {
    unlisten = await listen('download-progress', (event) => {
      const payload = event.payload as DownloadProgressPayload
      if (payload && typeof (payload as any).name === 'string' && (payload as any).name !== config.displayName) return
      onProgress(payload)
    })
  }
  try {
    await invoke('download_game', { config })
    console.log('[downloadGame] invoke 返回成功', config.displayName)
  } finally {
    unlisten?.()
  }
}

export async function listenDownloadProgress(callback: (data: any) => void): Promise<() => void> {
  return await listen('download-progress', (event) => {
    callback(event.payload)
  })
}

export async function launchGame(params: {
  version: string
  username: string
  java_path?: string
  max_memory?: number
}): Promise<void> {
  return await invoke('launch_game', params)
}

// ========== Microsoft 认证 ==========

export interface AuthResult {
  success: boolean
  uuid?: string
  username?: string
  access_token?: string
  error?: string
  user_code?: string
  verification_uri?: string
  message?: string
}

export async function startDeviceAuth(): Promise<AuthResult> {
  return await invoke('start_device_auth')
}

export async function pollDeviceAuth(): Promise<AuthResult> {
  return await invoke('poll_device_auth')
}

export async function checkAuthStatus(): Promise<AuthResult> {
  return await invoke('check_auth_status')
}

export async function logout(): Promise<void> {
  return await invoke('logout')
}

export async function listenLaunchStatus(callback: (data: any) => void): Promise<() => void> {
  return await listen('launch-status', (event) => {
    callback(event.payload)
  })
}

export async function openFolder(): Promise<void> {
  return await invoke('open_folder')
}

export async function openCrashLogFile(): Promise<void> {
  return await invoke('open_crash_log_file')
}

export interface CrashInfo {
  description: string
  entity_type: string | null
  entity_name: string | null
  entity_location: string | null
  block_type: string | null
  block_location: string | null
  error_type: string
  stack_summary: string
  raw_section: string | null
}

export async function parseCrashReport(): Promise<CrashInfo> {
  return await invoke('parse_crash_report')
}

export async function getFabricVersions(version: string): Promise<string[]> {
  return await invoke('get_fabric_versions', { version })
}

export async function getForgeVersions(version: string): Promise<string[]> {
  return await invoke('get_forge_versions', { version })
}

export async function getNeoForgeVersions(version: string): Promise<string[]> {
  return await invoke('get_neoforge_versions', { version })
}


export async function getConfig(): Promise<any> {
  return await invoke('get_config')
}

export async function saveConfig(config: any): Promise<void> {
  return await invoke('save_config', { config })
}

export async function saveBackgroundImage(base64Data: string, fileName: string): Promise<string> {
  return await invoke('save_background_image', { base64Data, fileName })
}

export async function removeBackgroundImage(path: string): Promise<void> {
  return await invoke('remove_background_image', { path })
}

export async function downloadFile(url: string, savePath: string): Promise<string> {
  return await invoke('download_file', { url, savePath })
}

export async function listenDownloadFileProgress(callback: (payload: any) => void): Promise<() => void> {
  return await listen('download-file-progress', (event) => {
    callback(event.payload)
  })
}

export async function getJavaPaths(): Promise<string[]> {
  return await invoke('get_java_paths')
}

// ============ 模组 ============
export interface ModSearchItem {
  id: string
  name: string
  summary: string
  iconUrl: string
  source: 'modrinth' | 'curseforge'
  loaders: string[]
  gameVersions: string[]
  downloads: number
  updatedAt: string
}

export async function searchMods(params: {
  query: string
  loader: string
  mcVersion: string
  source: string
  offset: number
  limit: number
}): Promise<ModSearchItem[]> {
  return await invoke('search_mods', {
    query: params.query,
    loader: params.loader,
    mcVersion: params.mcVersion,
    source: params.source,
    offset: params.offset,
    limit: params.limit,
  })
}

export interface ModSupportInfo {
  supported: boolean
  reason?: string | null
  dependencies: string[]
}

export async function checkModSupport(projectId: string, versionName: string, source: string): Promise<ModSupportInfo> {
  return await invoke('check_mod_support', { projectId, versionName, source })
}

export interface ModInstallResult {
  fileName: string
  dependencies: string[]
}

export async function installMod(projectId: string, versionName: string, source: string): Promise<ModInstallResult> {
  return await invoke('install_mod', { projectId, versionName, source })
}

export async function searchResourcePacks(params: {
  query: string
  mcVersion: string
  offset: number
  limit: number
}): Promise<ModSearchItem[]> {
  return await invoke('search_resource_packs', {
    query: params.query,
    mcVersion: params.mcVersion,
    offset: params.offset,
    limit: params.limit,
  })
}

export async function installResourcePack(projectId: string, versionName: string): Promise<ModInstallResult> {
  return await invoke('install_resource_pack', { projectId, versionName })
}

export async function searchShaderPacks(params: {
  query: string
  mcVersion: string
  offset: number
  limit: number
}): Promise<ModSearchItem[]> {
  return await invoke('search_shader_packs', {
    query: params.query,
    mcVersion: params.mcVersion,
    offset: params.offset,
    limit: params.limit,
  })
}

export async function installShaderPack(projectId: string, versionName: string): Promise<ModInstallResult> {
  return await invoke('install_shader_pack', { projectId, versionName })
}

export async function openUrl(url: string): Promise<void> {
  await invoke('open_url', { url })
}

// ============ 版本管理 ============
export interface ModFileInfo {
  fileName: string
  displayName: string
  enabled: boolean
  size: number
}

export async function renameVersion(oldName: string, newName: string): Promise<void> {
  await invoke('rename_version', { oldName, newName })
}

export async function deleteVersion(name: string): Promise<void> {
  await invoke('delete_version', { name })
}

export async function listMods(version: string): Promise<ModFileInfo[]> {
  return await invoke('list_mods', { version })
}

export async function toggleMod(version: string, fileName: string, enabled: boolean): Promise<void> {
  await invoke('toggle_mod', { version, fileName, enabled })
}

export async function deleteMod(version: string, fileName: string): Promise<void> {
  await invoke('delete_mod', { version, fileName })
}

export async function openModsFolder(version: string): Promise<void> {
  await invoke('open_mods_folder', { version })
}

export async function getOfflineUuid(username: string): Promise<{ uuid: string; skinIndex: number; skinName: string; slim: boolean }> {
  return await invoke('get_offline_uuid', { username })
}

// ============ 整合包 ============
export interface ModpackInfo {
  format: 'modrinth' | 'curseforge'
  name: string
  mcVersion: string
  loader: string
  loaderVersion: string
  fileCount: number
}

export async function inspectModpack(path: string): Promise<ModpackInfo> {
  return await invoke('inspect_modpack', { path })
}

export async function installModpack(path: string, customVersionName: string): Promise<string> {
  return await invoke('install_modpack', { path, customVersionName })
}

// ============ 陶瓦联机（Terracotta） ============
export async function terracottaStart(): Promise<number> {
  return await invoke('terracotta_start')
}

export async function terracottaStatus(port: number): Promise<any> {
  return await invoke('terracotta_status', { port })
}

export async function terracottaHost(port: number, playerName: string, customNodes?: string[]): Promise<any> {
  return await invoke('terracotta_host', { port, playerName, customNodes })
}

export async function terracottaJoin(port: number, room: string, playerName: string, customNodes?: string[]): Promise<any> {
  return await invoke('terracotta_join', { port, room, playerName, customNodes })
}

export interface TerracottaNodeStatus {
  url: string
  target: string
  /** true=可达 false=明确不可达 null=未知（网关 GET 失败等，不算挂） */
  ok: boolean | null
}

/** 明确不可达（不是未知） */
export function nodeDefinitelyDown(n: TerracottaNodeStatus): boolean {
  return n.ok === false
}

/** 公共节点可达性检查（默认节点 + 传入的自定义节点） */
export async function terracottaNodesStatus(customNodes?: string[]): Promise<TerracottaNodeStatus[]> {
  return await invoke('terracotta_nodes_status', { customNodes })
}

export async function terracottaStop(): Promise<void> {
  await invoke('terracotta_stop')
}

/** 游戏是否在运行（用于陶瓦联机开房前提示） */
export async function isGameRunning(): Promise<boolean> {
  return await invoke('is_game_running')
}

/** 停止扫描/退出房间，回到等待状态 */
export async function terracottaBack(port: number): Promise<any> {
  return await invoke('terracotta_back', { port })
}

/** 离线模式是否允许（仅中国大陆 IP 允许，Mojang 许可条款合规） */
export async function offlineModeAllowed(): Promise<boolean> {
  return await invoke('offline_mode_allowed')
}

// ============ AI 助手 ============
export interface AiChatMessage {
  role: 'user' | 'assistant'
  content: string
}

export interface AiToolCall {
  id: string
  name: string
  arguments: any
}

export interface AiUsage {
  promptTokens: number
  completionTokens: number
  totalTokens: number
}

export interface AiChatResult {
  reply: string
  /** 是否注入了 Wiki 参考资料（只有此时才需要标注来源） */
  usedWiki: boolean
  /** 本次请求的 token 用量 */
  usage: AiUsage
  toolCalls: AiToolCall[]
}

export interface AiFileOp {
  action: 'delete_file' | 'write_file'
  path: string
  content?: string
}

/** 发送聊天消息（后端注入 Wiki 上下文；返回回复 + 可能存在的文件操作提议） */
export async function aiChat(messages: AiChatMessage[]): Promise<AiChatResult> {
  return await invoke('ai_chat', { messages })
}

/** 执行用户已批准的文件操作（仅限游戏目录内） */
export async function aiExecuteFileOps(ops: AiFileOp[]): Promise<any> {
  return await invoke('ai_execute_file_ops', { ops })
}

export interface AiModelList {
  models: string[]
}

/** 获取指定提供商可用的模型列表（OpenAI 兼容 / Anthropic，参数为当前表单值，无需先保存） */
export async function listAiModels(baseUrl: string, apiKey: string, provider: string): Promise<AiModelList> {
  return await invoke('list_ai_models', { baseUrl, apiKey, provider })
}

// ============ 插件（Lumi） ============
export interface PluginInfo {
  id: string
  name: string
  version: string
  description: string
  icon: string
  enabled: boolean
  builtin: boolean
  hasControls: boolean
}

/** 插件列表（扫描 plugins/ 目录，含 .lplugin 压缩包与解压文件夹） */
export async function pluginList(): Promise<PluginInfo[]> {
  return await invoke('plugin_list')
}

/** 插件控件树（渲染插件页用） */
export async function pluginUiTree(id: string): Promise<PluginControlNode[]> {
  return await invoke('plugin_ui_tree', { id })
}

export interface PluginControlNode {
  name: string
  type: 'button' | 'text' | 'input' | 'list' | 'toggle' | 'image' | 'line'
  text: string
  layout: 'left' | 'center' | 'right' | 'top' | 'bottom'
  list: string[]
  checked: boolean
  selected: string | null
  lineLength: number | null
  portrait: boolean
}

export interface PluginNotify {
  kind: 'toast' | 'popup'
  text: string
}

export interface PluginEventResult {
  notify: PluginNotify[]
  tree: PluginControlNode[]
}

/** 触发插件控件事件（点击/切换等），返回执行后的控件树与通知 */
export async function pluginUiEvent(id: string, control: string, event: string): Promise<PluginEventResult> {
  return await invoke('plugin_ui_event', { id, control, event })
}

export interface PageInjectEntry {
  pluginId: string
  pluginName: string
  controls: PluginControlNode[]
  overrides: Record<string, string>
}

/** 查询所有启用插件对目标页面的控件注入 */
export async function pluginPageInjects(page: string): Promise<PageInjectEntry[]> {
  return await invoke('plugin_page_injects', { page })
}

/** 启用/停用插件（停用后侧边栏不显示入口，事件分发拒绝） */
export async function pluginSetEnabled(id: string, enabled: boolean): Promise<void> {
  return await invoke('plugin_set_enabled', { id, enabled })
}

/** 删除插件：移除注册表 + 解压目录 + 源 .lplugin 包 */
export async function pluginDelete(id: string): Promise<void> {
  return await invoke('plugin_delete', { id })
}

/** 用系统文件管理器打开插件目录 */
export async function pluginOpenDir(id: string): Promise<void> {
  return await invoke('plugin_open_dir', { id })
}

/** 系统事件广播：通知所有启用的插件执行 listen 系统事件的块（如 game_quit） */
export async function pluginEmitEvent(event: string): Promise<{ notify: PluginNotify[] }> {
  return await invoke('plugin_emit_event', { event })
}

// ============ 本地音乐库 ============
export interface MusicSong {
  filename: string
  /** 播放地址：stream://localhost/<歌单>/<文件>（已编码） */
  relPath: string
  size: number
}

export interface MusicPlaylist {
  name: string
  songs: MusicSong[]
}

export interface MusicImportResult {
  imported: number
  skipped: number
}

/** 按路径导入多个音频/视频文件（系统文件选择器拿绝对路径；后端磁盘复制进应用数据目录，源文件删除不影响） */
export async function musicImportPaths(playlist: string, paths: string[]): Promise<MusicImportResult> {
  return await invoke('music_import_paths', { playlist, paths })
}

/** 导入整个文件夹为一个歌单（歌单名 = 文件夹名；后端递归复制文件夹内所有音频/视频） */
export async function musicImportFolder(folder: string): Promise<MusicImportResult> {
  return await invoke('music_import_folder', { folder })
}

/** 获取全部歌单与歌曲 */
export async function musicList(): Promise<MusicPlaylist[]> {
  return await invoke('music_list')
}

/** 删除整个歌单 */
export async function musicDeletePlaylist(playlist: string): Promise<void> {
  await invoke('music_delete_playlist', { playlist })
}

/** 从歌单中删除一首歌 */
export async function musicRemoveSong(playlist: string, filename: string): Promise<void> {
  await invoke('music_remove_song', { playlist, filename })
}

/** 本地音乐播放 URL（无需后端命令，直接走 stream:// 协议） */
export function musicStreamUrl(relPath: string): string {
  // relPath 形如 "歌单/文件名.mp3"，每段 URL 编码
  const encoded = relPath.split('/').map((seg) => encodeURIComponent(seg)).join('/')
  return `stream://localhost/${encoded}`
}

/** 获取应用版本号（Cargo.toml 中的版本） */
export async function getAppVersion(): Promise<string> {
  return await invoke('get_app_version')
}

export interface UpdateCheckInfo {
  hasUpdate: boolean
  current: string
  latest: string
  channel: string
  beta: string | null
  stable: string | null
  betaDownload: string | null
  stableDownload: string | null
  /** 按平台返回的下载地址（API 新增） */
  winDownload: string | null
  macDownload: string | null
  /** 后端已按当前运行平台解析好的下载地址，优先使用 */
  downloadUrl: string | null
}

/** 自动检查新版本（后端请求官方版本 API 并比较） */
export async function checkForUpdates(): Promise<UpdateCheckInfo> {
  return await invoke('check_for_updates')
}

/** 标记更新日志已读（写入配置中的 last_seen_version） */
export async function markChangelogSeen(version: string): Promise<void> {
  await invoke('mark_changelog_seen', { version })
}

// ============ Touch Bar（macOS） ============
/** 监听 Touch Bar 按钮点击（动作名：launch / home / versions / download / terracotta / ai / settings / music） */
export async function listenTouchBarAction(callback: (action: string) => void): Promise<() => void> {
  return await listen('touchbar-action', (event) => {
    callback(event.payload as string)
  })
}

/** 更新 Touch Bar 下载进度条（0-100） */
export async function setTouchBarProgress(progress: number): Promise<void> {
  await invoke('touchbar_set_progress', { progress })
}

/** 同步 Touch Bar 可见按钮（与侧边栏显示同步） */
export async function touchbarUpdateActiveItems(active: string[]): Promise<void> {
  await invoke('touchbar_update_active_items', { active })
}

// 关闭窗口（使用 Tauri API）
export async function closeMainWindow(): Promise<void> {
  const window = getCurrentWindow()
  await window.close()
}