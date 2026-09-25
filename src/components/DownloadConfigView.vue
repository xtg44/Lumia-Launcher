<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { getFabricVersions, getForgeVersions, getNeoForgeVersions } from '../utils/tauri'
import { useI18n } from 'vue-i18n'
import { toast } from '../utils/toast'

const { t } = useI18n()

const props = defineProps<{
  version: string
}>()

const emit = defineEmits<{
  (e: 'back'): void
  (e: 'confirm', config: {
    version: string
    displayName: string
    loader: 'none' | 'fabric' | 'forge' | 'neoforge'
    loaderVersion: string
    installFabricApi: boolean
  }): void
}>()

const displayName = ref(props.version)
const selectedLoader = ref<'none' | 'fabric' | 'forge' | 'neoforge'>('none')
const selectedLoaderVersion = ref('')
const showVersionList = ref(false)

const fabricVersions = ref<string[]>([])
const forgeVersions = ref<string[]>([])
const neoforgeVersions = ref<string[]>([])
const loading = ref(false)
const loaderError = ref('')

const loaders = [
  { value: 'none', label: t('downloadConfig.loaderNone') },
  { value: 'fabric', label: 'Fabric' },
  { value: 'forge', label: 'Forge' },
  { value: 'neoforge', label: 'NeoForge' },
]

const currentLoaderVersions = computed(() => {
  switch (selectedLoader.value) {
    case 'fabric': return fabricVersions.value
    case 'forge': return forgeVersions.value
    case 'neoforge': return neoforgeVersions.value
    default: return []
  }
})

watch(currentLoaderVersions, (versions) => {
  if (selectedLoader.value !== 'none' && versions.length > 0 && !selectedLoaderVersion.value) {
    selectedLoaderVersion.value = versions[0]
  }
})

async function loadLoaderVersions() {
  loading.value = true
  loaderError.value = ''
  try {
    const results = await Promise.allSettled([
      getFabricVersions(props.version),
      getForgeVersions(props.version),
      getNeoForgeVersions(props.version),
    ])
    const [fabric, forge, neoforge] = results.map(r => r.status === 'fulfilled' ? r.value : [])
    fabricVersions.value = fabric
    forgeVersions.value = forge
    neoforgeVersions.value = neoforge
    results.forEach((r, i) => {
      if (r.status === 'rejected') {
        console.error(`加载 Loader 版本失败 (${i}):`, r.reason)
      }
    })
    const hasError = results.some(r => r.status === 'rejected')
    if (hasError) {
      loaderError.value = t('downloadConfig.loaderError')
    }
  } catch (err) {
    console.error('加载 Loader 版本失败:', err)
    loaderError.value = t('downloadConfig.loadFailed', { msg: String(err) })
  } finally {
    loading.value = false
  }
}

function handleLoaderChange(loader: 'none' | 'fabric' | 'forge' | 'neoforge') {
  selectedLoader.value = loader
  if (loader !== 'none') {
    showVersionList.value = true
    if (currentLoaderVersions.value.length > 0) {
      selectedLoaderVersion.value = currentLoaderVersions.value[0]
    }
  } else {
    showVersionList.value = false
    selectedLoaderVersion.value = ''
  }
}

function handleConfirm() {
  if (selectedLoader.value !== 'none' && !selectedLoaderVersion.value) {
    const label = loaders.find(l => l.value === selectedLoader.value)?.label ?? t('downloadConfig.loaderNone')
    toast(t('downloadConfig.needLoaderVersion', { label }), 'error')
    return
  }
  emit('confirm', {
    version: props.version,
    displayName: displayName.value.trim() || props.version,
    loader: selectedLoader.value,
    loaderVersion: selectedLoaderVersion.value,
    installFabricApi: selectedLoader.value === 'fabric',
  })
}

onMounted(() => {
  loadLoaderVersions()
})
</script>

<template>
  <div class="config-content">
    <div class="config-header">
      <button class="back-btn" @click="emit('back')">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <polyline points="15 18 9 12 15 6"/>
        </svg>
      </button>
      <span class="config-title">{{ $t('downloadConfig.title') }}</span>
      <div class="header-spacer"></div>
    </div>

    <div class="config-body">
      <div class="config-section">
        <div class="section-title">{{ $t('downloadConfig.sectionVersion') }}</div>
        <div class="config-row">
          <span class="row-label">{{ $t('downloadConfig.rowOriginal') }}</span>
          <span class="row-value">{{ version }}</span>
        </div>
        <div class="config-row">
          <span class="row-label">{{ $t('downloadConfig.rowName') }}</span>
          <input type="text" v-model="displayName" class="row-input" :placeholder="t('downloadConfig.namePlaceholder')" />
        </div>
      </div>

      <div class="config-section">
        <div class="section-title">{{ $t('downloadConfig.sectionLoader') }}</div>
        <div class="loader-list">
          <button
            v-for="loader in loaders"
            :key="loader.value"
            class="loader-item"
            :class="{ active: selectedLoader === loader.value }"
            @click="handleLoaderChange(loader.value as any)"
          >
            <span class="loader-name">{{ loader.label }}</span>
          </button>
        </div>
        <div v-if="selectedLoader === 'none'" class="loader-hint">
          {{ $t('downloadConfig.loaderHint') }}
        </div>
      </div>

      <div v-if="showVersionList" class="config-section">
        <div class="section-header" @click="showVersionList = !showVersionList">
          <div class="section-title">{{ t('downloadConfig.loaderVersions', { label: loaders.find(l => l.value === selectedLoader)?.label ?? t('downloadConfig.loaderNone') }) }}</div>
          <svg class="expand-icon" :class="{ expanded: showVersionList }" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <polyline points="6 9 12 15 18 9"/>
          </svg>
        </div>
        <div v-if="showVersionList" class="version-list-container">
          <div v-if="loading" class="loading-text">{{ $t('downloadConfig.loading') }}</div>
          <div v-else-if="loaderError" class="empty-text error-text">{{ loaderError }}</div>
          <div v-else-if="currentLoaderVersions.length === 0" class="empty-text">{{ $t('downloadConfig.noVersion') }}</div>
          <div v-else class="version-list">
            <button
              v-for="ver in currentLoaderVersions"
              :key="ver"
              class="version-item"
              :class="{ active: selectedLoaderVersion === ver }"
              @click="selectedLoaderVersion = ver"
            >
              <span class="version-name">{{ ver }}</span>
            </button>
          </div>
        </div>
      </div>
    </div>

    <div class="config-footer">
      <button class="action-btn secondary" @click="emit('back')">{{ $t('downloadConfig.back') }}</button>
      <button class="action-btn primary" @click="handleConfirm">{{ $t('downloadConfig.startDownload') }}</button>
    </div>
  </div>
