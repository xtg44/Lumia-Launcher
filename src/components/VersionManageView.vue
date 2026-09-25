<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from '../utils/toast'
import {
  renameVersion,
  deleteVersion,
  listMods,
  toggleMod,
  deleteMod,
  openModsFolder,
  type ModFileInfo,
} from '../utils/tauri'

const { t } = useI18n()

const props = defineProps<{
  version: string
  loader: string
}>()

const emit = defineEmits<{
  (e: 'back'): void
  (e: 'changed'): void
}>()

/** 原版版本不支持模组：不显示"模组管理"（避免小白困惑） */
const isVanilla = computed(() => props.loader?.toLowerCase() === 'vanilla')

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
const modsError = ref('')
const busyMod = ref('')
/** 待二次确认删除的模组文件名（Tauri WebView 不支持 window.confirm，用两步确认） */
const confirmDeleteMod = ref('')

async function loadMods() {
  modsLoading.value = true
  modsError.value = ''
  try {
    mods.value = await listMods(props.version)
  } catch (err) {
    modsError.value = err instanceof Error ? err.message : String(err)
    mods.value = []
  } finally {
    modsLoading.value = false
  }
}

onMounted(() => {
  newName.value = props.version
  loadMods()
})

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

function formatSize(bytes: number): string {
  if (bytes >= 1048576) return (bytes / 1048576).toFixed(1) + ' MB'
  if (bytes >= 1024) return (bytes / 1024).toFixed(0) + ' KB'
  return bytes + ' B'
}
</script>

<template>
  <div class="manage-content">
    <div class="manage-header">
      <button class="back-btn" @click="emit('back')" :title="$t('versionManage.back')">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <polyline points="15 18 9 12 15 6"/>
        </svg>
      </button>
      <span class="manage-title">{{ $t('versionManage.title') }}</span>
      <div class="header-spacer"></div>
    </div>

    <div class="manage-body">
      <!-- 基本信息 -->
      <div class="version-summary">
        <span class="summary-name">{{ version }}</span>
        <span class="summary-loader">{{ loader }}</span>
      </div>

      <!-- 重命名 -->
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

      <!-- 模组管理（原版版本不显示） -->
      <div v-if="!isVanilla" class="manage-item">
        <div class="manage-label-row">
          <label class="manage-label">{{ $t('versionManage.modsLabel', { count: mods.length }) }}</label>
          <button class="mini-btn" @click="openModsFolder(version)">{{ $t('versionManage.openModsFolder') }}</button>
        </div>
        <div class="mods-list">
          <div v-if="modsLoading" class="mods-empty">{{ $t('versionManage.loading') }}</div>
          <div v-else-if="modsError" class="mods-empty error-text">{{ modsError }}</div>
          <div v-else-if="mods.length === 0" class="mods-empty">
            {{ $t('versionManage.noMods') }}
          </div>
          <div v-for="mod in mods" :key="mod.fileName" class="mod-row" :class="{ disabled: !mod.enabled }">
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

      <!-- 删除版本 -->
      <div class="manage-item danger-zone">
        <label class="manage-label">{{ $t('versionManage.dangerLabel') }}</label>
        <button class="action-btn danger" :disabled="deleting" @click="handleDelete">
          {{ confirmDelete ? $t('versionManage.confirmDeleteAgain') : deleting ? $t('versionManage.deleting') : $t('versionManage.deleteVersion') }}
        </button>
        <span class="danger-hint">{{ $t('versionManage.dangerHint') }}</span>
      </div>
    </div>
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
.manage-title {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 18px;
  font-weight: 600;
  color: var(--text-color);
}
.header-spacer {
  width: 34px;
}

.manage-body {
  flex: 1;
  overflow-y: auto;
  padding: 16px 0;
  display: flex;
  flex-direction: column;
  gap: 18px;
}

.version-summary {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 14px;
  background: var(--card-bg);
  border-radius: 10px;
}
.summary-name {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 16px;
  font-weight: 600;
  color: var(--text-color);
}
.summary-loader {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 11px;
  padding: 2px 10px;
  border-radius: 6px;
  background: var(--button-bg);
  color: var(--text-color);
  opacity: 0.8;
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
.manage-label-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
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

.mods-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
  max-height: 300px;
  overflow-y: auto;
}
.mods-empty {
  padding: 16px;
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

.mods-list::-webkit-scrollbar, .manage-body::-webkit-scrollbar {
  width: 4px;
}
.mods-list::-webkit-scrollbar-thumb, .manage-body::-webkit-scrollbar-thumb {
  background: var(--button-bg);
  border-radius: 2px;
}
</style>
