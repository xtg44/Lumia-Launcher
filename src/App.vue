<script setup lang="ts">

import { ref, computed, onMounted, onUnmounted, watch, nextTick } from 'vue'
import HomeView from "./components/HomeView.vue";
import VersionSelector from "./components/VersionSelector.vue";
import DownloadView from "./components/DownloadView.vue";
import DownloadConfigView from "./components/DownloadConfigView.vue";
import SettingsView from "./components/SettingsView.vue";
import DownloadDetailPanel from "./components/DownloadDetailPanel.vue";
import ModpackInstallModal from "./components/ModpackInstallModal.vue";
import TerracottaView from "./components/TerracottaView.vue";
import AiAssistantView from "./components/AiAssistantView.vue";
import MusicView from "./components/MusicView.vue";
import PluginPage from "./components/PluginPage.vue";
import PluginCenter from "./components/PluginCenter.vue";
import PlayerBar from "./components/PlayerBar.vue";
import WhatsNewModal from "./components/WhatsNewModal.vue";
import UpdateModal from "./components/UpdateModal.vue";
import ToastHost from "./components/ToastHost.vue";
import ConfirmHost from "./components/ConfirmHost.vue";
import { getCurrentWindow } from '@tauri-apps/api/window'
import { convertFileSrc } from '@tauri-apps/api/core'
import { useI18n } from 'vue-i18n'
import { listenTouchBarAction, setTouchBarProgress, pluginList, getConfig, saveConfig } from './utils/tauri'
import type { PluginInfo } from './utils/tauri'
import { cleanSvg } from './utils/svg'
import homeIcon from './assets/icons/home.svg?raw'
import packageIcon from './assets/icons/package.svg?raw'
import downloadIcon from './assets/icons/download.svg?raw'
import internetIcon from './assets/icons/internet.svg?raw'
import musicIcon from './assets/icons/music.svg?raw'
import aiIcon from './assets/icons/ai.svg?raw'
import settingsIcon from './assets/icons/settings.svg?raw'
import pluginIcon from './assets/icons/plugin.svg?raw'

// 侧边栏图标（?raw 内联 + cleanSvg 统一尺寸，颜色跟随 currentColor）
const navIcons: Record<string, string> = {
  home: cleanSvg(homeIcon),
  version: cleanSvg(packageIcon),
  download: cleanSvg(downloadIcon),
  terracotta: cleanSvg(internetIcon),
  music: cleanSvg(musicIcon),
  ai: cleanSvg(aiIcon),
  settings: cleanSvg(settingsIcon),
  'plugin-center': cleanSvg(pluginIcon),
}

const { t } = useI18n()

type ViewType = 'home' | 'version' | 'download' | 'download-config' | 'settings' | 'terracotta' | 'ai' | 'music' | 'plugin' | 'plugin-center'

interface NavItem {
  label: string
  view: ViewType
  icon?: string
  pluginId?: string
}

const currentView = ref<ViewType>('home')
const currentPluginId = ref('')
const pluginRegistry = ref<PluginInfo[]>([])
const sidebarVisible = ref<Record<string, boolean>>({})
const themeColor = ref('#ffffff')
const bgBlur = ref(Number(localStorage.getItem('lumia-bg-blur')) || 0)
const showDownloadDetail = ref(false)
const overallDownloadProgress = ref(0)
const downloadDetailRef = ref<any>(null)
const versionSelectorRef = ref<any>(null)
const selectedVersionForConfig = ref('')
/** 从版本选择界面点击版本后，切到主页并选中的版本 */
const homeSelectedVersion = ref('')
const activeDownloadCount = ref(0)
/** 是否有进行中的启动任务（顶部进度条切换为「启动中」状态） */
const launchActive = ref(false)
const whatsNewModalRef = ref<any>(null)
const updateModalRef = ref<any>(null)
const homeViewRef = ref<any>(null)
let lastTouchBarProgress = -1

// ===== 整合包拖拽安装 =====
const showModpackModal = ref(false)
const modpackPath = ref('')

async function setupDragDrop() {
  try {
    const win = getCurrentWindow()
    await win.onDragDropEvent((event) => {
      if (event.payload.type === 'drop') {
        const file = event.payload.paths[0]
        if (!file) return
        const lower = file.toLowerCase()
        if (lower.endsWith('.mrpack') || lower.endsWith('.zip')) {
          modpackPath.value = file
          showModpackModal.value = true
        }
      }
    })
  } catch (err) {
    console.error('注册拖拽监听失败:', err)
  }
}

