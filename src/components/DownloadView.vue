<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { getVersions, getLocalVersions, getIconUrl } from '../utils/tauri'
import { cleanSvg } from '../utils/svg'
import ModView from './ModView.vue'
import ResourcePackView from './ResourcePackView.vue'
import ShaderPackView from './ShaderPackView.vue'
import AppSelect from './AppSelect.vue'
import { useI18n } from 'vue-i18n'
import minecraftRaw from '../assets/icons/minecraft.svg?raw'
import modRaw from '../assets/icons/mod.svg?raw'
import resourcepacksRaw from '../assets/icons/resourcepacks.svg?raw'
import shaderpackRaw from '../assets/icons/shaderpack.svg?raw'

const { t, locale } = useI18n()

const minecraftSvg = cleanSvg(minecraftRaw)
const modSvg = cleanSvg(modRaw)
const resourcepacksSvg = cleanSvg(resourcepacksRaw)
const shaderpackSvg = cleanSvg(shaderpackRaw)

const emit = defineEmits<{
  (event: 'download-request', version: string): void
}>()

// 下载页分区：游戏本体 / 模组 / 资源包 / 光影包
const activeSection = ref<'game' | 'mod' | 'resourcePack' | 'shaderPack'>('game')
const activeIndex = computed(() =>
  activeSection.value === 'game' ? 0 : activeSection.value === 'mod' ? 1 : activeSection.value === 'resourcePack' ? 2 : 3,
)

/** 版本类型使用稳定 id（release/snapshot/...），显示文案由 i18n 提供 */
type VersionType = 'release' | 'snapshot' | 'april_fool' | 'ancient' | 'unknown'

interface VersionItem {
  id: string
  version: string
  type: VersionType
  releaseTime: string
  url: string
  requiredJava: number
  isInstalled: boolean
  iconUrl: string
}

const versions = ref<VersionItem[]>([])
const searchQuery = ref('')
const filterType = ref('')
const loading = ref(false)
const loadError = ref('')

const filterOptions = computed(() => [
  { value: '', label: t('download.filterAll') },
  { value: 'release', label: t('download.typeRelease') },
  { value: 'snapshot', label: t('download.typeSnapshot') },
  { value: 'april_fool', label: t('download.typeAprilFool') },
  { value: 'ancient', label: t('download.typeAncient') },
])

const typeLabel = (type: VersionType): string => {
  switch (type) {
    case 'release': return t('download.typeRelease')
    case 'snapshot': return t('download.typeSnapshot')
    case 'april_fool': return t('download.typeAprilFool')
    case 'ancient': return t('download.typeAncient')
    default: return t('download.typeUnknown')
  }
}

const filteredVersions = computed(() => {
  let result = versions.value
  if (filterType.value !== '') {
    result = result.filter(v => v.type === filterType.value)
  }
  if (searchQuery.value.trim()) {
    const q = searchQuery.value.trim().toLowerCase()
    result = result.filter(v => 
      v.version.toLowerCase().includes(q) ||
      typeLabel(v.type).toLowerCase().includes(q)
    )
  }
  return result
})

const mapCategoryToType = (category: string): VersionType => {
  switch (category) {
    case 'release': return 'release'
    case 'snapshot': return 'snapshot'
    case 'april_fool': return 'april_fool'
    case 'ancient': return 'ancient'
    default: return 'unknown'
  }
}

const JAVA_VERSION_MAP: [RegExp, number][] = [
  [/^1\.(14|15|16)\./, 8],
  [/^1\.17/, 16],
  [/^1\.(18|19|20)\./, 17],
  [/^1\.(21|22)/, 21],
  [/^26/, 25],
]

const getRequiredJavaVersion = (version: string): number =>
  JAVA_VERSION_MAP.find(([re]) => re.test(version))?.[1] ?? 17

function formatReleaseTime(releaseTime: string): string {
  if (!releaseTime) return '--'
  const date = new Date(releaseTime)
  if (Number.isNaN(date.getTime())) return '--'
  const dateLocale = locale.value === 'zh-CN' ? 'zh-CN' : locale.value === 'ja' ? 'ja-JP' : locale.value === 'fr' ? 'fr-FR' : 'en-US'
  return date.toLocaleString(dateLocale, {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit'
  })
}

