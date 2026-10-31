<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, nextTick, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { save } from '@tauri-apps/plugin-dialog'
import { toast } from '../utils/toast'
import { cleanSvg } from '../utils/svg'
import {
  renameVersion,
  deleteVersion,
  listMods,
  toggleMod,
  deleteMod,
  openModsFolder,
  exportModpack,
  listenModpackExportProgress,
  listScreenshots,
  readScreenshot,
  openScreenshotsFolder,
  openVersionFolder,
  openVersionLogs,
  type ModFileInfo,
  type ModpackExportProgress,
  type ScreenshotFileInfo,
} from '../utils/tauri'
import modRaw from '../assets/icons/mod.svg?raw'
import infoRaw from '../assets/icons/info.svg?raw'
import settingsRaw from '../assets/icons/settings.svg?raw'

const { t } = useI18n()

const props = defineProps<{
  version: string
  loader: string
}>()

const emit = defineEmits<{
  (e: 'back'): void
  (e: 'changed'): void
}>()

/** 原版版本不支持模组：不显示"模组"选项卡（避免小白困惑） */
const isVanilla = computed(() => props.loader?.toLowerCase() === 'vanilla')

// ===== 选项卡 =====
type ManageTab = 'overview' | 'mods' | 'screenshots' | 'other'

const modIcon = cleanSvg(modRaw)
const infoIcon = cleanSvg(infoRaw)
const settingsIcon = cleanSvg(settingsRaw)
const screenshotsIcon = `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="5" width="18" height="14" rx="2.5"/><circle cx="9" cy="10" r="1.6"/><path d="m21 15-4.5-4.5L7 20"/></svg>`

const activeTab = ref<ManageTab>('overview')

const tabs = computed(() => {
  const list = [
    { id: 'overview' as const, label: 'versionManage.tabOverview', icon: infoIcon },
    { id: 'mods' as const, label: 'versionManage.tabMods', icon: modIcon },
    { id: 'screenshots' as const, label: 'versionManage.tabScreenshots', icon: screenshotsIcon },
    { id: 'other' as const, label: 'versionManage.tabOther', icon: settingsIcon },
  ]
  return isVanilla.value ? list.filter((tab) => tab.id !== 'mods') : list
})

const activeTabIndex = computed(() =>
  Math.max(0, tabs.value.findIndex((tab) => tab.id === activeTab.value)),
)

// ===== 重命名 =====
const newName = ref('')
const renaming = ref(false)
const renameError = ref('')

// ===== 删除版本 =====
const confirmDelete = ref(false)
const deleting = ref(false)

// ===== 模组管理 =====
const mods = ref<ModFileInfo[]>([])
const modsLoading = ref(false)
const modsLoaded = ref(false)
const modsError = ref('')
const busyMod = ref('')
const modSearch = ref('')
type ModFilter = 'all' | 'enabled' | 'disabled'
const modFilter = ref<ModFilter>('all')
const modFilters: { value: ModFilter; label: string }[] = [
  { value: 'all', label: 'versionManage.modsFilterAll' },
  { value: 'enabled', label: 'versionManage.modsFilterEnabled' },
  { value: 'disabled', label: 'versionManage.modsFilterDisabled' },
]
/** 待二次确认删除的模组文件名（Tauri WebView 不支持 window.confirm，用两步确认） */
const confirmDeleteMod = ref('')

const enabledModCount = computed(() => mods.value.filter((mod) => mod.enabled).length)

const filteredMods = computed(() => {
  let list = mods.value
  if (modFilter.value === 'enabled') list = list.filter((mod) => mod.enabled)
  if (modFilter.value === 'disabled') list = list.filter((mod) => !mod.enabled)
  const q = modSearch.value.trim().toLowerCase()
  if (q) {
    list = list.filter(
      (mod) =>
        mod.displayName.toLowerCase().includes(q) ||
        mod.fileName.toLowerCase().includes(q),
    )
  }
  return list
})

async function loadMods() {
  if (modsLoading.value) return
  modsLoading.value = true
  modsError.value = ''
  try {
    mods.value = await listMods(props.version)
  } catch (err) {
    modsError.value = err instanceof Error ? err.message : String(err)
    mods.value = []
  } finally {
    modsLoading.value = false
    modsLoaded.value = true
  }
}