function handleModpackInstallRequest(payload: { path: string; name: string; mcVersion: string; customVersionName: string }) {
  showDownloadDetail.value = true
  nextTick(() => {
    downloadDetailRef.value?.addModpackTask({
      path: payload.path,
      displayName: payload.name,
      version: payload.mcVersion,
      customVersionName: payload.customVersionName,
    })
  })
}

const handleProgressUpdate = (progress: number) => {
  overallDownloadProgress.value = progress
  // 同步到 macOS Touch Bar 进度条（节流：差异 ≥2 才发送，避免高频刷屏）
  if (Math.abs(progress - lastTouchBarProgress) >= 2 || progress >= 100 || progress <= 0) {
    lastTouchBarProgress = progress
    setTouchBarProgress(progress).catch(() => {})
  }
}

const handleTasksUpdate = (count: number) => {
  activeDownloadCount.value = count
  // 全部任务完成 → Touch Bar 进度归零
  if (count === 0 && lastTouchBarProgress !== 0) {
    lastTouchBarProgress = 0
    setTouchBarProgress(0).catch(() => {})
  }
}

// ===== 启动任务接入下载中心（HomeView 上报的启动状态） =====
interface LaunchStatePayload {
  phase: 'start' | 'update' | 'success' | 'fail'
  version?: string
  stage?: string
  error?: string
}

function handleLaunchState(payload: LaunchStatePayload) {
  const panel = downloadDetailRef.value
  if (!panel) return
  switch (payload.phase) {
    case 'start':
      panel.addLaunchTask(payload.version || '')
      break
    case 'update':
      panel.updateLaunchStage(payload.stage || '')
      break
    case 'success':
      panel.completeLaunchTask()
      break
    case 'fail':
      panel.failLaunchTask(payload.error)
      break
  }
}

const addToDownloadCenter = async (config: {
  version: string
  displayName: string
  loader: 'none' | 'fabric' | 'forge' | 'neoforge'
  loaderVersion: string
  installFabricApi: boolean
}) => {
  showDownloadDetail.value = true
  await nextTick()
  downloadDetailRef.value?.addDownloadTask(config)
}

const navigateToConfig = (version: string) => {
  selectedVersionForConfig.value = version
  currentView.value = 'download-config'
}

const selectVersionToHome = (version: string) => {
  homeSelectedVersion.value = version
  currentView.value = 'home'
}

const navigateBackFromConfig = () => {
  currentView.value = 'download'
}

const handleKeydown = (e: KeyboardEvent) => {
  if (e.key === 'Escape' && showDownloadDetail.value) {
    showDownloadDetail.value = false
  }
}

// 关闭按钮
const handleClose = async () => {
  try {
    const window = getCurrentWindow()
    await window.close()
  } catch (error) {
    console.error('关闭窗口失败:', error)
  }
}

const handleTitlebarMouseDown = async (event: MouseEvent) => {
  if (event.button !== 0) return

  const target = event.target as HTMLElement
  if (target.closest('button, input, select, textarea, a, [data-no-drag]')) return

  try {
    await getCurrentWindow().startDragging()
  } catch (error) {
    console.error('拖动窗口失败:', error)
  }
}

// Touch Bar 动作处理（macOS）
let unlistenTouchBar: (() => void) | null = null
function handleTouchBarAction(action: string) {
  switch (action) {
    case 'launch':
      // 切到主页并调用 HomeView 的启动逻辑
      currentView.value = 'home'
      nextTick(() => {
        homeViewRef.value?.handleLaunch()
      })
      break
    case 'home':
      currentView.value = 'home'
      break
    case 'versions':
      handleNavClick({ label: t('app.navVersions'), view: 'version' })
      break
    case 'download':
      currentView.value = 'download'
      break
    case 'terracotta':
      currentView.value = 'terracotta'
      break
    case 'ai':
      currentView.value = 'ai'
      break
    case 'settings':
      currentView.value = 'settings'
      break
    case 'music':
      handleNavClick({ label: t('app.navMusic'), view: 'music' })
      break
    default:
      console.log('[Touch Bar] 未知动作:', action)
  }
}

