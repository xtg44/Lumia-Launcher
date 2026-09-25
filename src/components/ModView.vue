<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  getVersions,
  getLocalVersions,
  searchMods,
  installMod,
  checkModSupport,
  type ModSearchItem,
  type LocalVersionInfo,
  type ModSupportInfo,
} from '../utils/tauri'
import { translateModQuery } from '../utils/modNames'
import AppSelect from './AppSelect.vue'

const { t } = useI18n()

// ============ 筛选状态 ============
const keyword = ref('')
const loaderFilter = ref('')
const versionFilter = ref('')
const sourceFilter = ref('modrinth')

// ============ 真实 MC 版本列表（来自 Mojang 清单） ============
const mcVersions = ref<string[]>([])
const versionsLoading = ref(false)

async function loadMcVersions() {
  versionsLoading.value = true
  try {
    const list = await getVersions()
    mcVersions.value = list.map(v => v.id)
  } catch (err) {
    console.error('加载版本列表失败:', err)
  } finally {
    versionsLoading.value = false
  }
}

// ============ 模组搜索（Modrinth / CurseForge 真实数据 + 无限滚动翻页） ============
const mods = ref<ModSearchItem[]>([])
const loading = ref(false)
const searchError = ref('')
const PAGE_SIZE = 30
const pageOffset = ref(0)
const hasMore = ref(true)
const loadingMore = ref(false)

let debounceTimer: ReturnType<typeof setTimeout> | null = null

async function runSearch() {
  loading.value = true
  searchError.value = ''
  pageOffset.value = 0
  hasMore.value = true
  try {
    const list = await searchMods({
      query: translateModQuery(keyword.value.trim()),
      loader: loaderFilter.value,
      mcVersion: versionFilter.value,
      source: sourceFilter.value,
      offset: 0,
      limit: PAGE_SIZE,
    })
    mods.value = list
    hasMore.value = list.length >= PAGE_SIZE
  } catch (err) {
    searchError.value = err instanceof Error ? err.message : String(err)
    mods.value = []
  } finally {
    loading.value = false
  }
}

async function loadMore() {
  if (loading.value || loadingMore.value || !hasMore.value) return
  loadingMore.value = true
  try {
    const nextOffset = pageOffset.value + PAGE_SIZE
    const list = await searchMods({
      query: translateModQuery(keyword.value.trim()),
      loader: loaderFilter.value,
      mcVersion: versionFilter.value,
      source: sourceFilter.value,
      offset: nextOffset,
      limit: PAGE_SIZE,
    })
    for (const m of list) {
      if (!mods.value.some(x => x.id === m.id && x.source === m.source)) {
        mods.value.push(m)
      }
    }
    pageOffset.value = nextOffset
    hasMore.value = list.length >= PAGE_SIZE
  } catch (err) {
    console.error('加载更多失败:', err)
  } finally {
    loadingMore.value = false
  }
}

function onListScroll(event: Event) {
  const el = event.target as HTMLElement
  if (el.scrollHeight - el.scrollTop - el.clientHeight < 120) {
    loadMore()
  }
}

watch([loaderFilter, versionFilter, sourceFilter], () => runSearch())
watch(keyword, () => {
  if (debounceTimer) clearTimeout(debounceTimer)
  debounceTimer = setTimeout(runSearch, 350)
})

// ============ 兼容性判定 ============
function compatInfo(name: string): ModSupportInfo | null {
  return compatChecks.value[name] ?? null
}

function isCompatible(name: string): boolean {
  const info = compatInfo(name)
  return !!info && info.supported
}

function compatReason(name: string): string {
  const info = compatInfo(name)
  return info?.reason || ''
}

function depsOf(name: string): string[] {
  const info = compatInfo(name)
  return info?.dependencies || []
}

/** 第一条不支持的原因（显示在弹窗底部） */
const incompatReasonShown = computed(() => {
  const v = installedVersions.value.find((item) => !isCompatible(item.name))
  if (!v) return ''
  return compatReason(v.name)
})

