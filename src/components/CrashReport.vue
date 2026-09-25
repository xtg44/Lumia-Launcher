<script setup lang="ts">
import { ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { openCrashLogFile, parseCrashReport, type CrashInfo } from '../utils/tauri'

const { t } = useI18n()

const props = defineProps<{
  visible: boolean
}>()

const emit = defineEmits<{
  close: []
}>()

const crashInfo = ref<CrashInfo | null>(null)
const loading = ref(false)

async function handleOpenLogFile() {
  try {
    await openCrashLogFile()
  } catch (err) {
    console.error('打开日志文件失败:', err)
  }
}

async function loadCrashInfo() {
  loading.value = true
  crashInfo.value = null
  try {
    crashInfo.value = await parseCrashReport()
  } catch (err) {
    console.error('解析崩溃报告失败:', err)
  } finally {
    loading.value = false
  }
}

function friendlyName(type: string | null): string {
  if (!type) return ''
  const parts = type.split(':')
  return parts.length > 1 ? parts[1].replace(/_/g, ' ') : type
}

function friendlyLocation(loc: string | null): string {
  if (!loc) return ''
  return loc.replace('World: ', '').replace('(', '').replace(')', '')
}

function crashMessage(crash: CrashInfo): string {
  switch (crash.error_type) {
    case 'entity_ticking':
      return t('crash.typeEntityTicking', {
        name: crash.entity_name || friendlyName(crash.entity_type),
        location: friendlyLocation(crash.entity_location)
      })
    case 'block_ticking':
      return t('crash.typeBlockTicking', {
        type: friendlyName(crash.block_type),
        location: friendlyLocation(crash.block_location)
      })
    case 'out_of_memory':
      return t('crash.typeOutOfMemory')
    case 'missing_native':
      return t('crash.typeMissingNative')
    case 'version_mismatch':
      return t('crash.typeVersionMismatch')
    case 'gpu_error':
      return t('crash.typeGpuError')
    case 'stack_overflow':
      return t('crash.typeStackOverflow')
    case 'null_pointer':
      return t('crash.typeNullPointer')
    case 'ticking':
      return t('crash.typeTicking')
    default:
      return ''
  }
}

watch(() => props.visible, (newVal) => {
  if (newVal) {
    loadCrashInfo()
  }
})
</script>

<template>
  <Transition name="crash">
    <div v-if="visible" class="crash-overlay" @click.self="emit('close')">
      <div class="crash-panel">
        <div class="crash-header">
          <div class="crash-icon">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <circle cx="12" cy="12" r="10"/>
              <line x1="15" y1="9" x2="9" y2="15"/>
              <line x1="9" y1="9" x2="15" y2="15"/>
            </svg>
          </div>
          <span class="crash-title">{{ t('crash.title') }}</span>
          <button class="close-btn" @click="emit('close')">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <line x1="18" y1="6" x2="6" y2="18"/>
              <line x1="6" y1="6" x2="18" y2="18"/>
            </svg>
          </button>
        </div>

        <div class="crash-body">
          <div v-if="loading" class="crash-loading">
            <div class="crash-spinner"></div>
            <span>{{ t('crash.analyzing') }}</span>
          </div>
          <template v-else-if="crashInfo">
            <div v-if="crashMessage(crashInfo)" class="crash-message">{{ crashMessage(crashInfo) }}</div>
            <div v-else class="crash-message">{{ t('crash.message') }}</div>
          </template>
          <div v-else class="crash-message">{{ t('crash.message') }}</div>
        </div>

        <div class="crash-footer">
          <button class="action-btn secondary" @click="handleOpenLogFile">{{ t('crash.openLog') }}</button>
          <button class="action-btn primary" @click="emit('close')">{{ t('crash.ok') }}</button>
        </div>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
.crash-overlay {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  background: rgba(0, 0, 0, 0.8);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
}

.crash-panel {
  width: 700px;
  max-width: 90vw;
  max-height: 80vh;
  background: var(--panel-bg);
  border-radius: 16px;
  border: 1px solid var(--border-color);
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.crash-enter-active {
  transition: opacity 0.25s ease;
}

.crash-enter-active .crash-panel {
  transition: all 0.35s cubic-bezier(0.34, 1.2, 0.64, 1);
}

.crash-enter-from {
  opacity: 0;
}

.crash-enter-from .crash-panel {
  transform: scale(0.9) translateY(20px);
  opacity: 0;
}

.crash-header {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 16px 20px;
  border-bottom: 1px solid var(--border-color);
}

.crash-icon {
  width: 32px;
  height: 32px;
  color: #ff3b30;
  flex-shrink: 0;
}

.crash-icon svg {
  width: 100%;
  height: 100%;
}

.crash-title {
  flex: 1;
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 18px;
  font-weight: 600;
  color: var(--text-color);
}

.close-btn {
  background: none;
  border: none;
  cursor: pointer;
  padding: 6px;
  border-radius: 6px;
  color: var(--text-color);
  transition: background-color 150ms ease;
}

.close-btn:active {
  transform: scale(0.92);
  background-color: var(--button-active);
}

.close-btn svg {
  width: 20px;
  height: 20px;
}

.crash-body {
  padding: 20px;
}

.crash-loading {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  padding: 40px 0;
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 14px;
  color: var(--text-color);
  opacity: 0.7;
}

.crash-spinner {
  width: 32px;
  height: 32px;
  border: 3px solid var(--border-color);
  border-top-color: var(--accent-color);
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
}

@keyframes spin {
  to { transform: rotate(360deg); }
}

.crash-message {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 14px;
  color: var(--text-color);
  line-height: 1.6;
}

.crash-footer {
  display: flex;
  gap: 12px;
  padding: 16px 20px;
  border-top: 1px solid var(--border-color);
  justify-content: flex-end;
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
</style>