onMounted(() => {
  const savedTheme = localStorage.getItem('lumia-theme')
  if (savedTheme) {
    themeColor.value = savedTheme
  }
  window.addEventListener('keydown', handleKeydown)
  // 禁用右键菜单
  window.addEventListener('contextmenu', (e) => e.preventDefault())
  setupDragDrop()
  // 监听 Touch Bar 按钮（macOS；其他平台事件不会到达）
  listenTouchBarAction(handleTouchBarAction).then((un) => {
    unlistenTouchBar = un
  }).catch((err) => {
    console.error('监听 Touch Bar 失败:', err)
  })
  // 检查版本更新日志
  nextTick(() => {
    whatsNewModalRef.value?.check()
  })
  // 自动检查新版本（尊重设置里的「自动检查更新」开关，默认开）
  getConfig().then((config) => {
    sidebarVisible.value = (config.sidebar_visible as Record<string, boolean>) || {}
    syncTouchBar()
    const enabled = config.auto_update !== false
    if (enabled) {
      nextTick(() => {
        updateModalRef.value?.check()
      })
    }
  }).catch(() => {
    // 读配置失败也照常检查
    nextTick(() => {
      updateModalRef.value?.check()
    })
  })
  // 加载插件注册表（侧边栏合并插件入口）
  loadPluginRegistry()
})

async function loadPluginRegistry() {
  try {
    pluginRegistry.value = await pluginList()
    // 当前正看的插件页若已被删除/停用 → 退回插件管理页
    if (currentView.value === 'plugin' && currentPluginId.value &&
        !pluginRegistry.value.some((p) => p.id === currentPluginId.value)) {
      currentPluginId.value = ''
      currentView.value = 'plugin-center'
    }
  } catch (err) {
    console.error('[插件] 加载注册表失败:', err)
  }
}

onUnmounted(() => {
  window.removeEventListener('keydown', handleKeydown)
  if (unlistenTouchBar) {
    unlistenTouchBar()
  }
})

watch(themeColor, (newTheme) => {
  localStorage.setItem('lumia-theme', newTheme)
  getConfig().then(config => {
    config.theme_color = newTheme
    saveConfig(config).catch(() => {})
  }).catch(() => {})
})

const navItems = computed(() => {
  const allBase: (NavItem & { key: string })[] = [
    { key: 'home', label: t('app.navHome'), view: 'home', icon: navIcons['home'] },
    { key: 'version', label: t('app.navVersions'), view: 'version', icon: navIcons['version'] },
    { key: 'download', label: t('app.navDownload'), view: 'download', icon: navIcons['download'] },
    { key: 'terracotta', label: t('app.navMultiplayer'), view: 'terracotta', icon: navIcons['terracotta'] },
    { key: 'music', label: t('app.navMusic'), view: 'music', icon: navIcons['music'] },
    { key: 'ai', label: t('app.navAi'), view: 'ai', icon: navIcons['ai'] },
    { key: 'settings', label: t('app.navSettings'), view: 'settings', icon: navIcons['settings'] },
  ]
  // 按 sidebar_visible 配置过滤（默认显示；settings 永远显示）
  const base = allBase.filter(item => item.key === 'settings' || sidebarVisible.value[item.key] !== false).map(({ key, ...rest }) => rest)
  // 插件注册表合并进侧边栏（启用中且有自有页面的插件；纯页面注入的不占侧边栏）
  const pluginItems: NavItem[] = pluginRegistry.value
    .filter((p) => p.enabled && p.hasControls)
    .map((p) => ({ label: p.name, view: 'plugin' as const, icon: navIcons['plugin-center'], pluginId: p.id }))
  // 插件管理入口（位于 ai 后面、settings 前面）
  // 找到 ai 和 settings 的位置，在中间插入
  const idxSettings = base.findIndex(item => item.view === 'settings')
  const head = idxSettings >= 0 ? base.slice(0, idxSettings) : base
  const tail = idxSettings >= 0 ? base.slice(idxSettings) : []
  return [
    ...head,
    ...(sidebarVisible.value['pluginCenter'] !== false ? [{ label: t('app.navPluginCenter'), view: 'plugin-center' as const, icon: navIcons['plugin-center'] }] : []),
    ...pluginItems,
    ...tail,
  ]
})

const navMenuRef = ref<HTMLElement | null>(null)
const navScrollTop = ref(0)
function onNavScroll() {
  navScrollTop.value = navMenuRef.value?.scrollTop || 0
}