</template>

<style scoped>
.config-content {
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
  padding: 10px 17px;
  box-sizing: border-box;
}

.config-header {
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

.config-title {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 18px;
  font-weight: 600;
  color: var(--text-color);
}

.header-spacer {
  width: 34px;
}

.config-body {
  flex: 1;
  overflow-y: auto;
  padding: 16px 0;
  display: flex;
  flex-direction: column;
  gap: 20px;
}

.config-section {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.section-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 14px;
  background: var(--card-bg);
  border-radius: 8px;
  cursor: pointer;
  transition: background-color 150ms ease;
}

.section-header:hover {
  background: var(--button-bg);
}

.section-title {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 14px;
  font-weight: 600;
  color: var(--text-color);
  opacity: 0.7;
}

.expand-icon {
  width: 18px;
  height: 18px;
  color: var(--text-color);
  opacity: 0.5;
  transition: transform 150ms ease;
}

.expand-icon.expanded {
  transform: rotate(180deg);
}

.version-list-container {
  margin-top: 6px;
}

.config-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 14px;
  background: var(--card-bg);
  border-radius: 8px;
}

.row-label {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 14px;
  color: var(--text-color);
  opacity: 0.7;
}

.row-value {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 14px;
  color: var(--text-color);
}

.row-input {
  flex: 1;
  max-width: 300px;
  padding: 8px 12px;
  border: 1px solid var(--border-color);
  border-radius: 6px;
  background: var(--input-bg);
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 14px;
  color: var(--text-color);
  outline: none;
  text-align: right;
}

.row-input:focus {
  border-color: var(--accent-color);
}

.row-input::placeholder {
  color: var(--label-color);
}

.loader-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.loader-item {
  display: flex;
  align-items: center;
  padding: 12px 14px;
  background: var(--card-bg);
  border-radius: 8px;
  border: 1px solid transparent;
  cursor: pointer;
  transition: all 150ms ease;
}

.loader-item:hover {
  border-color: var(--accent-color);
}

.loader-item.active {
  background: var(--accent-color);
  border-color: var(--accent-color);
}

.loader-name {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 14px;
  color: var(--text-color);
}

.loader-item.active .loader-name {
  color: var(--accent-text-color);
}

.loader-hint {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 12px;
  line-height: 1.5;
  color: #ff9500;
  padding: 8px 12px;
  border: 1px solid rgba(255, 149, 0, 0.4);
  border-radius: 8px;
  background: rgba(255, 149, 0, 0.08);
}

.version-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.version-item {
  display: flex;
  align-items: center;
  padding: 12px 14px;
  background: var(--card-bg);
  border-radius: 8px;
  border: 1px solid transparent;
  cursor: pointer;
  transition: all 150ms ease;
}

.version-item:hover {
  border-color: var(--accent-color);
}

.version-item.active {
  background: var(--accent-color);
  border-color: var(--accent-color);
}

.version-name {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 14px;
  color: var(--text-color);
}

.version-item.active .version-name {
  color: var(--accent-text-color);
}

.loading-text, .empty-text {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 14px;
  color: var(--label-color);
  padding: 12px 0;
}
.empty-text.error-text {
  color: #ff3b30;
}

.checkbox-label {
  display: flex;
  align-items: center;
  gap: 8px;
  cursor: pointer;
}

.checkbox-label input {
  width: 18px;
  height: 18px;
  accent-color: var(--accent-color);
}

.checkbox-text {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 14px;
  color: var(--text-color);
}

.config-footer {
  display: flex;
  gap: 12px;
  padding: 16px 0;
  border-top: 1px solid var(--border-color);
  justify-content: flex-end;
  flex-shrink: 0;
}

.action-btn {
  padding: 10px 20px;
  border-radius: 8px;
  border: none;
  cursor: pointer;
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 14px;
  font-weight: 500;
  transition: background-color 150ms ease;
}

.action-btn:active {
  transform: scale(0.95);
}

.action-btn.primary {
  background: var(--accent-color);
  color: var(--accent-text-color);
}

.action-btn.secondary {
  background: var(--button-bg);
  color: var(--text-color);
}

.config-body::-webkit-scrollbar {
  width: 4px;
}

.config-body::-webkit-scrollbar-track {
  background: transparent;
}

.config-body::-webkit-scrollbar-thumb {
  background: var(--button-bg);
  border-radius: 2px;
}
</style>