async function handleToggleMod(mod: ModFileInfo) {
  if (busyMod.value) return
  busyMod.value = mod.fileName
  try {
    await toggleMod(props.version, mod.fileName, !mod.enabled)
    await loadMods()
  } catch (err) {
    toast(t('versionManage.opFailed', { msg: err instanceof Error ? err.message : String(err) }), 'error')
  } finally {
    busyMod.value = ''
  }
}

async function handleDeleteMod(mod: ModFileInfo) {
  if (busyMod.value) return
  // 两步确认：第一次点击进入确认态，4 秒内再点才真正删除
  if (confirmDeleteMod.value !== mod.fileName) {
    confirmDeleteMod.value = mod.fileName
    setTimeout(() => {
      if (confirmDeleteMod.value === mod.fileName) confirmDeleteMod.value = ''
    }, 4000)
    return
  }
  confirmDeleteMod.value = ''
  busyMod.value = mod.fileName
  try {
    await deleteMod(props.version, mod.fileName)
    await loadMods()
  } catch (err) {
    toast(t('versionManage.deleteFailed', { msg: err instanceof Error ? err.message : String(err) }), 'error')
  } finally {
    busyMod.value = ''
  }
}

async function handleOpenModsFolder() {
  try {
    await openModsFolder(props.version)
  } catch (err) {
    toast(t('versionManage.opFailed', { msg: err instanceof Error ? err.message : String(err) }), 'error')
  }
}

// ===== 截图 =====
const screenshots = ref<ScreenshotFileInfo[]>([])
const screenshotsLoaded = ref(false)
const screenshotsLoading = ref(false)
const screenshotsError = ref('')
const screenshotUrls = ref<Record<string, string>>({})
const screenshotErrors = ref<Record<string, string>>({})
const screenshotLoading = ref<Record<string, boolean>>({})
const previewShot = ref<ScreenshotFileInfo | null>(null)
let screenshotObserver: IntersectionObserver | null = null
let screenshotListPromise: Promise<void> | null = null

async function loadScreenshotList() {
  screenshotsLoading.value = true
  screenshotsError.value = ''
  try {
    screenshots.value = await listScreenshots(props.version)
    screenshotsLoaded.value = true
  } catch (err) {
    screenshotsError.value = err instanceof Error ? err.message : String(err)
    screenshots.value = []
    screenshotsLoaded.value = true
  } finally {
    screenshotsLoading.value = false
  }
}

async function ensureScreenshotList() {
  if (screenshotsLoaded.value) return
  if (!screenshotListPromise) {
    screenshotListPromise = loadScreenshotList().finally(() => {
      screenshotListPromise = null
    })
  }
  await screenshotListPromise
}

async function loadScreenshots() {
  await ensureScreenshotList()
  await nextTick()
  observeScreenshotCards()
}

function observeScreenshotCards() {
  screenshotObserver?.disconnect()
  const cards = Array.from(document.querySelectorAll<HTMLElement>('.shot-card'))
  if (cards.length === 0) return

  if (typeof IntersectionObserver === 'undefined') {
    screenshots.value.forEach((shot) => loadScreenshot(shot.fileName))
    return
  }

  screenshotObserver = new IntersectionObserver(
    (entries) => {
      for (const entry of entries) {
        if (!entry.isIntersecting) continue
        const name = (entry.target as HTMLElement).dataset.shot
        if (name) loadScreenshot(name)
        screenshotObserver?.unobserve(entry.target)
      }
    },
    { rootMargin: '160px' },
  )
  cards.forEach((card) => screenshotObserver?.observe(card))
}

async function loadScreenshot(fileName: string) {
  if (screenshotUrls.value[fileName] || screenshotLoading.value[fileName]) return
  screenshotLoading.value = { ...screenshotLoading.value, [fileName]: true }
  try {
    const url = await readScreenshot(props.version, fileName)
    screenshotUrls.value = { ...screenshotUrls.value, [fileName]: url }
    const nextErrors = { ...screenshotErrors.value }
    delete nextErrors[fileName]
    screenshotErrors.value = nextErrors
  } catch (err) {
    screenshotErrors.value = {
      ...screenshotErrors.value,
      [fileName]: err instanceof Error ? err.message : String(err),
    }
  } finally {
    const nextLoading = { ...screenshotLoading.value }
    delete nextLoading[fileName]
    screenshotLoading.value = nextLoading
  }
}