const activeIndex = computed(() => {
  return navItems.value.findIndex(item => {
    if (item.view !== currentView.value) return false
    // 插件视图按 pluginId 精确匹配（多个插件项时避免错位）
    if (item.view === 'plugin') return item.pluginId === currentPluginId.value
    return true
  })
})

const handleNavClick = (item: NavItem) => {
  if (item.view === 'plugin' && item.pluginId) {
    currentPluginId.value = item.pluginId
    currentView.value = 'plugin'
    return
  }
  currentView.value = item.view
  if (item.view === 'version') {
    nextTick(() => {
      if (versionSelectorRef.value) {
        versionSelectorRef.value.loadVersions()
      }
    })
  }
}

function pluginNameOf(id: string): string {
  return pluginRegistry.value.find((p) => p.id === id)?.name || id
}

const handleDownloadComplete = () => {
  if (currentView.value === 'download') {
    currentView.value = 'version'
  }
  nextTick(() => {
    if (versionSelectorRef.value) {
      versionSelectorRef.value.loadVersions()
    }
  })
}

const handleThemeChange = (theme: string) => {
  themeColor.value = theme
}

// ===== 显式深色模式（持久化，独立于背景色亮度推断） =====
const darkMode = ref(localStorage.getItem('lumia-dark-mode') === '1')

function handleDarkModeChange(enabled: boolean) {
  darkMode.value = enabled
  localStorage.setItem('lumia-dark-mode', enabled ? '1' : '0')
}

function handleBlurChange(blur: number) {
  bgBlur.value = blur
}

async function handleSidebarChange() {
  try {
    const config = await getConfig()
    sidebarVisible.value = (config.sidebar_visible as Record<string, boolean>) || {}
    syncTouchBar()
  } catch {}
}

async function syncTouchBar() {
  const KEY_TO_ACTION: Record<string, string> = {
    home: 'home',
    version: 'versions',
    download: 'download',
    terracotta: 'terracotta',
    music: 'music',
    ai: 'ai',
    settings: 'settings',
  }
  const visible = Object.entries(KEY_TO_ACTION)
    .filter(([key]) => sidebarVisible.value[key] !== false)
    .map(([, action]) => action)
  try {
    const { touchbarUpdateActiveItems } = await import('./utils/tauri')
    await touchbarUpdateActiveItems(visible)
  } catch {}
}

// ===== 主题颜色处理 =====
function hexToRgb(hex: string) {
  const r = parseInt(hex.slice(1, 3), 16)
  const g = parseInt(hex.slice(3, 5), 16)
  const b = parseInt(hex.slice(5, 7), 16)
  return { r, g, b }
}

function toHex(r: number, g: number, b: number) {
  return `#${[r, g, b].map(v => v.toString(16).padStart(2, '0')).join('')}`
}

const isImageTheme = computed(() => {
  const tc = themeColor.value
  return tc && !tc.startsWith('#')
})

// 将主题色转换为可显示的背景 URL（文件路径 → asset URL，data: URL 原样返回）
function themeImageUrl(): string {
  const tc = themeColor.value
  if (!tc || tc.startsWith('#')) return ''
  if (tc.startsWith('data:')) return tc
  return convertFileSrc(tc)
}

const isDarkTheme = computed(() => {
  // 显式深色模式优先；否则按背景色亮度自动推断
  if (darkMode.value) return true
  if (isImageTheme.value) return false
  const { r, g, b } = hexToRgb(themeColor.value)
  return (0.299 * r + 0.587 * g + 0.114 * b) / 255 < 0.5
})

const textColor = computed(() => isDarkTheme.value ? '#ffffff' : '#1d1d1f')
const borderColor = computed(() => isDarkTheme.value ? 'rgba(255, 255, 255, 0.18)' : 'rgba(0, 0, 0, 0.1)')
const headerBg = computed(() => {
  return isDarkTheme.value ? 'rgba(0, 0, 0, 0.55)' : 'rgba(255, 255, 255, 0.72)'
})
const cardBg = computed(() => {
  return isDarkTheme.value ? 'rgba(255, 255, 255, 0.08)' : 'rgba(120, 120, 128, 0.12)'
})
const indicatorBg = computed(() => {
  return isDarkTheme.value ? 'rgba(255, 255, 255, 0.18)' : 'rgba(120, 120, 128, 0.16)'
})