// ============ 安装模组 ============
const showInstallPopup = ref(false)
const installTarget = ref<ModSearchItem | null>(null)
const installedVersions = ref<LocalVersionInfo[]>([])
const installingVersion = ref('')
const installError = ref('')
const installedIds = ref<string[]>([])
const iconFailed = ref<string[]>([])
const toastMsg = ref('')

function showToast(msg: string) {
  toastMsg.value = msg
  setTimeout(() => { toastMsg.value = '' }, 5000)
}
// 每个本地版本的兼容性检查结果（key = 版本名）；null = 检查中
const compatChecks = ref<Record<string, ModSupportInfo | null>>({})

async function openInstall(mod: ModSearchItem) {
  if (installedIds.value.includes(mod.id)) return
  installTarget.value = mod
  installError.value = ''
  installedVersions.value = []
  compatChecks.value = {}
  showInstallPopup.value = true
  try {
    installedVersions.value = await getLocalVersions()
  } catch {
    installedVersions.value = []
  }
  // 并行检查每个已装版本是否支持该模组（原版版本直接标记不支持，不发网络请求）
  const checks: Record<string, ModSupportInfo | null> = {}
  for (const v of installedVersions.value) {
    checks[v.name] = v.loader === 'vanilla' ? { supported: false, reason: '', dependencies: [] } : null
  }
  compatChecks.value = checks
  await Promise.all(
    installedVersions.value
      .filter((v) => v.loader !== 'vanilla')
      .map(async (v) => {
        try {
          checks[v.name] = await checkModSupport(mod.id, v.name, mod.source)
        } catch (err) {
          checks[v.name] = {
            supported: false,
            reason: err instanceof Error ? err.message : String(err),
            dependencies: [],
          }
        }
      }),
  )
  compatChecks.value = { ...checks }
}

async function confirmInstall(versionName: string) {
  if (!installTarget.value || installingVersion.value) return
  installingVersion.value = versionName
  installError.value = ''
  try {
    const result = await installMod(installTarget.value.id, versionName, installTarget.value.source)
    installedIds.value.push(installTarget.value.id)
    showInstallPopup.value = false
    // 提示：顺带安装的依赖
    if (result.dependencies.length > 0) {
      showToast(t('modView.depsInstalled', { deps: result.dependencies.join(', ') }))
    }
  } catch (err) {
    installError.value = err instanceof Error ? err.message : String(err)
  } finally {
    installingVersion.value = ''
  }
}

// ============ 工具函数 ============
function formatDownloads(n: number): string {
  if (n >= 1e8) return (n / 1e8).toFixed(1) + ' ' + t('modView.unitHundredMillion')
  if (n >= 1e4) return (n / 1e4).toFixed(1) + ' ' + t('modView.unitTenThousand')
  return String(n)
}

function daysAgo(iso: string): number {
  const t = new Date(iso).getTime()
  if (Number.isNaN(t)) return 0
  return Math.max(0, Math.floor((Date.now() - t) / 86400_000))
}

const LOADER_LABELS: Record<string, string> = {
  forge: 'Forge',
  fabric: 'Fabric',
  neoforge: 'NeoForge',
  quilt: 'Quilt',
  vanilla: '原版',
}

function loaderLabel(loader: string): string {
  if (loader === 'vanilla') return t('modView.loaderVanilla')
  return LOADER_LABELS[loader] ?? loader
}

const LOADER_OPTIONS = ['', 'forge', 'fabric', 'neoforge', 'quilt']
const SOURCE_OPTIONS: Array<'modrinth' | 'curseforge'> = ['modrinth', 'curseforge']
const SOURCE_LABELS: Record<string, string> = {
  modrinth: 'Modrinth',
  curseforge: 'CurseForge',
}

