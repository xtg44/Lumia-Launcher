<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { getConfig, saveConfig, getJavaPaths, getAppVersion, listAiModels, saveBackgroundImage, removeBackgroundImage, downloadFile, listenDownloadFileProgress } from '../utils/tauri'
import { useI18n } from 'vue-i18n'
import { resolveLocale, applyLocale } from '../i18n'
import { toast } from '../utils/toast'
import { triggerEasterEgg } from '../utils/dev'
import { convertFileSrc } from '@tauri-apps/api/core'
import AppSelect from './AppSelect.vue'

const { t } = useI18n()

const emit = defineEmits<{
  (e: 'theme-change', theme: string): void
  (e: 'dark-mode-change', enabled: boolean): void
  (e: 'sidebar-change'): void
  (e: 'blur-change', blur: number): void
}>()

const props = defineProps<{
  darkMode?: boolean
}>()

const javaPath = ref('')
const gameDir = ref('')
const autoUpdate = ref(true)
const SIDEBAR_KEYS = ['home', 'version', 'download', 'terracotta', 'music', 'ai', 'pluginCenter', 'settings'] as const
const sidebarVisible = ref<Record<string, boolean>>({})
const javaArgs = ref('')
const selectedTheme = ref('#ffffff')
const customBackground = ref<string | null>(null)
const customBgPreviewUrl = computed(() => {
  const bg = customBackground.value
  if (!bg || bg.startsWith('#')) return ''
  if (bg.startsWith('data:')) return bg
  return convertFileSrc(bg)
})
const bgBlur = ref(Number(localStorage.getItem('lumia-bg-blur')) || 0)

// 工具箱 - 下载器
const dlUrl = ref('')
const dlSavePath = ref('')
const dlProgress = ref(0)
const dlStage = ref('')
const dlDownloading = ref(false)
const dlTotal = ref(0)
const dlDownloaded = ref(0)
const dlSuccess = ref(false)
const showSponsor = ref(false)
let dlUnlisten: (() => void) | null = null
const availableJavaPaths = ref<string[]>([])

function extractFilenameFromUrl(rawUrl: string): string {
  try {
    let url = rawUrl.trim()
    // 去掉 query 参数和 hash
    const qIdx = url.indexOf('?')
    const hIdx = url.indexOf('#')
    let end = url.length
    if (qIdx !== -1) end = Math.min(end, qIdx)
    if (hIdx !== -1) end = Math.min(end, hIdx)
    url = url.substring(0, end)
    // 取最后一个 / 之后的部分
    const name = url.split('/').pop() || 'download'
    // URL 解码（%20 → 空格，%E4%B8%AD → 中文）
    return decodeURIComponent(name)
  } catch {
    return 'download'
  }
}
/** 语言偏好：auto（跟随系统）/ zh-CN / en / ja */
const language = ref('auto')

const languageOptions = computed(() => [
  { value: 'auto', label: t('settings.languageAuto') },
  { value: 'zh-CN', label: '简体中文' },
  { value: 'zh-TW', label: '繁體中文（台灣）' },
  { value: 'zh-HK', label: '繁體中文（香港）' },
  { value: 'en', label: 'English' },
  { value: 'ja', label: '日本語' },
  { value: 'ko', label: '한국어' },
  { value: 'fr', label: 'Français' },
])
/** AI 提供商预设：baseUrl 为 OpenAI 兼容 API 地址（不含 /chat/completions 后缀），model 为默认模型 */
interface AiProviderPreset {
  value: string
  label: string
  baseUrl: string
  model: string
}

const AI_PROVIDERS: AiProviderPreset[] = [
  { value: 'deepseek', label: 'DeepSeek', baseUrl: 'https://api.deepseek.com', model: 'deepseek-chat' },
  { value: 'siliconflow', label: '硅基流动 SiliconFlow', baseUrl: 'https://api.siliconflow.cn/v1', model: 'deepseek-ai/DeepSeek-V3' },
  { value: 'openai', label: 'OpenAI', baseUrl: 'https://api.openai.com/v1', model: 'gpt-4o-mini' },
  { value: 'anthropic', label: 'Anthropic', baseUrl: 'https://api.anthropic.com', model: 'claude-sonnet-4-5' },
  { value: 'volcano', label: '火山方舟 Volcengine Ark', baseUrl: 'https://ark.cn-beijing.volces.com/api/v3', model: 'doubao-seed-2-1-pro-260628' },
  { value: 'zai', label: 'z.ai / 智谱 GLM', baseUrl: 'https://api.z.ai/api/paas/v4', model: 'glm-4.7' },
  { value: 'qwen', label: '通义千问 Qwen', baseUrl: 'https://dashscope.aliyuncs.com/compatible-mode/v1', model: 'qwen-plus' },
  { value: 'moonshot', label: '月之暗面 Kimi', baseUrl: 'https://api.moonshot.cn/v1', model: 'moonshot-v1-8k' },
]