const panelBg = computed(() => {
  const { r, g, b } = hexToRgb(themeColor.value)
  if (isDarkTheme.value) {
    return `rgb(${Math.round(r * 0.2 + 51)}, ${Math.round(g * 0.2 + 51)}, ${Math.round(b * 0.2 + 51)})`
  }
  return `rgb(${Math.round(r * 0.85)}, ${Math.round(g * 0.85)}, ${Math.round(b * 0.85)})`
})

// 深色模式的整窗背景：把所选主题色压暗，保证白字可读（不用亮色背景）
const darkThemeBg = computed(() => {
  const { r, g, b } = hexToRgb(themeColor.value)
  return `rgb(${Math.round(r * 0.12 + 40)}, ${Math.round(g * 0.12 + 40)}, ${Math.round(b * 0.12 + 40)})`
})

// 强调色
const accentColor = computed(() => {
  const { r, g, b } = hexToRgb(themeColor.value)
  const maxDiff = Math.max(Math.abs(r - g), Math.abs(g - b), Math.abs(r - b))
  if (maxDiff < 30) return isDarkTheme.value ? '#0a84ff' : '#007aff'
  const cr = 255 - r, cg = 255 - g, cb = 255 - b
  const cl = (0.299 * cr + 0.587 * cg + 0.114 * cb) / 255
  const scale = cl > 0.65 ? 0.55 / cl : cl < 0.35 ? 0.45 / cl : 1
  return toHex(Math.round(cr * scale), Math.round(cg * scale), Math.round(cb * scale))
})

const accentTextColor = computed(() => {
  const { r, g, b } = hexToRgb(accentColor.value)
  return (0.299 * r + 0.587 * g + 0.114 * b) / 255 > 0.55 ? '#000000' : '#ffffff'
})

const themeStyle = computed(() => {
  return {
    '--theme-color': themeColor.value,
    '--text-color': textColor.value,
    '--border-color': borderColor.value,
    '--header-bg': headerBg.value,
    '--card-bg': cardBg.value,
    '--indicator-bg': indicatorBg.value,
    '--sidebar-border': isDarkTheme.value ? 'rgba(255, 255, 255, 0.2)' : '#b3b3b3',
    '--progress-fill': accentColor.value,
    '--input-bg': isDarkTheme.value ? 'rgba(255, 255, 255, 0.1)' : '#ffffff',
    '--button-bg': isDarkTheme.value ? 'rgba(255, 255, 255, 0.1)' : '#e8e8ed',
    '--button-active': isDarkTheme.value ? 'rgba(255, 255, 255, 0.2)' : 'rgba(0, 0, 0, 0.2)',
    '--label-color': isDarkTheme.value ? 'rgba(255, 255, 255, 0.6)' : '#6e6e73',
    '--panel-bg': panelBg.value,
    '--popup-bg': isDarkTheme.value ? panelBg.value : '#ffffff',
    '--popup-shadow': isDarkTheme.value ? '0 16px 50px rgba(0, 0, 0, 0.5)' : '0 12px 40px rgba(0, 0, 0, 0.18)',
    '--accent-color': accentColor.value,
    '--accent-text-color': accentTextColor.value,
    background: isImageTheme.value
      ? (isDarkTheme.value
          ? `linear-gradient(rgba(0, 0, 0, 0.55), rgba(0, 0, 0, 0.55)), url(${themeImageUrl()})`
          : `url(${themeImageUrl()})`)
      : (isDarkTheme.value ? darkThemeBg.value : themeColor.value),
    backgroundSize: isImageTheme.value ? 'cover' : 'auto',
    backgroundPosition: isImageTheme.value ? 'center' : 'auto',
    color: textColor.value
  }
})

const bgLayerStyle = computed(() => ({
  background: isDarkTheme.value
    ? `linear-gradient(rgba(0, 0, 0, 0.55), rgba(0, 0, 0, 0.55)), url(${themeImageUrl()})`
    : `url(${themeImageUrl()})`,
  backgroundSize: 'cover',
  backgroundPosition: 'center',
  filter: `blur(${bgBlur.value}px)`,
  transform: 'scale(1.1)'
}))

</script>