const loaderOptions = computed(() => [
  { value: '', label: t('modView.allLoaders') },
  ...LOADER_OPTIONS.slice(1).map(l => ({ value: l, label: loaderLabel(l) })),
])

const versionOptions = computed(() => [
  { value: '', label: t('modView.allVersions') },
  ...mcVersions.value.map(v => ({ value: v, label: v })),
])

const sourceOptions = computed(() =>
  SOURCE_OPTIONS.map(s => ({ value: s, label: SOURCE_LABELS[s] })),
)

const MOD_COLORS: string[] = [
  '#3b82f6', '#f59e0b', '#ef4444', '#a78bfa', '#22c55e', '#06b6d4',
  '#f97316', '#eab308', '#8b5cf6', '#ec4899', '#14b8a6', '#6366f1',
]

function modColor(name: string): string {
  let hash = 0
  for (let i = 0; i < name.length; i++) hash = (hash * 31 + name.charCodeAt(i)) | 0
  return MOD_COLORS[Math.abs(hash) % MOD_COLORS.length]
}

function modInitial(name: string): string {
  return name.charAt(0).toUpperCase()
}

function markIconFailed(id: string) {
  if (!iconFailed.value.includes(id)) iconFailed.value.push(id)
}

onMounted(() => {
  loadMcVersions()
  runSearch()
})
</script>