/** AI 助手配置 */
const aiBaseUrl = ref('')
const aiApiKey = ref('')
const aiModel = ref('')
/** 当前选择的提供商标识（自定义时用户可自由编辑地址/模型） */
const aiProvider = ref('deepseek')
/** 从提供商拉取到的可用模型列表（供点击回填） */
const aiModels = ref<string[]>([])
const aiModelsLoading = ref(false)
const appVersion = ref('')
const devClickCount = ref(0)
let devClickTimer: ReturnType<typeof setTimeout> | null = null

const themes = [
  '#ffffff', '#e94560', '#ff6b6b', '#feca57', '#48dbfb', 
  '#1dd1a1', '#5f27cd', '#ff9ff3', '#54a0ff', '#00d2d3', '#ff9f43'
]

async function loadConfig() {
  try {
    const config = await getConfig()
    javaPath.value = config.java_path || '/usr/bin/java'
    gameDir.value = config.game_dir || ''
    language.value = (config.language as string) || 'auto'
    aiBaseUrl.value = (config.ai_base_url as string) || ''
    aiApiKey.value = (config.ai_api_key as string) || ''
    aiModel.value = (config.ai_model as string) || ''
    aiProvider.value = (config.ai_provider as string) || matchAiProvider(aiBaseUrl.value)
    autoUpdate.value = config.auto_update !== false
    javaArgs.value = (config.java_args as string) || ''
    sidebarVisible.value = (config.sidebar_visible as Record<string, boolean>) || {}
    // 恢复自定义背景/主题色
    const tc = (config.theme_color as string) || ''
    if (tc && !tc.startsWith('#')) {
      customBackground.value = tc
    } else if (tc) {
      selectedTheme.value = tc
    }
  } catch (err) {
    console.error('加载配置失败:', err)
  }
}

/** 提供商下拉选项（品牌名保持原样，自定义项走 i18n） */
const aiProviderOptions = computed(() => [
  ...AI_PROVIDERS.map((p) => ({ value: p.value, label: p.label })),
  { value: 'custom', label: t('ai.customProvider') },
])

function normalizeUrl(u: string): string {
  return u.trim().replace(/\/+$/, '')
}

/** 根据已保存的 API 地址反推提供商（无法匹配则视为自定义） */
function matchAiProvider(url: string): string {
  const norm = normalizeUrl(url)
  return AI_PROVIDERS.find((p) => normalizeUrl(p.baseUrl) === norm)?.value ?? 'custom'
}

/** 选择提供商时自动填入对应 API 地址与默认模型（自定义则保留当前内容供手工编辑） */
function selectProvider(value: string) {
  aiModels.value = []
  const preset = AI_PROVIDERS.find((p) => p.value === value)
  if (preset) {
    aiBaseUrl.value = preset.baseUrl
    aiModel.value = preset.model
  }
}

/** 询问当前提供商，拉取可用模型列表（结果以可点击芯片展示，不强制覆盖已填模型） */
async function fetchAiModels() {
  if (aiModelsLoading.value) return
  aiModelsLoading.value = true
  aiModels.value = []
  try {
    const res = await listAiModels(aiBaseUrl.value.trim(), aiApiKey.value.trim(), aiProvider.value)
    aiModels.value = res.models || []
    if (aiModels.value.length === 0) {
      toast(t('ai.modelsEmpty'), 'info')
    }
  } catch (err) {
    toast(t('ai.fetchFailed', { msg: String(err) }), 'error')
  } finally {
    aiModelsLoading.value = false
  }
}

async function loadJavaPaths() {
  try {
    availableJavaPaths.value = await getJavaPaths()
  } catch (err) {
    console.error('扫描 Java 失败:', err)
  }
}

async function saveJavaPath() {
  try {
    const config = await getConfig()
    config.java_path = javaPath.value
    await saveConfig(config)
    toast(t('settings.javaPathSaved'), 'success')
  } catch (err) {
    toast(t('settings.saveFailed', { msg: String(err) }), 'error')
  }
}