async function loadVersions() {
  loading.value = true
  loadError.value = ''
  try {
    const list = await getVersions()
    const localList = await getLocalVersions()
    const localNames = localList.map(v => v.name)
    const iconPromises = list.map(async (manifestItem) => {
      const type = mapCategoryToType(manifestItem.category)
      const iconName = type === 'ancient' ? 'Ancient' : type === 'april_fool' ? 'jokes' : type
      const iconUrl = await getIconUrl(`${iconName}.png`)
      return {
        id: manifestItem.id,
        version: manifestItem.id,
        type,
        releaseTime: formatReleaseTime(manifestItem.releaseTime),
        url: '',
        requiredJava: getRequiredJavaVersion(manifestItem.id),
        isInstalled: localNames.includes(manifestItem.id),
        iconUrl
      }
    })
    versions.value = await Promise.all(iconPromises)
  } catch (err: unknown) {
    console.error('加载版本失败:', err)
    loadError.value = err instanceof Error ? err.message : String(err)
  } finally {
    loading.value = false
  }
}

function handleDownload(item: VersionItem) {
  emit('download-request', item.version)
}

onMounted(() => {
  loadVersions()
})
</script>

<template>
  <div class="download-layout">
    <aside class="section-nav">
      <div class="section-indicator" :style="{ transform: `translateY(${activeIndex * 62 + 9}px)` }"></div>
      <button
        class="section-item"
        :class="{ active: activeSection === 'game' }"
        @click="activeSection = 'game'"
      >
        <span class="section-icon" v-html="minecraftSvg"></span>
        <span class="section-label">{{ t('download.sectionGame') }}</span>
      </button>
      <button
        class="section-item"
        :class="{ active: activeSection === 'mod' }"
        @click="activeSection = 'mod'"
      >
        <span class="section-icon" v-html="modSvg"></span>
        <span class="section-label">{{ t('download.sectionMod') }}</span>
      </button>
      <button
        class="section-item"
        :class="{ active: activeSection === 'resourcePack' }"
        @click="activeSection = 'resourcePack'"
      >
        <span class="section-icon" v-html="resourcepacksSvg"></span>
        <span class="section-label">{{ t('download.sectionResourcePack') }}</span>
      </button>
      <button
        class="section-item"
        :class="{ active: activeSection === 'shaderPack' }"
        @click="activeSection = 'shaderPack'"
      >
        <span class="section-icon" v-html="shaderpackSvg"></span>
        <span class="section-label">{{ t('download.sectionShaderPack') }}</span>
      </button>
    </aside>

    <div v-if="activeSection === 'game'" class="download-content">
      <div class="search-bar">
      <input type="text" v-model="searchQuery" :placeholder="t('download.searchPlaceholder')" class="search-input" />
      <AppSelect v-model="filterType" :options="filterOptions" />
    </div>

    <div class="table-header">
      <span class="header-item icon-header"></span>
      <span class="header-item version-header">{{ t('download.colVersion') }}</span>
      <span class="header-item type-header">{{ t('download.colType') }}</span>
      <span class="header-item java-header">{{ t('download.colJava') }}</span>
      <span class="header-item status-header">{{ t('download.colStatus') }}</span>
      <span class="header-item time-header">{{ t('download.colTime') }}</span>
      <span class="header-item action-header"></span>
    </div>

    <div class="download-list">
      <div v-if="loading" class="loading-state">
        <span class="loading-text">{{ t('download.loadingList') }}</span>
      </div>
      <div v-else-if="filteredVersions.length === 0" class="empty-state">
        <span class="empty-text">{{ loadError || t('download.noMatch') }}</span>
      </div>
      <div
        v-for="item in filteredVersions"
        :key="item.id"
        class="download-card"
        :class="{ installed: item.isInstalled }"
      >
        <img 
          :src="item.iconUrl" 
          :alt="typeLabel(item.type)"
          class="download-icon"
        />
        <span class="download-version">{{ item.version }}</span>
        <span class="download-type" :class="item.type">{{ typeLabel(item.type) }}</span>
        <span class="download-java">Java {{ item.requiredJava }}</span>
        <span class="download-status">
          <span v-if="item.isInstalled" class="status-badge installed">{{ t('common.installed') }}</span>
          <span v-else class="status-badge not-installed">{{ t('common.notInstalled') }}</span>
        </span>
        <span class="download-time">{{ item.releaseTime }}</span>
        <button 
          class="download-action-btn"
          :class="{ installed: item.isInstalled }"
          @click="handleDownload(item)"
          :title="item.isInstalled ? t('download.reDownload') : t('download.download')"
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/>
            <polyline points="7 10 12 15 17 10"/>
            <line x1="12" y1="15" x2="12" y2="3"/>
          </svg>
        </button>
      </div>
    </div>
    </div>

    <div v-else-if="activeSection === 'mod'" class="mod-section">
      <ModView />
    </div>
    <div v-else-if="activeSection === 'resourcePack'" class="mod-section">
      <ResourcePackView />
    </div>
    <div v-else class="mod-section">
      <ShaderPackView />
    </div>
  </div>