async function openScreenshot(shot: ScreenshotFileInfo) {
  if (!screenshotUrls.value[shot.fileName]) {
    await loadScreenshot(shot.fileName)
  }
  if (screenshotUrls.value[shot.fileName]) {
    previewShot.value = shot
  }
}

function closePreview() {
  previewShot.value = null
}

async function refreshScreenshots() {
  screenshotsLoaded.value = false
  screenshotUrls.value = {}
  screenshotErrors.value = {}
  screenshotLoading.value = {}
  await loadScreenshots()
}

async function handleOpenScreenshotsFolder() {
  try {
    await openScreenshotsFolder(props.version)
  } catch (err) {
    toast(t('versionManage.opFailed', { msg: err instanceof Error ? err.message : String(err) }), 'error')
  }
}

function formatShotDate(ms: number): string {
  if (!ms) return ''
  const date = new Date(ms)
  if (Number.isNaN(date.getTime())) return ''
  return date.toLocaleString(undefined, {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  })
}

// ===== 打开目录 =====
async function handleOpenVersionFolder() {
  try {
    await openVersionFolder(props.version)
  } catch (err) {
    toast(t('versionManage.opFailed', { msg: err instanceof Error ? err.message : String(err) }), 'error')
  }
}

async function handleOpenLogsFolder() {
  try {
    await openVersionLogs(props.version)
  } catch (err) {
    toast(t('versionManage.opFailed', { msg: err instanceof Error ? err.message : String(err) }), 'error')
  }
}

// ===== 导出整合包（.mrpack） =====
const exportName = ref('')
const exporting = ref(false)
const exportProgress = ref<ModpackExportProgress | null>(null)
let unlistenExportProgress: (() => void) | null = null

const exportPercent = computed(() => Math.round(exportProgress.value?.percent ?? 0))

