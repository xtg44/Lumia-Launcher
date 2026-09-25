<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from '../utils/toast'
import { getLocalVersions, getConfig, launchGame, getIconUrl, listenLaunchStatus } from '../utils/tauri'
import CrashReport from './CrashReport.vue'
import PluginInject from './PluginInject.vue'
import VersionManageView from './VersionManageView.vue'

const { t } = useI18n()

const emit = defineEmits<{
  (e: 'select', versionName: string): void
}>()

interface VersionItem {
  id: string
  name: string
  version: string
  loader: string
  loaderType: 'vanilla' | 'fabric' | 'forge' | 'neoforge' | 'quilt'
  iconUrl: string
}

const loaderIcons: Record<string, string> = {
  vanilla: 'release.png',
  fabric: 'fabric.png',
  forge: 'forge.png',
  neoforge: 'neoforge.png',
  quilt: 'quilt.png'
}

const loaderLabels: Record<string, string> = {
  vanilla: 'Vanilla',
  fabric: 'Fabric',
  forge: 'Forge',
  neoforge: 'NeoForge',
  quilt: 'Quilt'
}

const versions = ref<VersionItem[]>([])
const loading = ref(false)
const launching = ref(false)
const showCrashReport = ref(false)
const managingVersion = ref<{ name: string; loader: string } | null>(null)
let unlistenLaunchStatus: (() => void) | null = null

function openManage(item: VersionItem) {
  managingVersion.value = { name: item.name, loader: item.loader }
}

function handleManageChanged() {
  managingVersion.value = null
  loadVersions()
}

async function loadVersions() {
  try {
    loading.value = true
    const list = await getLocalVersions()
    
    const parsedVersions = await Promise.all(list.map(async (info) => {
      const dirName = info.name
      const loaderType: VersionItem['loaderType'] = info.loader
      const loaderLabel = loaderType === 'vanilla' ? 'Vanilla' : `${loaderLabels[loaderType]}`
      const iconName = loaderIcons[loaderType] || 'default.png'
      let iconUrl: string
      try {
        iconUrl = await getIconUrl(iconName)
      } catch {
        iconUrl = await getIconUrl('default.png')
      }
      
      return {
        id: dirName,
        name: dirName,
        version: dirName,
        loader: loaderLabel,
        loaderType,
        iconUrl
      }
    }))
    
    versions.value = parsedVersions
  } catch (err) {
    console.error('加载本地版本失败:', err)
  } finally {
    loading.value = false
  }
}

defineExpose({
  loadVersions
})

async function handleLaunch(version: string) {
  if (launching.value) {
    toast(t('versionSelector.launchingAlert'))
    return
  }
  try {
    launching.value = true
    console.log('开始启动游戏，版本:', version)
    const config = await getConfig()
    console.log('配置:', config)
    if (!config || !config.username) {
      toast(t('versionSelector.setNameAlert'), 'error')
      launching.value = false
      return
    }
    console.log('调用 launchGame...')
    await launchGame({
      version,
      username: config.username
    })
    console.log('launchGame 返回成功')
    toast(t('versionSelector.launchSuccess'), 'success')
  } catch (err: any) {
    console.error('启动失败:', err)
    toast(t('versionSelector.launchFailed', { msg: err?.toString?.() || String(err) }), 'error')
  } finally {
    launching.value = false
  }
}

onMounted(async () => {
  loadVersions()
  
  unlistenLaunchStatus = await listenLaunchStatus((data: any) => {
    if (data.stage === '游戏已启动') {
      launching.value = false
    }
    if (data.stage === '游戏异常退出' || (data.stage === '游戏已退出' && data.exit_code !== 0)) {
      showCrashReport.value = true
      launching.value = false
    }
    if (data.stage === '正在启动 Minecraft...') {
      launching.value = true
    }
  })
})

onUnmounted(() => {
  if (unlistenLaunchStatus) {
    unlistenLaunchStatus()
  }
})
</script>