<template>
  <div class="app-container" :class="{ 'has-blur': isImageTheme && bgBlur > 0 }" :style="themeStyle">
    <div v-if="isImageTheme && bgBlur > 0" class="bg-layer" :style="bgLayerStyle"></div>
    <header class="header" @mousedown.capture="handleTitlebarMouseDown">
      <div class="logo-wrapper">
        <img src="./assets/icons/Lumia.png" alt="Lumia" class="logo-image" />
        <span class="logo-text">Lumia</span>
      </div>
      <div class="download-header" :class="{ 'has-tasks': activeDownloadCount > 0 }">
        <span class="download-status">{{ launchActive ? t('downloadDetail.launching') : t('app.downloading', { count: activeDownloadCount }) }}</span>
        <div class="progress-bar">
          <div class="progress-fill" :class="{ 'indeterminate': launchActive }" :style="launchActive ? undefined : { width: overallDownloadProgress + '%' }"></div>
        </div>
        <button class="info-btn" @click="showDownloadDetail = true" :aria-label="t('app.ariaDownloadDetail')">
          ⓘ
        </button>
      </div>
      <button class="close-btn" @click.stop="handleClose" :aria-label="t('app.ariaCloseWindow')">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <line x1="18" y1="6" x2="6" y2="18"/>
          <line x1="6" y1="6" x2="18" y2="18"/>
        </svg>
      </button>
    </header>

    <div class="main-content">
      <aside class="sidebar">
        <div class="nav-indicator" :style="{ transform: `translateY(${activeIndex * 62 + 12 - navScrollTop}px)` }"></div>
        <nav class="nav-menu" ref="navMenuRef" @scroll="onNavScroll">
          <button
            v-for="item in navItems"
            :key="item.label"
            class="nav-item"
            @click="handleNavClick(item)"
          >
            <span v-if="item.icon" class="nav-icon" v-html="item.icon"></span>
            <span class="nav-label" :class="{ 'nav-label-small': item.view === 'version' }">{{ item.label }}</span>
          </button>
        </nav>
        </aside>

      <section class="content-area">
        <HomeView v-if="currentView === 'home'" ref="homeViewRef" :initial-version="homeSelectedVersion" @launch-state="handleLaunchState" />
        <VersionSelector v-else-if="currentView === 'version'" ref="versionSelectorRef" @select="selectVersionToHome" />
        <DownloadView v-else-if="currentView === 'download'" @download-request="navigateToConfig" />
        <DownloadConfigView 
          v-else-if="currentView === 'download-config'" 
          :version="selectedVersionForConfig"
          @back="navigateBackFromConfig"
          @confirm="addToDownloadCenter"
        />
        <SettingsView v-else-if="currentView === 'settings'" :dark-mode="darkMode" @theme-change="handleThemeChange" @dark-mode-change="handleDarkModeChange" @sidebar-change="handleSidebarChange" @blur-change="handleBlurChange" />
        <TerracottaView v-else-if="currentView === 'terracotta'" />
        <MusicView v-else-if="currentView === 'music'" />
        <AiAssistantView v-else-if="currentView === 'ai'" />
        <PluginCenter v-else-if="currentView === 'plugin-center'" @changed="loadPluginRegistry" />
        <PluginPage
          v-else-if="currentView === 'plugin'"
          :plugin-id="currentPluginId"
          :plugin-name="pluginNameOf(currentPluginId)"
        />
      </section>
    </div>

    <!-- 全局常驻播放条：切换页面不断播，游戏时也能听音乐 -->
    <PlayerBar />

    <DownloadDetailPanel 
      ref="downloadDetailRef"
      :visible="showDownloadDetail" 
      @close="showDownloadDetail = false"
      @download-complete="handleDownloadComplete"
      @progress-update="handleProgressUpdate"
      @tasks-update="handleTasksUpdate"
      @launch-active="launchActive = $event"
    />

    <ModpackInstallModal
      :visible="showModpackModal"
      :pack-path="modpackPath"
      @close="showModpackModal = false"
      @install-request="handleModpackInstallRequest"
    />

    <WhatsNewModal ref="whatsNewModalRef" />
    <UpdateModal ref="updateModalRef" />

    <ToastHost />
    <ConfirmHost />
  </div>
</template>

<style scoped>
* {
  user-select: none;
  -webkit-user-select: none;
  -moz-user-select: none;
  -ms-user-select: none;
}

.app-container {
  width: 100%;
  height: 100%;
  border-radius: 20px;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  position: relative;
  isolation: isolate;
  clip-path: inset(0 round 20px);
}

.bg-layer {
  position: absolute;
  inset: 0;
  pointer-events: none;
}

.app-container.has-blur .header,
.app-container.has-blur .main-content {
  position: relative;
  z-index: 1;
}

/* 注意：header、sidebar 等仍然需要背景色，否则内容不可见 */
.header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 4px 30px 3px 30px;
  background: var(--header-bg);
  border-bottom: 1px solid var(--border-color);
  cursor: grab;
}