function sanitizeFileName(name: string): string {
  const cleaned = name.trim().replace(/[\\/:*?"<>|]/g, '_').replace(/\s+/g, ' ').trim()
  return cleaned || props.version
}

async function handleExport() {
  if (exporting.value) return
  const packName = exportName.value.trim()
  if (!packName) {
    toast(t('versionManage.exportNoName'), 'error')
    return
  }
  let path: string | null = null
  try {
    path = await save({
      defaultPath: `${sanitizeFileName(packName)}.mrpack`,
      filters: [{ name: 'Modrinth Modpack', extensions: ['mrpack'] }],
    })
  } catch (err) {
    toast(t('versionManage.exportFailed', { msg: err instanceof Error ? err.message : String(err) }), 'error')
    return
  }
  if (!path) return

  exporting.value = true
  exportProgress.value = { stage: 'collect', current: 0, total: 0, percent: 0 }
  try {
    const result = await exportModpack({
      version: props.version,
      name: packName,
      outputPath: path,
    })
    toast(t('versionManage.exportSuccess', { path: result.path, count: result.fileCount }), 'success')
  } catch (err) {
    toast(t('versionManage.exportFailed', { msg: err instanceof Error ? err.message : String(err) }), 'error')
  } finally {
    exporting.value = false
    exportProgress.value = null
  }
}

// ===== 删除版本 =====
async function handleDelete() {
  if (!confirmDelete.value) {
    confirmDelete.value = true
    setTimeout(() => { confirmDelete.value = false }, 4000)
    return
  }
  if (deleting.value) return
  deleting.value = true
  try {
    await deleteVersion(props.version)
    emit('changed')
    emit('back')
  } catch (err) {
    toast(t('versionManage.deleteFailed', { msg: err instanceof Error ? err.message : String(err) }), 'error')
  } finally {
    deleting.value = false
  }
}

async function handleRename() {
  if (renaming.value) return
  if (newName.value.trim() === props.version) {
    renameError.value = t('versionManage.renameNoChange')
    return
  }
  renaming.value = true
  renameError.value = ''
  try {
    await renameVersion(props.version, newName.value.trim())
    emit('changed')
    emit('back')
  } catch (err) {
    renameError.value = err instanceof Error ? err.message : String(err)
  } finally {
    renaming.value = false
  }
}

function formatSize(bytes: number): string {
  if (bytes >= 1048576) return (bytes / 1048576).toFixed(1) + ' MB'
  if (bytes >= 1024) return (bytes / 1024).toFixed(0) + ' KB'
  return bytes + ' B'
}

function handleKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape' && previewShot.value) closePreview()
}

watch(activeTab, async (tab) => {
  if (tab === 'mods' && !modsLoaded.value) await loadMods()
  if (tab === 'screenshots') await loadScreenshots()
})

onMounted(() => {
  newName.value = props.version
  exportName.value = props.version
  if (!isVanilla.value) loadMods()
  // 概览页需要截图数量；截图页首次打开时也要触发懒加载
  loadScreenshots()
  window.addEventListener('keydown', handleKeydown)
  listenModpackExportProgress((data) => {
    exportProgress.value = data
  }).then((unlisten) => {
    unlistenExportProgress = unlisten
  }).catch(() => {
    // 非 Tauri 环境（纯浏览器预览）没有事件系统，忽略即可
  })
})

onUnmounted(() => {
  window.removeEventListener('keydown', handleKeydown)
  screenshotObserver?.disconnect()
  screenshotObserver = null
  if (unlistenExportProgress) {
    unlistenExportProgress()
    unlistenExportProgress = null
  }
})
</script>

<template>
  <div class="manage-content">
    <div class="manage-header">
      <button class="back-btn" @click="emit('back')" :title="$t('versionManage.back')">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <polyline points="15 18 9 12 15 6" />
        </svg>
      </button>
      <div class="manage-heading">
        <span class="manage-title">{{ $t('versionManage.title') }}</span>
        <span class="manage-subtitle">{{ version }} · {{ loader }}</span>
      </div>
      <div class="header-spacer"></div>
    </div>

    <div class="manage-layout">
      <aside class="manage-nav">
        <div
          class="nav-indicator"
          :style="{ transform: `translateY(${activeTabIndex * 62 + 9}px)` }"
        ></div>
        <button
          v-for="tab in tabs"
          :key="tab.id"
          class="nav-item"
          :class="{ active: activeTab === tab.id }"
          @click="activeTab = tab.id"
        >
          <span class="nav-icon" v-html="tab.icon"></span>
          <span class="nav-label">{{ $t(tab.label) }}</span>
        </button>
      </aside>

      <div class="manage-panel">
        <!-- 概览 -->
        <div v-if="activeTab === 'overview'" class="tab-panel">
          <div class="version-summary">
            <div class="summary-main">
              <span class="summary-name">{{ version }}</span>
              <span class="summary-loader">{{ loader }}</span>
            </div>
            <button class="mini-btn" @click="handleOpenVersionFolder">
              {{ $t('versionManage.openVersionFolder') }}
            </button>
          </div>

          <div class="overview-stats">
            <div v-if="!isVanilla" class="stat-card">
              <span class="stat-value">{{ mods.length }}</span>
              <span class="stat-label">{{ $t('versionManage.statsMods') }}</span>
            </div>
            <div class="stat-card">
              <span class="stat-value">{{ screenshots.length }}</span>
              <span class="stat-label">{{ $t('versionManage.statsScreenshots') }}</span>
            </div>
          </div>

          <div class="manage-item">
            <label class="manage-label">{{ $t('versionManage.renameLabel') }}</label>
            <div class="manage-row">
              <input
                type="text"
                v-model="newName"
                class="manage-input"
                :placeholder="$t('versionManage.renamePlaceholder')"
              />
              <button class="action-btn primary" :disabled="renaming" @click="handleRename">
                {{ renaming ? $t('versionManage.renaming') : $t('versionManage.rename') }}
              </button>
            </div>
            <span v-if="renameError" class="error-text">{{ renameError }}</span>
          </div>
        </div>

        <!-- 模组 -->
        <div v-if="activeTab === 'mods'" class="tab-panel">
          <div class="panel-head">
            <div class="panel-heading">
              <span class="panel-title">{{ $t('versionManage.modsLabel', { count: mods.length }) }}</span>
              <span class="panel-subtitle">
                {{ $t('versionManage.modsEnabledCount', { enabled: enabledModCount, total: mods.length }) }}
              </span>
            </div>
            <div class="panel-actions">
              <button class="mini-btn" :disabled="modsLoading" @click="loadMods">
                {{ $t('versionManage.refresh') }}
              </button>
              <button class="mini-btn" @click="handleOpenModsFolder">
                {{ $t('versionManage.openModsFolder') }}
              </button>
            </div>
          </div>

          <div class="mods-toolbar">
            <input
              v-model="modSearch"
              type="text"
              class="manage-input mods-search"
              :placeholder="$t('versionManage.modsSearchPlaceholder')"
            />
            <div class="filter-pills">
              <button
                v-for="filter in modFilters"
                :key="filter.value"
                class="filter-pill"
                :class="{ active: modFilter === filter.value }"
                @click="modFilter = filter.value"
              >
                {{ $t(filter.label) }}
              </button>
            </div>
          </div>

          <div class="mods-list">
            <div v-if="modsLoading" class="mods-empty">{{ $t('versionManage.loading') }}</div>
            <div v-else-if="modsError" class="mods-empty error-text">{{ modsError }}</div>
            <div v-else-if="mods.length === 0" class="mods-empty">
              {{ $t('versionManage.noMods') }}
            </div>
            <div
              v-else-if="filteredMods.length === 0"
              class="mods-empty"
            >
              {{ $t('versionManage.noModsMatch') }}
            </div>
            <div
              v-for="mod in filteredMods"
              :key="mod.fileName"
              class="mod-row"
              :class="{ disabled: !mod.enabled }"
            >
              <span class="mod-dot" :class="{ on: mod.enabled }"></span>
              <div class="mod-info">
                <span class="mod-name">{{ mod.displayName }}</span>
                <span class="mod-file">{{ formatSize(mod.size) }} · {{ mod.fileName }}</span>
              </div>
              <button
                class="mini-btn"
                :disabled="busyMod !== ''"
                @click="handleToggleMod(mod)"
              >
                {{ mod.enabled ? $t('versionManage.disable') : $t('versionManage.enable') }}
              </button>
              <button class="mini-btn danger" :disabled="busyMod !== ''" @click="handleDeleteMod(mod)">
                {{ confirmDeleteMod === mod.fileName ? $t('versionManage.confirmDeleteAgain') : $t('common.delete') }}
              </button>
            </div>
          </div>
        </div>

        <!-- 截图 -->
        <div v-if="activeTab === 'screenshots'" class="tab-panel">
          <div class="panel-head">
            <div class="panel-heading">
              <span class="panel-title">{{ $t('versionManage.screenshotsCount', { count: screenshots.length }) }}</span>
              <span class="panel-subtitle">{{ $t('versionManage.screenshotsHint') }}</span>
            </div>
            <div class="panel-actions">
              <button class="mini-btn" :disabled="screenshotsLoading" @click="refreshScreenshots">
                {{ $t('versionManage.refresh') }}
              </button>
              <button class="mini-btn" @click="handleOpenScreenshotsFolder">
                {{ $t('versionManage.openScreenshotsFolder') }}
              </button>
            </div>
          </div>

          <div v-if="screenshotsLoading" class="tab-empty">
            {{ $t('common.loading') }}
          </div>
          <div v-else-if="screenshotsError" class="tab-empty error-text">
            {{ screenshotsError }}
          </div>
          <div v-else-if="screenshots.length === 0" class="tab-empty">
            <span>{{ $t('versionManage.noScreenshots') }}</span>
            <span class="tab-empty-hint">{{ $t('versionManage.screenshotsHint') }}</span>
          </div>
          <div v-else class="screenshot-grid">
            <button
              v-for="shot in screenshots"
              :key="shot.fileName"
              class="shot-card"
              :data-shot="shot.fileName"
              @click="openScreenshot(shot)"
            >
              <img
                v-if="screenshotUrls[shot.fileName]"
                :src="screenshotUrls[shot.fileName]"
                :alt="shot.fileName"
                loading="lazy"
              />
              <span v-else-if="screenshotErrors[shot.fileName]" class="shot-placeholder error-text">
                {{ $t('versionManage.screenshotLoadFailed') }}
              </span>
              <span v-else class="shot-placeholder">
                <span class="shot-spinner"></span>
              </span>
              <span class="shot-meta">
                <span class="shot-name">{{ shot.fileName }}</span>
                <span class="shot-sub">
                  {{ formatSize(shot.size) }}<template v-if="shot.modified"> · {{ formatShotDate(shot.modified) }}</template>
                </span>
              </span>
            </button>
          </div>
        </div>

        <!-- 其他 -->
        <div v-if="activeTab === 'other'" class="tab-panel">
          <div class="manage-item">
            <label class="manage-label">{{ $t('versionManage.exportLabel') }}</label>
            <span class="manage-hint">{{ $t('versionManage.exportHint') }}</span>
            <div class="manage-row">
              <input
                type="text"
                v-model="exportName"
                class="manage-input"
                :placeholder="$t('versionManage.exportNamePlaceholder')"
                :disabled="exporting"
              />
              <button class="action-btn primary" :disabled="exporting" @click="handleExport">
                {{ exporting ? $t('versionManage.exporting') : $t('versionManage.exportBtn') }}
              </button>
            </div>
            <div v-if="exportProgress" class="export-progress">
              <div class="export-progress-row">
                <span>{{ exportProgress.stage === 'collect' ? $t('versionManage.exportStageCollect') : $t('versionManage.exportStagePack') }}</span>
                <span v-if="exportProgress.total > 0">
                  {{ $t('versionManage.exportProgress', { current: exportProgress.current, total: exportProgress.total, percent: exportPercent }) }}
                </span>
              </div>
              <div class="export-progress-track">
                <div class="export-progress-bar" :style="{ width: exportPercent + '%' }"></div>
              </div>
            </div>
          </div>

          <div class="manage-item">
            <label class="manage-label">{{ $t('versionManage.quickActions') }}</label>
            <div class="quick-actions">
              <button class="mini-btn" @click="handleOpenVersionFolder">
                {{ $t('versionManage.openVersionFolder') }}
              </button>
              <button class="mini-btn" @click="handleOpenLogsFolder">
                {{ $t('versionManage.openLogsFolder') }}
              </button>
              <button class="mini-btn" @click="handleOpenScreenshotsFolder">
                {{ $t('versionManage.openScreenshotsFolder') }}
              </button>
            </div>
          </div>

          <div class="manage-item danger-zone">
            <label class="manage-label">{{ $t('versionManage.dangerLabel') }}</label>
            <button class="action-btn danger" :disabled="deleting" @click="handleDelete">
              {{ confirmDelete ? $t('versionManage.confirmDeleteAgain') : deleting ? $t('versionManage.deleting') : $t('versionManage.deleteVersion') }}
            </button>
            <span class="danger-hint">{{ $t('versionManage.dangerHint') }}</span>
          </div>
        </div>
      </div>
    </div>

    <Teleport to="body">
      <div v-if="previewShot" class="shot-lightbox" @click.self="closePreview">
        <button class="shot-lightbox-close" :title="$t('common.close')" @click="closePreview">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M18 6 6 18M6 6l12 12" />
          </svg>
        </button>
        <img
          v-if="screenshotUrls[previewShot.fileName]"
          :src="screenshotUrls[previewShot.fileName]"
          :alt="previewShot.fileName"
        />
        <div class="shot-lightbox-caption">{{ previewShot.fileName }}</div>
      </div>
    </Teleport>
  </div>
</template>

<style scoped>
.manage-content {
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
  padding: 10px 17px;
  box-sizing: border-box;
}
.manage-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 0;
  border-bottom: 1px solid var(--border-color);
  flex-shrink: 0;
}
.back-btn {
  background: none;
  border: none;
  cursor: pointer;
  padding: 6px;
  border-radius: 6px;
  color: var(--text-color);
  transition: background-color 150ms ease;
}
.back-btn:active {
  transform: scale(0.92);
  background-color: var(--button-active);
}
.back-btn svg {
  width: 22px;
  height: 22px;
}
.manage-heading {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
  min-width: 0;
}
.manage-title {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 18px;
  font-weight: 600;
  color: var(--text-color);
}
.manage-subtitle {
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 10px;
  color: var(--label-color);
}
.header-spacer {
  width: 34px;
}

