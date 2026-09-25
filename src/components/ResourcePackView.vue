<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import AppSelect from './AppSelect.vue'
import {
  getVersions,
  getLocalVersions,
  searchResourcePacks,
  installResourcePack,
  type ModSearchItem,
  type LocalVersionInfo,
} from '../utils/tauri'

const { t } = useI18n()

// ============ 筛选状态 ============
const keyword = ref('')
const versionFilter = ref('')

// ============ 真实 MC 版本列表（来自 Mojang 清单） ============
const mcVersions = ref<string[]>([])
const versionsLoading = ref(false)

const versionOptions = computed(() => [
  { value: '', label: t('resourcePack.allVersions') },
  ...mcVersions.value.map(v => ({ value: v, label: v })),
])

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

// ============ 资源包搜索（Modrinth 真实数据 + 无限滚动翻页） ============
const packs = ref<ModSearchItem[]>([])
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
    const list = await searchResourcePacks({
      query: keyword.value.trim(),
      mcVersion: versionFilter.value,
      offset: 0,
      limit: PAGE_SIZE,
    })
    packs.value = list
    hasMore.value = list.length === PAGE_SIZE
  } catch (err) {
    searchError.value = err instanceof Error ? err.message : String(err)
    packs.value = []
  } finally {
    loading.value = false
  }
}