.header:active {
  cursor: grabbing;
}
.download-header {
  display: none;
  align-items: center;
  gap: 12px;
  flex: 1;
  justify-content: center;
}

.download-header.has-tasks {
  display: flex;
}

.download-status {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 14px;
  color: var(--text-color);
  white-space: nowrap;
}

.progress-bar {
  width: 200px;
  height: 6px;
  background: var(--button-bg);
  border-radius: 3px;
  overflow: hidden;
}

.progress-fill {
  height: 100%;
  background: var(--progress-fill);
  border-radius: 3px;
  transition: width 0.3s ease;
}

.progress-fill.indeterminate {
  width: 40%;
  animation: progress-indeterminate 1.1s ease-in-out infinite;
}

@keyframes progress-indeterminate {
  0% { transform: translateX(-100%); }
  100% { transform: translateX(250%); }
}

.info-btn {
  background: none;
  border: none;
  cursor: pointer;
  padding: 6px;
  border-radius: 6px;
  transition: background-color 150ms ease;
  cursor: pointer;
}

.info-btn:active {
  transform: scale(0.92);
  background-color: var(--button-active);
}

.logo-wrapper {
  display: flex;
  align-items: center;
  gap: 8px;
}

.logo-image {
  width: 36px;
  height: 36px;
  object-fit: contain;
  border-radius: 8px;
}

.logo-text {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 32px;
  color: var(--text-color);
  letter-spacing: 0;
}

.close-btn {
  background: none;
  border: none;
  cursor: pointer;
  padding: 8px;
  border-radius: 8px;
  transition: background-color 150ms ease;
  cursor: pointer;
}

.close-btn:active {
  transform: scale(0.92);
  background-color: var(--button-active);
}

.close-btn svg {
  width: 24px;
  height: 24px;
  color: var(--text-color);
}

.main-content {
  display: flex;
  flex: 1;
  overflow: hidden;
}

.sidebar {
  width: 60px;
  display: flex;
  flex-direction: column;
  padding: 0;
  border-right: 1px solid var(--sidebar-border);
  flex-shrink: 0;
  position: relative;
}

.nav-indicator {
  position: absolute;
  top: 0;
  left: 5px;
  width: 50px;
  height: 50px;
  border-radius: 10px;
  background: var(--indicator-bg);
  transition: transform 0.4s cubic-bezier(0.34, 1.2, 0.64, 1);
  pointer-events: none;
}

.nav-menu {
  display: flex;
  flex-direction: column;
  align-items: center;
  padding: 2px 0 0;
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  overscroll-behavior: contain;
  scrollbar-width: none; /* Firefox：隐藏滚动条 */
}
.nav-menu::-webkit-scrollbar {
  display: none; /* Chrome/Safari：隐藏滚动条 */
}
.nav-menu::before,
.nav-menu::after {
  content: '';
  display: block;
  flex-shrink: 0;
}
.nav-menu::before {
  height: 6px; /* 顶部留白（已缩小），避免 indicator 顶格 */
}
.nav-menu::after {
  height: 16px; /* 底部缓冲 */
}

.nav-item {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  width: 50px;
  height: 58px;
  padding: 0;
  background: none;
  border: none;
  cursor: pointer;
  border-radius: 10px;
  margin-bottom: 4px;
  position: relative;
  z-index: 1;
  transition: transform 100ms ease;
}

.nav-item:active {
  transform: scale(0.95);
}

.nav-icon {
  width: 22px;
  height: 22px;
  display: block;
  color: var(--text-color);
  margin-bottom: 3px;
  flex-shrink: 0;
}

.nav-icon :deep(svg),
.nav-icon svg {
  width: 100%;
  height: 100%;
  display: block;
}

.nav-label {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 11px;
  color: var(--text-color);
  line-height: 13px;
  max-width: 50px;
  text-align: center;
  overflow-wrap: anywhere;
}

.nav-label-small {
  font-size: 10px;
  line-height: 13px;
  max-width: 50px;
  text-align: center;
  overflow-wrap: anywhere;
}

.content-area {
  flex: 1;
  display: flex;
}

@media (prefers-reduced-motion: reduce) {
  .nav-indicator {
    transition: transform 0.15s linear;
  }
  .nav-item:active,
  .close-btn:active {
    transform: none;
  }
}
</style>