.manage-layout {
  flex: 1;
  min-height: 0;
  display: flex;
  margin-top: 8px;
}
.manage-nav {
  width: 62px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  padding: 5px 0 0;
  border-right: 1px solid var(--border-color);
  box-sizing: border-box;
  position: relative;
}
.nav-indicator {
  position: absolute;
  top: 0;
  left: 5px;
  width: 52px;
  height: 50px;
  border-radius: 10px;
  background: var(--indicator-bg);
  transition: transform 0.4s cubic-bezier(0.34, 1.2, 0.64, 1);
  pointer-events: none;
}
.nav-item {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  width: 52px;
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
.nav-item:active {
  transform: scale(0.95);
}
.nav-icon {
  width: 26px;
  height: 26px;
  display: block;
  color: var(--text-color);
}
.nav-icon svg {
  width: 100%;
  height: 100%;
  display: block;
}
.nav-label {
  font-family: Inter, 'PingFang SC', 'Microsoft YaHei', sans-serif;
  font-size: 10px;
  line-height: 13px;
  color: var(--text-color);
  opacity: 0.7;
  white-space: nowrap;
}
.nav-item.active .nav-label {
  opacity: 1;
  font-weight: 600;
}

.manage-panel {
  flex: 1;
  min-width: 0;
  min-height: 0;
}
.tab-panel {
  height: 100%;
  overflow-y: auto;
  padding: 16px 18px;
  box-sizing: border-box;
  display: flex;
  flex-direction: column;
  gap: 18px;
}

.version-summary {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 14px 16px;
  background: var(--card-bg);
  border-radius: 12px;
}
.summary-main {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 7px;
}
.summary-name {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 20px;
  line-height: 1.2;
  font-weight: 600;
  color: var(--text-color);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.summary-loader {
  align-self: flex-start;
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 11px;
  padding: 2px 10px;
  border-radius: 6px;
  background: var(--button-bg);
  color: var(--text-color);
  opacity: 0.8;
}
.overview-stats {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 10px;
}
.stat-card {
  display: flex;
  flex-direction: column;
  gap: 3px;
  padding: 12px 14px;
  background: var(--card-bg);
  border-radius: 10px;
}
.stat-value {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 22px;
  line-height: 1.1;
  font-weight: 600;
  color: var(--text-color);
}
.stat-label {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 11px;
  color: var(--label-color);
}

.manage-item {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.manage-label {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 13px;
  font-weight: 500;
  color: var(--text-color);
  opacity: 0.85;
}
.manage-row {
  display: flex;
  gap: 8px;
}
.manage-input {
  flex: 1;
  padding: 9px 14px;
  border: 1px solid var(--border-color);
  border-radius: 8px;
  background: var(--button-bg);
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 13px;
  color: var(--text-color);
  outline: none;
}
.manage-input:focus { border-color: var(--accent-color); }
.manage-hint {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 11px;
  line-height: 1.5;
  color: var(--label-color);
}
.export-progress {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin-top: 2px;
}
.export-progress-row {
  display: flex;
  justify-content: space-between;
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 11px;
  color: var(--label-color);
}
.export-progress-track {
  height: 5px;
  border-radius: 999px;
  background: var(--button-bg);
  overflow: hidden;
}
.export-progress-bar {
  height: 100%;
  border-radius: 999px;
  background: var(--accent-color);
  transition: width 150ms ease;
}

.panel-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
  flex-shrink: 0;
}
.panel-heading {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 3px;
}
.panel-title {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 15px;
  font-weight: 600;
  color: var(--text-color);
}
.panel-subtitle {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 11px;
  line-height: 1.45;
  color: var(--label-color);
}
.panel-actions {
  display: flex;
  gap: 6px;
  flex-shrink: 0;
}
.quick-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.mods-toolbar {
  display: flex;
  gap: 8px;
  align-items: center;
}
.mods-search {
  min-width: 0;
}
.filter-pills {
  display: flex;
  gap: 5px;
  flex-shrink: 0;
}
.filter-pill {
  padding: 7px 11px;
  border-radius: 8px;
  border: 1px solid var(--border-color);
  background: transparent;
  color: var(--text-color);
  cursor: pointer;
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 11px;
  white-space: nowrap;
  transition: opacity 150ms ease, background-color 150ms ease;
}
.filter-pill:hover { opacity: 0.75; }
.filter-pill.active {
  background: var(--accent-color);
  border-color: transparent;
  color: var(--accent-text-color);
}
.mods-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.mods-empty {
  padding: 28px 16px;
  text-align: center;
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 12px;
  color: var(--label-color);
}
.mod-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 12px;
  background: var(--card-bg);
  border-radius: 8px;
}
.mod-row.disabled { opacity: 0.55; }
.mod-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--button-bg);
  flex-shrink: 0;
}
.mod-dot.on { background: #2ecc71; }
.mod-info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 1px;
}
.mod-name {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 13px;
  color: var(--text-color);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.mod-file {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 10px;
  color: var(--label-color);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.screenshot-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
  gap: 10px;
}
.shot-card {
  position: relative;
  display: block;
  width: 100%;
  aspect-ratio: 16 / 9;
  padding: 0;
  border: none;
  border-radius: 10px;
  overflow: hidden;
  background: var(--card-bg);
  cursor: zoom-in;
}
.shot-card img {
  width: 100%;
  height: 100%;
  object-fit: cover;
  display: block;
}
.shot-placeholder {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 12px;
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 11px;
  text-align: center;
}
.shot-spinner {
  width: 18px;
  height: 18px;
  border: 2px solid var(--border-color);
  border-top-color: var(--accent-color);
  border-radius: 50%;
  animation: shot-spin 0.8s linear infinite;
}
@keyframes shot-spin {
  to { transform: rotate(360deg); }
}
.shot-meta {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 1px;
  padding: 22px 10px 8px;
  background: linear-gradient(to top, rgba(0, 0, 0, 0.78), transparent);
  color: #fff;
  text-align: left;
  pointer-events: none;
}
.shot-name {
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 11px;
  font-weight: 500;
}
.shot-sub {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 9px;
  opacity: 0.8;
}
.tab-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 4px;
  padding: 48px 16px;
  text-align: center;
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 13px;
  color: var(--label-color);
}
.tab-empty-hint {
  font-size: 11px;
  opacity: 0.8;
}