<template>
  <VersionManageView
    v-if="managingVersion"
    :version="managingVersion.name"
    :loader="managingVersion.loader"
    @back="managingVersion = null"
    @changed="handleManageChanged"
  />

  <div v-else class="version-content">
    <div class="content-header">
      <span class="content-title">{{ t('versionSelector.title') }}</span>
    </div>

    <div class="content-divider"></div>

    <div v-if="loading" class="loading-state">{{ t('versionSelector.loading') }}</div>
    <div v-else-if="versions.length === 0" class="empty-state">{{ t('versionSelector.empty') }}</div>
    <div v-else class="versions-grid">
      <div
        v-for="item in versions"
        :key="item.id"
        class="version-card"
        @click="emit('select', item.name)"
      >
        <img :src="item.iconUrl" :alt="item.name" class="version-icon" />
        <div class="version-info">
          <span class="version-name">{{ item.name }}</span>
          <span class="version-detail">{{ item.version }}</span>
          <span class="version-detail">{{ item.loader }}</span>
        </div>
        <button
          class="manage-btn"
          :disabled="launching"
          @click.stop="openManage(item)"
          :title="t('versionSelector.manage')"
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <circle cx="12" cy="12" r="3"/>
            <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z"/>
          </svg>
        </button>
        <button 
          class="settings-btn" 
          :disabled="launching"
          @click.stop="handleLaunch(item.id)" 
          :title="launching ? t('versionSelector.launching') : t('versionSelector.launch')"
        >
          <svg v-if="!launching" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <polygon points="5 3 19 12 5 21 5 3"/>
          </svg>
          <span v-else>{{ t('versionSelector.launching') }}</span>
        </button>
      </div>
    </div>

    <CrashReport :visible="showCrashReport" @close="showCrashReport = false" />
    <PluginInject page="version" />
  </div>
</template>

<style scoped>
.version-content {
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
}
.content-header {
  display: flex;
  justify-content: flex-start;
  align-items: center;
  padding: 0 0 0 17px;
  height: 48px;
}
.content-title {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 24px;
  color: var(--text-color);
  line-height: 29px;
}
.content-divider {
  background: var(--border-color);
  width: 100%;
  height: 1px;
  margin-top: 0;
}
.loading-state, .empty-state {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 14px;
  color: var(--text-color);
  opacity: 0.5;
}
.versions-grid {
  flex: 1;
  overflow-y: auto;
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  align-content: start;
  gap: 20px 40px;
  padding: 20px;
  max-width: 900px;
  width: 100%;
  margin: 0 auto;
  box-sizing: border-box;
}
.version-card {
  display: flex;
  align-items: center;
  gap: 15px;
  padding: 2px 14px 4px 9px;
  background: var(--card-bg);
  border-radius: 10px;
  min-width: 0;
  cursor: pointer;
  transition: background-color 150ms ease;
}
.version-card:hover {
  background: var(--button-bg);
}
.version-icon {
  width: 52px;
  height: 56px;
  object-fit: contain;
}
.version-info {
  display: flex;
  flex-direction: column;
  gap: 0;
  flex: 1;
  min-width: 0;
}
.version-name {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 24px;
  color: var(--text-color);
}
.version-detail {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 10px;
  color: var(--label-color);
  line-height: 12px;
}
.settings-btn {
  background: none;
  border: none;
  cursor: pointer;
  padding: 8px;
  border-radius: 8px;
  margin-top: 17px;
  transition: background-color 150ms ease;
}
.manage-btn {
  background: none;
  border: none;
  cursor: pointer;
  padding: 8px;
  border-radius: 8px;
  margin-left: auto;
  margin-top: 17px;
  transition: background-color 150ms ease;
  color: var(--text-color);
}
.manage-btn:hover {
  background-color: var(--button-active);
}
.manage-btn:active {
  transform: scale(0.92);
}
.manage-btn svg {
  width: 22px;
  height: 22px;
  color: var(--text-color);
  opacity: 0.8;
}
.settings-btn:active {
  transform: scale(0.92);
  background-color: var(--button-active);
}
.settings-btn svg {
  width: 30px;
  height: 30px;
  color: var(--text-color);
}
</style>