<template>
  <div class="mod-content">
    <!-- 筛选框：名称 / 模组加载器 / 版本 / 来源 -->
    <div class="filter-bar">
      <input
        type="text"
        v-model="keyword"
        :placeholder="$t('modView.namePlaceholder')"
        class="filter-input keyword-input"
      />
      <AppSelect v-model="loaderFilter" :options="loaderOptions" />
      <AppSelect v-model="versionFilter" :options="versionOptions" />
      <AppSelect v-model="sourceFilter" :options="sourceOptions" />
    </div>

    <!-- 模组列表（下载量排序 + 滚动到底自动加载更多） -->
    <div class="mod-list" @scroll="onListScroll">
      <div v-if="loading" class="empty-state">
        <span class="empty-text">{{ $t('modView.searching') }}</span>
      </div>
      <div v-else-if="searchError" class="empty-state">
        <span class="empty-text error-text">{{ $t('modView.searchFailed', { msg: searchError }) }}</span>
      </div>
      <div v-else-if="mods.length === 0" class="empty-state">
        <span class="empty-text">{{ $t('modView.noMods') }}</span>
      </div>
      <div v-for="mod in mods" :key="mod.id" class="mod-card">
        <div class="mod-icon" :style="{ background: `linear-gradient(135deg, ${modColor(mod.name)}, ${modColor(mod.name)}cc)` }">
          <img
            v-if="mod.iconUrl && !iconFailed.includes(mod.id)"
            :src="mod.iconUrl"
            :alt="mod.name"
            class="mod-icon-img"
            loading="lazy"
            @error="markIconFailed(mod.id)"
          />
          <span v-else class="mod-icon-letter">{{ modInitial(mod.name) }}</span>
        </div>

        <div class="mod-info">
          <span class="mod-name">{{ mod.name }}</span>
          <span class="mod-meta">
            {{ t('modView.meta', { count: formatDownloads(mod.downloads), days: daysAgo(mod.updatedAt) }) }}
          </span>
        </div>

        <span class="source-badge" :class="mod.source">{{ SOURCE_LABELS[mod.source] }}</span>

        <button
          class="mod-download-btn"
          :class="{ done: installedIds.includes(mod.id) }"
          @click="openInstall(mod)"
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/>
            <polyline points="7 10 12 15 17 10"/>
            <line x1="12" y1="15" x2="12" y2="3"/>
          </svg>
          <span>{{ installedIds.includes(mod.id) ? $t('modView.installed') : $t('modView.download') }}</span>
        </button>
      </div>
      <div v-if="loadingMore" class="list-footer">
        <svg class="spinning" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <line x1="12" y1="2" x2="12" y2="6"/><line x1="12" y1="18" x2="12" y2="22"/>
          <line x1="4.93" y1="4.93" x2="7.76" y2="7.76"/><line x1="16.24" y1="16.24" x2="19.07" y2="19.07"/>
          <line x1="2" y1="12" x2="6" y2="12"/><line x1="18" y1="12" x2="22" y2="12"/>
          <line x1="4.93" y1="19.07" x2="7.76" y2="16.24"/><line x1="16.24" y1="7.76" x2="19.07" y2="4.93"/>
        </svg>
        <span>{{ $t('modView.loadingMore') }}</span>
      </div>
      <div v-else-if="!hasMore && mods.length > 0" class="list-footer">
        <span>{{ $t('modView.endOfList') }}</span>
      </div>
    </div>

    <!-- 安装弹窗：选择要装到的游戏版本 -->
    <div v-if="showInstallPopup" class="popup-overlay" @click.self="showInstallPopup = false">
      <div class="popup-card">
        <div class="popup-header">
          <span class="popup-title">{{ $t('modView.installTitle', { name: installTarget?.name }) }}</span>
          <button class="popup-close" @click="showInstallPopup = false">X</button>
        </div>
        <div class="popup-body">
          <div v-if="installedVersions.length === 0" class="empty-text">
            {{ $t('modView.noInstalledVersion') }}
          </div>
          <div
            v-for="v in installedVersions"
            :key="v.name"
            class="version-row"
            :class="{ disabled: !isCompatible(v.name) }"
          >
            <span class="version-name">{{ v.name }}</span>
            <span class="loader-badge" :class="v.loader">{{ loaderLabel(v.loader) }}</span>
            <!-- 兼容性状态 -->
            <span v-if="compatChecks[v.name] === null && v.loader !== 'vanilla'" class="compat-badge checking">
              {{ $t('modView.compatChecking') }}
            </span>
            <span
              v-else-if="!isCompatible(v.name)"
              class="compat-badge no"
              :title="compatReason(v.name)"
            >
              {{ $t('modView.compatNo') }}
            </span>
            <span v-else class="compat-badge yes">{{ $t('modView.compatYes') }}</span>
            <button
              v-if="v.loader !== 'vanilla'"
              class="row-install-btn"
              :disabled="installingVersion !== '' || !isCompatible(v.name)"
              @click="confirmInstall(v.name)"
            >
              <svg v-if="installingVersion === v.name" class="spinning" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <line x1="12" y1="2" x2="12" y2="6"/><line x1="12" y1="18" x2="12" y2="22"/>
                <line x1="4.93" y1="4.93" x2="7.76" y2="7.76"/><line x1="16.24" y1="16.24" x2="19.07" y2="19.07"/>
                <line x1="2" y1="12" x2="6" y2="12"/><line x1="18" y1="12" x2="22" y2="12"/>
                <line x1="4.93" y1="19.07" x2="7.76" y2="16.24"/><line x1="16.24" y1="7.76" x2="19.07" y2="4.93"/>
              </svg>
              <span v-else>{{ $t('modView.install') }}</span>
            </button>
            <!-- 原版版本：列出但明确提示不支持安装模组，避免小白困惑 -->
            <span v-else class="vanilla-no-mods">{{ $t('modView.vanillaNoMods') }}</span>
            <!-- 依赖提示 -->
            <span v-if="isCompatible(v.name) && depsOf(v.name).length > 0" class="dep-hint">
              {{ $t('modView.depsHint', { deps: depsOf(v.name).join(', ') }) }}
            </span>
          </div>
          <p v-if="incompatReasonShown" class="error-text install-error">{{ incompatReasonShown }}</p>
          <div v-if="installError" class="error-text install-error">{{ installError }}</div>
        </div>
      </div>
    </div>

    <!-- 依赖安装提示 toast -->
    <Transition name="toast">
      <div v-if="toastMsg" class="mod-toast">{{ toastMsg }}</div>
    </Transition>
  </div>