async function saveGameDir() {
  try {
    const config = await getConfig()
    config.game_dir = gameDir.value.trim() ? gameDir.value.trim() : null
    await saveConfig(config)
    gameDir.value = config.game_dir || ''
    toast(t('settings.gameDirSaved'), 'success')
  } catch (err) {
    toast(t('settings.saveFailed', { msg: String(err) }), 'error')
  }
}

/** 保存「自动检查更新」开关（静默失败，不打扰用户） */
async function saveAutoUpdate() {
  try {
    const config = await getConfig()
    config.auto_update = autoUpdate.value
    await saveConfig(config)
  } catch {
    // 静默失败
  }
}

async function saveSidebarVisibility() {
  try {
    const config = await getConfig()
    config.sidebar_visible = sidebarVisible.value
    await saveConfig(config)
    emit('sidebar-change')
  } catch {
    // 静默失败
  }
}

/** 保存自定义 Java 启动参数 */
async function saveJavaArgs() {
  try {
    const config = await getConfig()
    config.java_args = javaArgs.value.trim()
    await saveConfig(config)
    toast(t('settings.javaArgsSaved'), 'success')
  } catch (err) {
    toast(t('settings.saveFailed', { msg: String(err) }), 'error')
  }
}

/** 保存语言偏好并立即生效（auto 时按系统语言解析） */
async function saveLanguage() {
  try {
    const config = await getConfig()
    config.language = language.value
    await saveConfig(config)
    const sys = navigator.language || navigator.languages?.[0] || 'en'
    applyLocale(resolveLocale(language.value, sys))
  } catch (err) {
    toast(t('settings.saveFailed', { msg: String(err) }), 'error')
  }
}

/** 保存 AI 助手配置 */
async function saveAiConfig() {
  try {
    const config = await getConfig()
    config.ai_base_url = aiBaseUrl.value.trim()
    config.ai_api_key = aiApiKey.value.trim()
    config.ai_model = aiModel.value.trim()
    config.ai_provider = aiProvider.value
    await saveConfig(config)
    toast(t('ai.saved'), 'success')
  } catch (err) {
    toast(t('settings.saveFailed', { msg: String(err) }), 'error')
  }
}

const handleThemeSelect = async (color: string) => {
  if (customBackground.value) {
    try {
      await removeBackgroundImage(customBackground.value)
    } catch {}
  }
  selectedTheme.value = color
  customBackground.value = null
  emit('theme-change', color)
}

const handlePhotoUpload = (event: Event) => {
  const target = event.target as HTMLInputElement
  const file = target.files?.[0]
  if (file) {
    const reader = new FileReader()
    reader.onload = async (e) => {
      const dataUrl = e.target?.result as string
      const base64 = dataUrl.split(',')[1]
      if (!base64) {
        toast(t('settings.saveFailed', { msg: '图片数据无效' }), 'error')
        return
      }
      try {
        const savedPath = await saveBackgroundImage(base64, file.name)
        customBackground.value = savedPath
        emit('theme-change', savedPath)
      } catch (err) {
        toast(t('settings.saveFailed', { msg: String(err) }), 'error')
      }
    }
    reader.readAsDataURL(file)
  }
}

const removeCustomBackground = async () => {
  if (customBackground.value) {
    try {
      await removeBackgroundImage(customBackground.value)
    } catch {}
  }
  customBackground.value = null
  selectedTheme.value = '#ffffff'
  emit('theme-change', '#ffffff')
}

onMounted(() => {
  loadConfig()
  loadJavaPaths()
  loadVersion()
})

// 下载器方法
async function browseSavePath() {
  try {
    const { open } = await import('@tauri-apps/plugin-dialog')
    const folder = await open({
      directory: true,
      title: t('settings.downloaderSavePath'),
    })
    if (folder) {
      const filename = extractFilenameFromUrl(dlUrl.value) || 'download'
      dlSavePath.value = folder + (folder.endsWith('/') || folder.endsWith('\\') ? '' : '/') + filename
    }
  } catch {}
}