</template>

<style scoped>
.download-layout {
  width: 100%;
  height: 100%;
  display: flex;
  overflow: hidden;
}
.section-nav {
  width: 60px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  padding: 5px 0 0;
  border-right: 1px solid var(--border-color);
  box-sizing: border-box;
  position: relative;
}
.section-indicator {
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
.section-item {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  width: 50px;
  height: 58px;
  margin-bottom: 4px;
  padding: 0;
  border: none;
  border-radius: 10px;
  background: none;
  cursor: pointer;
  position: relative;
  z-index: 1;
  transition: transform 100ms ease;
}
.section-item:active {
  transform: scale(0.95);
}
.section-icon {
  width: 26px;
  height: 26px;
  display: block;
  color: var(--text-color);
}
.section-icon svg {
  width: 100%;
  height: 100%;
  display: block;
}
.section-label {
  font-family: Inter, 'PingFang SC', 'Microsoft YaHei', sans-serif;
  font-size: 10px;
  line-height: 13px;
  color: var(--text-color);
  opacity: 0.7;
  white-space: nowrap;
}
.section-item.active .section-label {
  opacity: 1;
  font-weight: 600;
}
.mod-section {
  flex: 1;
  min-width: 0;
  display: flex;
  padding: 10px 17px;
  box-sizing: border-box;
}
.download-content {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  padding: 10px 17px;
  box-sizing: border-box;
}
.search-bar {
  display: flex;
  gap: 10px;
  padding: 10px;
  background: var(--card-bg);
  border-radius: 10px;
  margin-bottom: 12px;
  flex-shrink: 0;
}
.search-input {
  flex: 1;
  padding: 8px 14px;
  border: none;
  border-radius: 8px;
  background: var(--button-bg);
  font-family: Inter, 'PingFang SC', 'Microsoft YaHei', sans-serif;
  font-size: 14px;
  color: var(--text-color);
  outline: none;
  box-sizing: border-box;
}
.search-input::placeholder {
  color: var(--label-color);
}
.filter-select {
  padding: 8px 32px 8px 14px;
  border: none;
  border-radius: 8px;
  background: var(--card-bg);
  font-family: Inter, 'PingFang SC', 'Microsoft YaHei', sans-serif;
  font-size: 14px;
  color: var(--text-color);
  outline: none;
  cursor: pointer;
  min-width: 100px;
  appearance: none;
  background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='%23ffffff' stroke-width='2'%3E%3Cpolyline points='6 9 12 15 18 9'%3E%3C/polyline%3E%3C/svg%3E");
  background-repeat: no-repeat;
  background-position: right 8px center;
  background-size: 16px;
}
.table-header {
  display: flex;
  align-items: center;
  padding: 10px 12px;
  background: var(--card-bg);
  border-radius: 8px;
  margin-bottom: 8px;
  gap: 16px;
  flex-shrink: 0;
}
.header-item {
  font-family: Inter, 'PingFang SC', 'Microsoft YaHei', sans-serif;
  font-size: 13px;
  color: var(--text-color);
  opacity: 0.6;
  font-weight: 500;
}
.icon-header { width: 40px; flex-shrink: 0; }
.version-header { width: 100px; flex-shrink: 0; }
.type-header { width: 80px; flex-shrink: 0; }
.java-header { width: 70px; flex-shrink: 0; }
.status-header { width: 80px; flex-shrink: 0; }
.time-header { flex: 1; }
.action-header { width: 44px; flex-shrink: 0; }
.download-list {
  flex: 1;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.loading-state, .empty-state {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
}
.loading-text, .empty-text {
  font-family: Inter, 'PingFang SC', 'Microsoft YaHei', sans-serif;
  font-size: 14px;
  color: var(--text-color);
  opacity: 0.5;
}
.download-card {
  display: flex;
  align-items: center;
  padding: 10px 12px;
  background: var(--card-bg);
  border-radius: 8px;
  gap: 16px;
  transition: background 0.15s ease;
}
.download-card:hover {
  background: var(--card-hover, #1a2748);
}
.download-card.installed {
  opacity: 0.7;
}
.download-card.downloading {
  border-left: 3px solid #e94560;
  background: var(--card-hover, #1a2748);
}
.download-icon {
  width: 36px;
  height: 36px;
  flex-shrink: 0;
  object-fit: contain;
}
.download-version {
  font-family: Inter, 'PingFang SC', 'Microsoft YaHei', sans-serif;
  font-size: 18px;
  font-weight: 500;
  color: var(--text-color);
  width: 100px;
  flex-shrink: 0;
}
.download-type {
  font-family: Inter, 'PingFang SC', 'Microsoft YaHei', sans-serif;
  font-size: 13px;
  color: var(--text-color);
  width: 80px;
  flex-shrink: 0;
}
.download-type.release { color: #2ecc71; }
.download-type.snapshot { color: #f39c12; }
.download-type.april_fool { color: #e94560; }
.download-type.ancient { color: #9b59b6; }
.download-type.unknown { color: #3498db; }
.download-java {
  font-family: Inter, 'PingFang SC', 'Microsoft YaHei', sans-serif;
  font-size: 12px;
  color: var(--text-color);
  opacity: 0.6;
  width: 70px;
  flex-shrink: 0;
}
.download-status {
  width: 80px;
  flex-shrink: 0;
}
.status-badge {
  font-family: Inter, 'PingFang SC', 'Microsoft YaHei', sans-serif;
  font-size: 11px;
  padding: 2px 10px;
  border-radius: 6px;
  display: inline-block;
}
.status-badge.installed {
  color: #2ecc71;
  background: rgba(46, 204, 113, 0.15);
}
.status-badge.downloading {
  color: #e94560;
  background: rgba(233, 69, 96, 0.15);
  animation: pulse 1s ease-in-out infinite;
}
.status-badge.not-installed {
  color: var(--text-color);
  opacity: 0.4;
  background: var(--button-bg);
}
.download-time {
  font-family: Inter, 'PingFang SC', 'Microsoft YaHei', sans-serif;
  font-size: 13px;
  color: var(--text-color);
  opacity: 0.5;
  flex: 1;
  text-align: left;
}
.download-action-btn {
  background: none;
  border: none;
  cursor: pointer;
  padding: 6px;
  border-radius: 6px;
  opacity: 0;
  transition: opacity 0.2s ease, transform 100ms ease;
  flex-shrink: 0;
  width: 44px;
  display: flex;
  align-items: center;
  justify-content: center;
}
.download-card:hover .download-action-btn:not(.installed):not(.downloading) {
  opacity: 1;
}
.download-action-btn.installed {
  opacity: 0.4;
  cursor: default;
}
.download-action-btn.downloading {
  opacity: 1;
  cursor: not-allowed;
  animation: spin 1s linear infinite;
}
.download-action-btn:active:not(.installed):not(.downloading) {
  transform: scale(0.92);
  background-color: var(--button-active);
}
.download-action-btn svg {
  width: 20px;
  height: 20px;
  color: var(--text-color);
}
.download-action-btn.installed svg {
  color: #2ecc71;
}
@keyframes pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.5; }
}
@keyframes spin {
  from { transform: rotate(0deg); }
  to { transform: rotate(360deg); }
}
.download-list::-webkit-scrollbar {
  width: 4px;
}
.download-list::-webkit-scrollbar-track {
  background: transparent;
}
.download-list::-webkit-scrollbar-thumb {
  background: var(--button-bg);
  border-radius: 2px;
}
.download-list::-webkit-scrollbar-thumb:hover {
  background: var(--button-active);
}
@media (prefers-reduced-motion: reduce) {
  .section-indicator {
    transition: transform 0.15s linear;
  }
  .section-item:active {
    transform: none;
  }
}
</style>