async function loadMore() {
  if (loading.value || loadingMore.value || !hasMore.value) return
  loadingMore.value = true
  try {
    const nextOffset = pageOffset.value + PAGE_SIZE
    const list = await searchResourcePacks({
      query: keyword.value.trim(),
      mcVersion: versionFilter.value,
      offset: nextOffset,
      limit: PAGE_SIZE,
    })
    for (const m of list) {
      if (!packs.value.some(x => x.id === m.id && x.source === m.source)) {
        packs.value.push(m)
      }
    }
    pageOffset.value = nextOffset
    hasMore.value = list.length === PAGE_SIZE
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

watch(versionFilter, () => runSearch())
watch(keyword, () => {
  if (debounceTimer) clearTimeout(debounceTimer)
  debounceTimer = setTimeout(runSearch, 350)
})

// ============ 安装资源包 ============
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

async function openInstall(pack: ModSearchItem) {
  if (installedIds.value.includes(pack.id)) return
  installTarget.value = pack
  installError.value = ''
  installedVersions.value = []
  showInstallPopup.value = true
  try {
    installedVersions.value = await getLocalVersions()
  } catch {
    installedVersions.value = []
  }
}

async function confirmInstall(versionName: string) {
  if (!installTarget.value || installingVersion.value) return
  installingVersion.value = versionName
  installError.value = ''
  try {
    const result = await installResourcePack(installTarget.value.id, versionName)
    installedIds.value.push(installTarget.value.id)
    showInstallPopup.value = false
    if (result.dependencies.length > 0) {
      showToast(t('resourcePack.depsInstalled', { deps: result.dependencies.join(', ') }))
    } else {
      showToast(t('resourcePack.installedOk', { name: result.fileName }))
    }
  } catch (err) {
    installError.value = err instanceof Error ? err.message : String(err)
  } finally {
    installingVersion.value = ''
  }
}

// ============ 工具函数 ============
function formatDownloads(n: number): string {
  if (n >= 1e8) return (n / 1e8).toFixed(1) + ' ' + t('resourcePack.unitHundredMillion')
  if (n >= 1e4) return (n / 1e4).toFixed(1) + ' ' + t('resourcePack.unitTenThousand')
  return String(n)
}

function daysAgo(iso: string): number {
  const d = new Date(iso).getTime()
  if (Number.isNaN(d)) return 0
  return Math.max(0, Math.floor((Date.now() - d) / 86400_000))
}

const PACK_COLORS: string[] = [
  '#3b82f6', '#f59e0b', '#ef4444', '#a78bfa', '#22c55e', '#06b6d4',
  '#f97316', '#eab308', '#8b5cf6', '#ec4899', '#14b8a6', '#6366f1',
]

function packColor(name: string): string {
  let hash = 0
  for (let i = 0; i < name.length; i++) hash = (hash * 31 + name.charCodeAt(i)) | 0
  return PACK_COLORS[Math.abs(hash) % PACK_COLORS.length]
}

function packInitial(name: string): string {
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
  <div class="rp-content">
    <!-- 筛选框：名称 / MC 版本（资源包不区分加载器，原版即可用） -->
    <div class="filter-bar">
      <input
        type="text"
        v-model="keyword"
        :placeholder="$t('resourcePack.namePlaceholder')"
        class="filter-input keyword-input"
      />
      <AppSelect v-model="versionFilter" :options="versionOptions" />
    </div>

    <!-- 资源包列表（按下载量排序 + 滚动到底自动加载更多） -->
    <div class="rp-list" @scroll="onListScroll">
      <div v-if="loading" class="empty-state">
        <span class="empty-text">{{ $t('resourcePack.searching') }}</span>
      </div>
      <div v-else-if="searchError" class="empty-state">
        <span class="empty-text error-text">{{ $t('resourcePack.searchFailed', { msg: searchError }) }}</span>
      </div>
      <div v-else-if="packs.length === 0" class="empty-state">
        <span class="empty-text">{{ $t('resourcePack.noPacks') }}</span>
      </div>
      <div v-for="pack in packs" :key="pack.id" class="rp-card">
        <div class="rp-icon" :style="{ background: `linear-gradient(135deg, ${packColor(pack.name)}, ${packColor(pack.name)}cc)` }">
          <img
            v-if="pack.iconUrl && !iconFailed.includes(pack.id)"
            :src="pack.iconUrl"
            :alt="pack.name"
            class="rp-icon-img"
            loading="lazy"
            @error="markIconFailed(pack.id)"
          />
          <span v-else class="rp-icon-letter">{{ packInitial(pack.name) }}</span>
        </div>

        <div class="rp-info">
          <span class="rp-name">{{ pack.name }}</span>
          <span class="rp-meta">
            {{ t('resourcePack.meta', { count: formatDownloads(pack.downloads), days: daysAgo(pack.updatedAt) }) }}
          </span>
        </div>

        <button
          class="rp-download-btn"
          :class="{ done: installedIds.includes(pack.id) }"
          @click="openInstall(pack)"
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/>
            <polyline points="7 10 12 15 17 10"/>
            <line x1="12" y1="15" x2="12" y2="3"/>
          </svg>
          <span>{{ installedIds.includes(pack.id) ? $t('resourcePack.installed') : $t('resourcePack.download') }}</span>
        </button>
      </div>
      <div v-if="loadingMore" class="list-footer">
        <svg class="spinning" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <line x1="12" y1="2" x2="12" y2="6"/><line x1="12" y1="18" x2="12" y2="22"/>
          <line x1="4.93" y1="4.93" x2="7.76" y2="7.76"/><line x1="16.24" y1="16.24" x2="19.07" y2="19.07"/>
          <line x1="2" y1="12" x2="6" y2="12"/><line x1="18" y1="12" x2="22" y2="12"/>
          <line x1="4.93" y1="19.07" x2="7.76" y2="16.24"/><line x1="16.24" y1="7.76" x2="19.07" y2="4.93"/>
        </svg>
        <span>{{ $t('resourcePack.loadingMore') }}</span>
      </div>
      <div v-else-if="!hasMore && packs.length > 0" class="list-footer">
        <span>{{ $t('resourcePack.endOfList') }}</span>
      </div>
    </div>

    <!-- 安装弹窗：选择要装到的游戏版本 -->
    <div v-if="showInstallPopup" class="popup-overlay" @click.self="showInstallPopup = false">
      <div class="popup-card">
        <div class="popup-header">
          <span class="popup-title">{{ $t('resourcePack.installTitle', { name: installTarget?.name }) }}</span>
          <button class="popup-close" @click="showInstallPopup = false">X</button>
        </div>
        <div class="popup-body">
          <div v-if="installedVersions.length === 0" class="empty-text">
            {{ $t('resourcePack.noInstalledVersion') }}
          </div>
          <div
            v-for="v in installedVersions"
            :key="v.name"
            class="version-row"
          >
            <span class="version-name">{{ v.name }}</span>
            <span class="loader-badge" :class="v.loader">{{ $t('resourcePack.loaderLabels.' + v.loader) }}</span>
            <button
              class="row-install-btn"
              :disabled="installingVersion !== ''"
              @click="confirmInstall(v.name)"
            >
              <svg v-if="installingVersion === v.name" class="spinning" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <line x1="12" y1="2" x2="12" y2="6"/><line x1="12" y1="18" x2="12" y2="22"/>
                <line x1="4.93" y1="4.93" x2="7.76" y2="7.76"/><line x1="16.24" y1="16.24" x2="19.07" y2="19.07"/>
                <line x1="2" y1="12" x2="6" y2="12"/><line x1="18" y1="12" x2="22" y2="12"/>
                <line x1="4.93" y1="19.07" x2="7.76" y2="16.24"/><line x1="16.24" y1="7.76" x2="19.07" y2="4.93"/>
              </svg>
              <span v-else>{{ $t('resourcePack.install') }}</span>
            </button>
          </div>
          <div v-if="installError" class="error-text install-error">{{ installError }}</div>
        </div>
      </div>
    </div>

    <!-- 安装完成提示 toast -->
    <Transition name="toast">
      <div v-if="toastMsg" class="rp-toast">{{ toastMsg }}</div>
    </Transition>
  </div>
</template>

<style scoped>
.rp-content {
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
.rp-list {
  flex: 1;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.rp-card {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 10px 12px;
  background: var(--card-bg);
  border-radius: 10px;
  transition: background 150ms ease;
}
.rp-card:hover {
  background: var(--button-bg);
}
.rp-icon {
  width: 44px;
  height: 44px;
  border-radius: 10px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
}
.rp-icon-img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.rp-icon-letter {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 20px;
  font-weight: 700;
  color: rgba(255, 255, 255, 0.92);
}
.rp-info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.rp-name {
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
.rp-meta {
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
.rp-download-btn {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-left: auto;
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
.rp-download-btn svg {
  width: 15px;
  height: 15px;
}
.rp-download-btn:active {
  transform: scale(0.94);
}
.rp-download-btn.done {
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

/* ===== 空状态 / 列表脚 ===== */
.empty-state {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
}
.empty-text {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 14px;
  color: var(--text-color);
  opacity: 0.5;
}
.error-text {
  color: #ff3b30;
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

/* ===== toast ===== */
.rp-toast {
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