async function startFileDownload() {
  if (!dlUrl.value.trim()) {
    toast(t('settings.downloaderUrl'), 'info')
    return
  }
  if (!dlSavePath.value.trim()) {
    toast(t('settings.downloaderSavePath'), 'info')
    return
  }

  // 自动清理文件名中的 query 参数（如 ?x-oss-process=...）
  const cleanPath = dlSavePath.value.replace(/\?.*$/, '')
  if (cleanPath !== dlSavePath.value) {
    dlSavePath.value = cleanPath
  }

  dlDownloading.value = true
  dlProgress.value = 0
  dlStage.value = ''
  dlTotal.value = 0
  dlDownloaded.value = 0
  dlSuccess.value = false

  dlUnlisten = await listenDownloadFileProgress((payload: any) => {
    if (payload.stage) {
      dlStage.value = payload.stage
    }
    if (payload.progress !== undefined) {
      dlProgress.value = payload.progress
    }
    if (payload.total) {
      dlTotal.value = payload.total
    }
    if (payload.downloaded) {
      dlDownloaded.value += payload.downloaded
      if (dlTotal.value > 0) {
        dlProgress.value = Math.round((dlDownloaded.value / dlTotal.value) * 100)
      }
    }
  })

  try {
    await downloadFile(dlUrl.value.trim(), dlSavePath.value.trim())
    dlProgress.value = 100
    dlStage.value = ''
    dlSuccess.value = true
    toast(t('settings.downloaderComplete', { path: dlSavePath.value }), 'success')
  } catch (err) {
    toast(t('settings.downloaderFailed'), 'error')
  } finally {
    dlDownloading.value = false
    if (dlUnlisten) {
      dlUnlisten()
      dlUnlisten = null
    }
  }
}

async function openDownloadFolder() {
  if (!dlSavePath.value) return
  try {
    const { openPath } = await import('@tauri-apps/plugin-opener')
    const { dirname } = await import('@tauri-apps/api/path')
    const folder = await dirname(dlSavePath.value)
    await openPath(folder || dlSavePath.value)
  } catch {}
}

async function loadVersion() {
  try {
    appVersion.value = formatAppVersion(await getAppVersion())
  } catch {
    appVersion.value = 'v1.0 beta 1'
  }
}

/** 语义化版本号 (1.0.0-beta.1) → 展示文本 (v1.0 beta 1) */
function formatAppVersion(v: string): string {
  const m = v.match(/^(\d+)\.(\d+)\.(\d+)(?:-([a-zA-Z]+)\.?(\d+))?$/)
  if (!m) return v
  const [, major, minor, , tag, tagNum] = m
  const base = `v${major}.${minor}`
  if (tag) return `${base} ${tag} ${tagNum}`
  return base
}

function onBgBlurChange(e: Event) {
  const val = Number((e.target as HTMLInputElement).value)
  bgBlur.value = val
  localStorage.setItem('lumia-bg-blur', String(val))
  emit('blur-change', val)
}
const DEV_CLICKS_NEEDED = 10

function handleVersionClick(e: MouseEvent) {
  if (!e.shiftKey) {
    devClickCount.value = 0
    return
  }
  devClickCount.value++
  if (devClickTimer) clearTimeout(devClickTimer)
  if (devClickCount.value >= DEV_CLICKS_NEEDED) {
    devClickCount.value = 0
    triggerEasterEgg()
    toast('Never Gonna Give You Up!')
  }
  devClickTimer = setTimeout(() => {
    devClickCount.value = 0
  }, 3000)
}
</script>