</template>

<style scoped>
.mod-content {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 10px;
}

/* ===== 筛选框 ===== */
.filter-bar {
  display: flex;
  gap: 10px;
  padding: 10px;
  background: var(--card-bg);
  border-radius: 10px;
  flex-shrink: 0;
}
.keyword-input {
  flex: 1;
  min-width: 120px;
}
.filter-input, .filter-select {
  padding: 8px 14px;
  border: none;
  border-radius: 8px;
  background: var(--button-bg);
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 13px;
  color: var(--text-color);
  outline: none;
  box-sizing: border-box;
}
.filter-input::placeholder {
  color: var(--label-color);
}
.filter-select {
  appearance: none;
  cursor: pointer;
  min-width: 110px;
  padding-right: 28px;
  background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='%23888' stroke-width='2'%3E%3Cpolyline points='6 9 12 15 18 9'%3E%3C/polyline%3E%3C/svg%3E");
  background-repeat: no-repeat;
  background-position: right 8px center;
  background-size: 14px;
}
.filter-select option {
  background: var(--panel-bg);
  color: var(--text-color);
}

/* ===== 列表 ===== */
.mod-list {
  flex: 1;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.mod-card {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 10px 12px;
  background: var(--card-bg);
  border-radius: 10px;
  transition: background 150ms ease;
}
.mod-card:hover {
  background: var(--button-bg);
}
.mod-icon {
  width: 44px;
  height: 44px;
  border-radius: 10px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
}
.mod-icon-img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.mod-icon-letter {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 20px;
  font-weight: 700;
  color: rgba(255, 255, 255, 0.92);
}
.mod-info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.mod-name {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 15px;
  font-weight: 600;
  color: var(--text-color);
  display: block;
  width: 100%;
  max-width: 100%;
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.mod-meta {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 11px;
  color: var(--label-color);
  opacity: 0.75;
  display: block;
  width: 100%;
  max-width: 100%;
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.source-badge {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 11px;
  font-weight: 500;
  padding: 3px 10px;
  border-radius: 6px;
  flex-shrink: 0;
}
.source-badge.curseforge {
  color: #ff7849;
  background: rgba(241, 100, 54, 0.14);
}
.source-badge.modrinth {
  color: #1bd96a;
  background: rgba(27, 217, 106, 0.14);
}
.mod-download-btn {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 7px 14px;
  border: none;
  border-radius: 8px;
  background: var(--accent-color);
  color: var(--accent-text-color);
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  flex-shrink: 0;
  transition: transform 100ms ease, opacity 150ms ease;
}
.mod-download-btn svg {
  width: 15px;
  height: 15px;
}
.mod-download-btn:active {
  transform: scale(0.94);
}
.mod-download-btn.done {
  background: var(--button-bg);
  color: var(--text-color);
  opacity: 0.6;
  cursor: default;
}

/* ===== 安装弹窗 ===== */
.popup-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.45);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 100;
}
.popup-card {
  width: 420px;
  max-width: calc(100vw - 60px);
  max-height: 70vh;
  display: flex;
  flex-direction: column;
  background: var(--panel-bg);
  border-radius: 14px;
  overflow: hidden;
  box-shadow: 0 12px 40px rgba(0, 0, 0, 0.35);
}
.popup-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 14px 16px;
  border-bottom: 1px solid var(--border-color);
}
.popup-title {
  flex: 1;
  min-width: 0;
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 15px;
  font-weight: 600;
  color: var(--text-color);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.popup-close {
  background: none;
  border: none;
  cursor: pointer;
  color: var(--text-color);
  opacity: 0.6;
  font-size: 14px;
  padding: 4px 8px;
}
.popup-close:hover {
  opacity: 1;
}
.popup-body {
  flex: 1;
  overflow-y: auto;
  padding: 12px 16px 16px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.version-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 12px;
  background: var(--card-bg);
  border-radius: 10px;
}
.version-row.disabled {
  opacity: 0.5;
}
.version-name {
  flex: 1;
  min-width: 0;
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 14px;
  font-weight: 500;
  color: var(--text-color);
}
.loader-badge {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 11px;
  padding: 2px 10px;
  border-radius: 6px;
  flex-shrink: 0;
  color: var(--text-color);
  background: var(--button-bg);
  opacity: 0.8;
}
.loader-badge.forge { color: #f39c12; background: rgba(243, 156, 18, 0.15); }
.loader-badge.fabric { color: #7d8aa5; background: rgba(125, 138, 165, 0.18); }
.loader-badge.neoforge { color: #e94560; background: rgba(233, 69, 96, 0.15); }
.row-install-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  min-width: 64px;
  padding: 6px 14px;
  border: none;
  border-radius: 8px;
  background: var(--accent-color);
  color: var(--accent-text-color);
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 13px;
  cursor: pointer;
  flex-shrink: 0;
}
.row-install-btn:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}
.row-install-btn svg {
  width: 14px;
  height: 14px;
}
.vanilla-no-mods {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 12px;
  color: var(--label-color);
  opacity: 0.8;
  flex-shrink: 0;
  padding: 0 4px;
}
.spinning {
  animation: spin 1s linear infinite;
}
@keyframes spin {
  from { transform: rotate(0deg); }
  to { transform: rotate(360deg); }
}
.install-error {
  margin-top: 4px;
  font-size: 12px;
  line-height: 1.5;
  word-break: break-all;
}
.error-text {
  color: #ff3b30;
}

.empty-state {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
}
.list-footer {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 14px 0;
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 12px;
  color: var(--label-color);
  flex-shrink: 0;
}
.list-footer svg {
  width: 14px;
  height: 14px;
}
.empty-text {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 14px;
  color: var(--text-color);
  opacity: 0.5;
}
.error-text.empty-text {
  opacity: 1;
}

.mod-list::-webkit-scrollbar, .popup-body::-webkit-scrollbar {
  width: 4px;
}
.mod-list::-webkit-scrollbar-track, .popup-body::-webkit-scrollbar-track {
  background: transparent;
}
.mod-list::-webkit-scrollbar-thumb, .popup-body::-webkit-scrollbar-thumb {
  background: var(--button-bg);
  border-radius: 2px;
}

/* ===== 兼容性徽标 ===== */
.compat-badge {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 10px;
  font-weight: 500;
  padding: 2px 7px;
  border-radius: 6px;
  flex-shrink: 1;
  max-width: 96px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.compat-badge.checking {
  color: var(--label-color);
  background: var(--button-bg);
  opacity: 0.8;
}
.compat-badge.yes {
  color: #1bd96a;
  background: rgba(27, 217, 106, 0.14);
}
.compat-badge.no {
  color: #ff3b30;
  background: rgba(255, 59, 48, 0.14);
}
.dep-hint {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 10px;
  color: var(--label-color);
  opacity: 0.85;
  flex-shrink: 1;
  flex-basis: 0;
  max-width: 130px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* ===== 依赖安装提示 toast ===== */
.mod-toast {
  position: fixed;
  left: 50%;
  bottom: 74px;
  transform: translateX(-50%);
  max-width: 70vw;
  padding: 10px 18px;
  border-radius: 10px;
  background: var(--panel-bg);
  border: 1px solid var(--border-color);
  box-shadow: 0 8px 30px rgba(0, 0, 0, 0.3);
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 13px;
  color: var(--text-color);
  z-index: 200;
  word-break: break-word;
}
.toast-enter-active {
  transition: opacity 0.25s ease, transform 0.25s ease;
}
.toast-leave-active {
  transition: opacity 0.2s ease;
}
.toast-enter-from,
.toast-leave-to {
  opacity: 0;
  transform: translateX(-50%) translateY(8px);
}
</style>