.shot-lightbox {
  position: fixed;
  inset: 0;
  z-index: 3000;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 38px;
  background: rgba(5, 10, 18, 0.88);
  backdrop-filter: blur(8px);
}
.shot-lightbox img {
  max-width: 100%;
  max-height: calc(100vh - 110px);
  object-fit: contain;
  border-radius: 12px;
  box-shadow: 0 24px 80px rgba(0, 0, 0, 0.55);
}
.shot-lightbox-close {
  position: absolute;
  top: 16px;
  right: 16px;
  width: 34px;
  height: 34px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  border-radius: 50%;
  background: rgba(255, 255, 255, 0.12);
  color: #fff;
  cursor: pointer;
  transition: background-color 150ms ease;
}
.shot-lightbox-close:hover { background: rgba(255, 255, 255, 0.22); }
.shot-lightbox-close svg {
  width: 18px;
  height: 18px;
}
.shot-lightbox-caption {
  position: absolute;
  left: 50%;
  bottom: 12px;
  transform: translateX(-50%);
  max-width: 80%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 11px;
  color: rgba(255, 255, 255, 0.72);
}

.action-btn {
  padding: 9px 18px;
  border-radius: 8px;
  border: none;
  cursor: pointer;
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 13px;
  font-weight: 500;
  transition: transform 100ms ease, opacity 150ms ease;
}
.action-btn:active { transform: scale(0.95); }
.action-btn:disabled { opacity: 0.5; cursor: not-allowed; }
.action-btn.primary {
  background: var(--accent-color);
  color: var(--accent-text-color);
}
.action-btn.danger {
  background: rgba(255, 59, 48, 0.15);
  color: #ff3b30;
}
.danger-hint {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 11px;
  color: var(--label-color);
}
.danger-zone {
  padding-top: 14px;
  border-top: 1px solid var(--border-color);
  align-items: flex-start;
}

.mini-btn {
  padding: 4px 12px;
  border-radius: 7px;
  border: 1px solid var(--border-color);
  background: transparent;
  color: var(--text-color);
  cursor: pointer;
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 12px;
  flex-shrink: 0;
  transition: opacity 150ms ease;
}
.mini-btn:hover { opacity: 0.75; }
.mini-btn:disabled { opacity: 0.4; cursor: not-allowed; }
.mini-btn.danger { color: #ff3b30; border-color: rgba(255, 59, 48, 0.4); }

.error-text {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 12px;
  color: #ff3b30;
}
</style>