<template>
  <div class="settings-content">
    <div class="settings-section">
      <div class="section-title">{{ t('settings.theme') }}</div>

      <div class="setting-card">
        <div class="setting-label">{{ t('settings.chooseColor') }}</div>
        <div class="color-grid">
          <button
            v-for="color in themes"
            :key="color"
            :class="['color-btn', { active: selectedTheme === color && !customBackground }]"
            @click="handleThemeSelect(color)"
            :style="{ background: color }"
          ></button>
        </div>
      </div>

      <div class="setting-card">
        <label class="toggle-switch full">
          <input
            type="checkbox"
            :checked="!!props.darkMode"
            @change="(e) => emit('dark-mode-change', (e.target as HTMLInputElement).checked)"
          />
          <span class="slider"></span>
          <span class="toggle-label">{{ t('settings.darkMode') }}</span>
        </label>
      </div>

      <div class="setting-card">
        <div class="setting-label">{{ t('settings.customBg') }}</div>
        <div class="custom-bg-area">
          <div v-if="customBackground" class="bg-preview">
            <img :src="customBgPreviewUrl" :alt="t('settings.customBg')" class="bg-image" />
            <button class="remove-bg-btn" @click="removeCustomBackground">{{ t('settings.remove') }}</button>
          </div>
          <label v-else class="upload-btn">
            <input type="file" accept="image/*" @change="handlePhotoUpload" class="file-input" />
            <span class="upload-icon">+</span>
            <span class="upload-text">{{ t('settings.uploadPhoto') }}</span>
          </label>
        </div>
        <div v-if="customBackground" class="blur-slider-row">
          <span class="setting-label">{{ t('settings.bgBlur') }}</span>
          <input
            type="range"
            min="0"
            max="20"
            :value="bgBlur"
            @input="onBgBlurChange"
            class="blur-range"
          />
          <span class="blur-value">{{ bgBlur }}px</span>
        </div>
      </div>
    </div>

    <div class="settings-section">
      <div class="section-title">{{ t('settings.game') }}</div>

      <div class="setting-card">
        <div class="setting-label">{{ t('settings.javaPath') }}</div>
        <div class="setting-row">
          <input type="text" v-model="javaPath" class="setting-input" />
          <button class="browse-btn" @click="saveJavaPath">{{ t('common.save') }}</button>
        </div>
        <div v-if="availableJavaPaths.length" class="java-suggestions">
          <span class="suggestion-label">{{ t('settings.detectedJava') }}</span>
          <button
            v-for="path in availableJavaPaths"
            :key="path"
            class="suggestion-btn"
            @click="javaPath = path; saveJavaPath()"
          >
            {{ path }}
          </button>
        </div>
      </div>

      <div class="setting-card">
        <div class="setting-label">{{ t('settings.gameDir') }}</div>
        <div class="setting-row">
          <input type="text" v-model="gameDir" class="setting-input" :placeholder="t('settings.gameDirPlaceholder')" />
          <button class="browse-btn" @click="saveGameDir">{{ t('common.save') }}</button>
        </div>
        <div class="setting-hint">{{ t('settings.gameDirHint') }}</div>
      </div>

    </div>

    <div class="settings-section">
      <div class="section-title">{{ t('settings.other') }}</div>

      <div class="setting-card">
        <div class="setting-label">{{ t('settings.language') }}</div>
        <div class="setting-row">
          <AppSelect v-model="language" :options="languageOptions" @change="saveLanguage" />
        </div>
      </div>

      <div class="setting-card">
        <label class="toggle-switch full">
          <input type="checkbox" v-model="autoUpdate" @change="saveAutoUpdate" />
          <span class="slider"></span>
          <span class="toggle-label">{{ t('settings.autoUpdate') }}</span>
        </label>
      </div>
    </div>

    <div class="settings-section">
      <div class="section-title">{{ t('settings.sidebar') }}</div>

      <div class="setting-card sidebar-toggle-grid">
        <label
          v-for="key in SIDEBAR_KEYS"
          :key="key"
          class="toggle-switch"
          :class="{ locked: key === 'settings' }"
        >
          <input
            type="checkbox"
            :disabled="key === 'settings'"
            :checked="key === 'settings' ? true : sidebarVisible[key] !== false"
            @change="sidebarVisible[key] = ($event.target as HTMLInputElement).checked; saveSidebarVisibility()"
          />
          <span class="slider"></span>
          <span class="toggle-label">{{ t(`settings.sidebarItems.${key}`) }}</span>
        </label>
      </div>
    </div>

    <div class="settings-section">
      <div class="section-title">{{ t('settings.advanced') }}</div>

      <div class="setting-card">
        <div class="setting-label">{{ t('settings.javaArgs') }}</div>
        <div class="setting-row">
          <textarea v-model="javaArgs" class="setting-input java-args-input" rows="3"
            :placeholder="t('settings.javaArgsPlaceholder')"></textarea>
        </div>
        <div class="setting-hint">{{ t('settings.javaArgsHint') }}</div>
        <div class="setting-row" style="margin-top: 10px; justify-content: flex-end">
          <button class="browse-btn" @click="saveJavaArgs">{{ t('common.save') }}</button>
        </div>
      </div>
    </div>

    <div class="settings-section">
      <div class="section-title">{{ t('ai.settings') }}</div>

      <div class="setting-card">
        <div class="setting-label">{{ t('ai.provider') }}</div>
        <div class="setting-row">
          <AppSelect v-model="aiProvider" :options="aiProviderOptions" @change="selectProvider" />
        </div>
        <div class="setting-label" style="margin-top: 8px">{{ t('ai.baseUrl') }}</div>
        <div class="setting-row">
          <input type="text" v-model="aiBaseUrl" class="setting-input" placeholder="https://api.deepseek.com" />
        </div>
        <div class="setting-label" style="margin-top: 8px">{{ t('ai.apiKey') }}</div>
        <div class="setting-row">
          <input type="password" v-model="aiApiKey" class="setting-input" :placeholder="t('ai.apiKey')" />
        </div>
        <div class="setting-label" style="margin-top: 8px">{{ t('ai.model') }}</div>
        <div class="setting-row">
          <input type="text" v-model="aiModel" class="setting-input" placeholder="deepseek-chat" />
          <button class="browse-btn" :disabled="aiModelsLoading" @click="fetchAiModels">
            {{ aiModelsLoading ? t('ai.fetching') : t('ai.fetchModels') }}
          </button>
        </div>
        <div v-if="aiModels.length" class="java-suggestions">
          <span class="suggestion-label">{{ t('ai.availableModels') }}</span>
          <button
            v-for="m in aiModels"
            :key="m"
            class="suggestion-btn"
            :class="{ active: aiModel === m }"
            @click="aiModel = m"
          >
            {{ m }}
          </button>
        </div>
        <div class="setting-hint">{{ t('ai.settingsHint') }}</div>
        <div class="setting-row" style="margin-top: 10px; justify-content: flex-end">
          <button class="browse-btn" @click="saveAiConfig">{{ t('common.save') }}</button>
        </div>
      </div>
    </div>

    <div class="settings-section">
      <div class="section-title">{{ t('settings.toolbox') }}</div>

      <div class="setting-card">
        <div class="setting-label">{{ t('settings.downloader') }}</div>
        <div class="setting-label" style="margin-top: 8px">{{ t('settings.downloaderUrl') }}</div>
        <div class="setting-row">
          <input type="text" v-model="dlUrl" class="setting-input" :placeholder="t('settings.downloaderUrlPlaceholder')" :disabled="dlDownloading" />
        </div>
        <div class="setting-label" style="margin-top: 8px">{{ t('settings.downloaderSavePath') }}</div>
        <div class="setting-row">
          <input type="text" v-model="dlSavePath" class="setting-input" :placeholder="t('settings.downloaderSavePathPlaceholder')" :disabled="dlDownloading" />
          <button class="browse-btn" @click="browseSavePath" :disabled="dlDownloading">{{ t('settings.downloaderBrowse') }}</button>
        </div>
        <div class="setting-row" style="margin-top: 10px; justify-content: space-between; align-items: center">
          <button class="browse-btn" @click="startFileDownload" :disabled="dlDownloading">
            {{ dlDownloading ? t('settings.downloaderDownloading') : t('settings.downloaderStart') }}
          </button>
          <span v-if="dlDownloading && dlStage" class="dl-status">{{ dlStage }}</span>
        </div>
        <div v-if="dlDownloading && dlTotal > 0" class="dl-progress-bar" style="margin-top: 8px">
          <div class="dl-progress-fill" :style="{ width: dlProgress + '%' }"></div>
          <span class="dl-progress-text">{{ dlProgress }}%</span>
        </div>
        <div v-if="dlSuccess" class="setting-row" style="margin-top: 8px">
          <span class="dl-success-path">{{ dlSavePath }}</span>
          <button class="browse-btn" @click="openDownloadFolder">{{ t('settings.downloaderOpenFolder') }}</button>
        </div>
      </div>
    </div>

    <div class="settings-section">
      <div class="section-title">{{ t('settings.about') }}</div>

      <div class="setting-card">
        <div class="about-info">
          <span class="about-label">{{ t('settings.aboutVersion') }}</span>
          <span class="about-value" @click="handleVersionClick">{{ appVersion }}</span>
        </div>
        <div class="about-info">
          <span class="about-label">{{ t('settings.aboutAuthor') }}</span>
          <span class="about-value">小甜瓜</span>
        </div>
        <div class="about-info">
          <span class="about-label">{{ t('settings.aboutLicense') }}</span>
          <span class="about-value">MIT License</span>
        </div>
      </div>
    </div>

    <div class="settings-section">
      <div class="setting-card" style="text-align: center; padding: 16px">
        <button class="sponsor-btn" @click="showSponsor = true">{{ t('settings.sponsor') }}</button>
      </div>
    </div>

    <div v-if="showSponsor" class="sponsor-overlay" @click.self="showSponsor = false">
      <div class="sponsor-modal">
        <button class="sponsor-close" @click="showSponsor = false">✕</button>
        <h3 class="sponsor-title">{{ t('settings.sponsorTitle') }}</h3>
        <div class="sponsor-images">
          <div class="sponsor-item">
            <img src="@/assets/icons/WechatPay.JPG" alt="WeChat Pay" />
            <span>微信支付</span>
          </div>
          <div class="sponsor-item">
            <img src="@/assets/icons/Alipay.JPG" alt="Alipay" />
            <span>支付宝</span>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.settings-content {
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
  padding: 10px 17px;
  box-sizing: border-box;
  overflow-y: auto;
}
.settings-section {
  padding: 16px 0;
}
.section-title {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 14px;
  color: var(--label-color);
  margin-bottom: 12px;
  padding-left: 8px;
}
.setting-card {
  background: var(--card-bg);
  border-radius: 10px;
  padding: 12px 16px;
  margin-bottom: 8px;
}
.setting-label {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 14px;
  color: var(--label-color);
  margin-bottom: 8px;
}
.setting-row {
  display: flex;
  align-items: center;
  gap: 12px;
}
.setting-hint {
  margin-top: 8px;
  color: var(--label-color);
  font-size: 12px;
}
.setting-input {
  flex: 1;
  padding: 8px 12px;
  border: none;
  border-radius: 8px;
  background: var(--input-bg);
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 14px;
  color: var(--text-color);
  outline: none;
  box-sizing: border-box;
}
.java-args-input {
  min-height: 76px;
  resize: vertical;
  font-family: 'SF Mono', Menlo, Consolas, 'Courier New', monospace;
  line-height: 1.5;
  white-space: pre;
}
.browse-btn {
  padding: 8px 16px;
  border: none;
  border-radius: 8px;
  background: var(--button-bg);
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 14px;
  color: var(--text-color);
  cursor: pointer;
  transition: background-color 0.2s, transform 100ms ease;
}
.browse-btn:active {
  transform: scale(0.97);
  background-color: var(--button-active);
}
.color-grid {
  display: grid;
  grid-template-columns: repeat(5, 1fr);
  gap: 8px;
}
.color-btn {
  width: 40px;
  height: 40px;
  border: 2px solid rgba(0, 0, 0, 0.1);
  border-radius: 8px;
  cursor: pointer;
  transition: transform 0.2s, border-color 0.2s;
  justify-self: center;
}
.color-btn:hover {
  transform: scale(1.1);
}
.color-btn.active {
  border-color: var(--text-color);
}
.custom-bg-area {
  display: flex;
  flex-direction: column;
  align-items: center;
}
.bg-preview {
  position: relative;
  width: 100%;
  height: 100px;
  border-radius: 8px;
  overflow: hidden;
}
.bg-image {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.remove-bg-btn {
  position: absolute;
  top: 8px;
  right: 8px;
  padding: 4px 8px;
  border: none;
  border-radius: 4px;
  background: rgba(0, 0, 0, 0.5);
  color: #ffffff;
  font-size: 12px;
  cursor: pointer;
  transition: background-color 0.2s;
}
.remove-bg-btn:hover {
  background: rgba(0, 0, 0, 0.7);
}
.blur-slider-row {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-top: 10px;
}
.blur-slider-row .setting-label {
  flex-shrink: 0;
}
.blur-range {
  flex: 1;
  height: 4px;
  -webkit-appearance: none;
  appearance: none;
  background: var(--button-bg);
  border-radius: 2px;
  outline: none;
  cursor: pointer;
}
.blur-range::-webkit-slider-thumb {
  -webkit-appearance: none;
  appearance: none;
  width: 16px;
  height: 16px;
  border-radius: 50%;
  background: var(--accent-color);
  cursor: pointer;
}
.blur-value {
  font-size: 12px;
  color: var(--label-color);
  min-width: 32px;
  text-align: right;
}
.upload-btn {
  width: 100%;
  height: 100px;
  border: 2px dashed var(--border-color);
  border-radius: 8px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  cursor: pointer;
  transition: border-color 0.2s, background-color 0.2s;
}
.upload-btn:hover {
  border-color: var(--accent-color);
  background: rgba(0, 0, 0, 0.05);
}
.upload-icon {
  font-size: 24px;
  color: var(--label-color);
}
.upload-text {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 14px;
  color: var(--label-color);
}
.file-input {
  display: none;
}
.toggle-switch {
  display: flex;
  align-items: center;
  gap: 8px;
  cursor: pointer;
}
.toggle-switch.full {
  width: 100%;
}
.toggle-switch input {
  display: none;
}
.toggle-switch .slider {
  width: 48px;
  height: 24px;
  background: rgba(0, 0, 0, 0.2);
  border-radius: 12px;
  position: relative;
  transition: background-color 0.3s ease;
}
.toggle-switch input:checked + .slider {
  background: #4ade80;
}
.toggle-switch .slider::before {
  content: '';
  position: absolute;
  top: 2px;
  left: 2px;
  width: 20px;
  height: 20px;
  background: #ffffff;
  border-radius: 50%;
  transition: transform 0.3s cubic-bezier(0.34, 1.2, 0.64, 1);
}
.toggle-switch input:checked + .slider::before {
  transform: translateX(24px);
}
.toggle-label {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 14px;
  color: var(--text-color);
}
.toggle-switch.full .toggle-label {
  flex: 1;
  text-align: left;
}
.sidebar-toggle-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 12px 20px;
}
.sidebar-toggle-grid .toggle-switch.locked {
  opacity: 0.5;
  cursor: not-allowed;
}
.sidebar-toggle-grid .toggle-switch.locked input {
  pointer-events: none;
}
.about-info {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 8px 0;
}
.about-label {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 14px;
  color: var(--label-color);
}
.about-value {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 14px;
  color: var(--text-color);
}
.java-suggestions {
  margin-top: 8px;
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}
.suggestion-label {
  font-size: 12px;
  color: var(--label-color);
  margin-right: 4px;
}
.suggestion-btn {
  padding: 2px 8px;
  border: 1px solid var(--border-color);
  border-radius: 4px;
  background: transparent;
  color: var(--text-color);
  font-size: 11px;
  cursor: pointer;
  transition: background 0.2s;
}
.suggestion-btn:hover {
  background: var(--card-bg);
}
.suggestion-btn.active {
  border-color: var(--accent-color);
  color: var(--accent-color);
}
@media (prefers-reduced-motion: reduce) {
  .color-btn:hover, .color-btn { transform: none; }
  .toggle-switch .slider { transition: background-color 0.15s linear; }
  .toggle-switch .slider::before { transition: transform 0.15s linear; }
  .browse-btn:active { transform: none; }
}

.dl-status {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 12px;
  color: var(--label-color);
}
.dl-progress-bar {
  position: relative;
  width: 100%;
  height: 18px;
  background: var(--card-bg);
  border-radius: 9px;
  overflow: hidden;
}
.dl-progress-fill {
  height: 100%;
  background: var(--accent-color);
  border-radius: 9px;
  transition: width 0.3s ease;
}
.dl-progress-text {
  position: absolute;
  top: 50%;
  left: 50%;
  transform: translate(-50%, -50%);
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 11px;
  color: #fff;
  text-shadow: 0 1px 2px rgba(0,0,0,0.3);
}
.dl-success-path {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 12px;
  color: var(--label-color);
  word-break: break-all;
  flex: 1;
  margin-right: 8px;
}

.sponsor-btn {
  padding: 10px 24px;
  border: none;
  border-radius: 10px;
  background: var(--button-bg);
  font-size: 15px;
  font-weight: 600;
  color: var(--text-color);
  cursor: pointer;
  transition: opacity 0.2s;
}

.sponsor-overlay {
  position: fixed;
  top: 0;
  left: 0;
  width: 100%;
  height: 100%;
  background: rgba(0, 0, 0, 0.5);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 9999;
}
.sponsor-modal {
  background: var(--popup-bg);
  border-radius: 12px;
  padding: 28px;
  max-width: 480px;
  width: 90%;
  position: relative;
  box-shadow: var(--popup-shadow);
}
.sponsor-close {
  position: absolute;
  top: 12px;
  right: 16px;
  border: none;
  background: none;
  font-size: 20px;
  cursor: pointer;
  color: var(--label-color);
}
.sponsor-title {
  text-align: center;
  font-size: 18px;
  font-weight: 600;
  margin: 0 0 20px 0;
  color: var(--text-color);
}
.sponsor-images {
  display: flex;
  gap: 20px;
  justify-content: center;
}
.sponsor-item {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
}
.sponsor-item img {
  width: 180px;
  height: 180px;
  border-radius: 12px;
  object-fit: contain;
  background: #fff;
  padding: 8px;
}
.sponsor-item span {
  font-size: 13px;
  color: var(--label-color);
}
</style>