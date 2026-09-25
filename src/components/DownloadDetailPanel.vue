<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { downloadGame, installModpack, listenDownloadProgress, type DownloadProgressPayload } from '../utils/tauri'

const { t } = useI18n()

interface DownloadTask {
  id: string
  version: string
  displayName: string
  loader: 'none' | 'fabric' | 'forge' | 'neoforge'
  loaderVersion: string
  installFabricApi: boolean
  kind: 'game' | 'modpack' | 'launch'
  packPath?: string
  customVersionName?: string
  progress: number
  speed: string
  remaining: string
  status: 'downloading' | 'paused' | 'queued' | 'completed' | 'error' | 'cancelled'
  error?: string
  stage: string
  totalSize: number
  downloaded: number
  startTime: number
  lastUpdateTime: number
  lastProgress: number
  lastDownloaded: number
}

const tasks = ref<DownloadTask[]>([])
const activeDownloads = ref<Set<string>>(new Set())
defineProps<{
  visible: boolean
}>()

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'download-complete', version: string): void
  (e: 'progress-update', progress: number): void
  (e: 'tasks-update', count: number): void
  (e: 'launch-active', active: boolean): void
}>()

watch(tasks, (newTasks) => {
  const activeCount = newTasks.filter(t => t.status !== 'completed' && t.status !== 'error').length
  emit('tasks-update', activeCount)
  // 是否有进行中的启动任务（用于顶部进度条切换为「启动中」状态）
  const launchActive = newTasks.some(t => t.kind === 'launch' && t.status === 'downloading')
  emit('launch-active', launchActive)
}, { deep: true, immediate: true })

const overallProgress = computed(() => {
  if (tasks.value.length === 0) return 0
  const sum = tasks.value.reduce((acc, t) => acc + t.progress, 0)
  return Math.round(sum / tasks.value.length)
})

// ===== 串行下载队列：同一时刻只允许一个任务真正下载 =====
// 后端 cancel_download 是全局标志、download-progress 是全局广播事件，
// 并发下载会导致取消互相影响、进度互相串台，因此改为队列串行。
function startNextQueued() {
  // 已有活动下载时不得启动新任务（防止 watchdog 与 finally 双重触发）
  if (activeDownloads.value.size > 0) return
  const next = tasks.value.find(t => t.status === 'queued')
  if (next) {
    startDownload(next)
  }
}

function addDownloadTask(config: {
  version: string
  displayName: string
  loader: 'none' | 'fabric' | 'forge' | 'neoforge'
  loaderVersion: string
  installFabricApi: boolean
}) {
  const isDuplicate = tasks.value.some(t => 
    t.displayName === config.displayName &&
    t.status !== 'completed' && 
    t.status !== 'error'
  )
  
  if (isDuplicate) {
    return
  }
  const task: DownloadTask = {
    id: `dl-${Date.now()}-${config.version}`,
    version: config.version,
    displayName: config.displayName,
    loader: config.loader,
    loaderVersion: config.loaderVersion,
    installFabricApi: config.installFabricApi,
    kind: 'game',
    progress: 0,
    speed: '0 MB/s',
    remaining: t('downloadDetail.computing'),
    status: 'queued',
    stage: t('downloadDetail.queuing'),
    totalSize: 0,
    downloaded: 0,
    startTime: 0,
    lastUpdateTime: Date.now(),
    lastProgress: 0,
    lastDownloaded: 0
  }
  tasks.value.unshift(task)
  // 队列串行：仅在无活动下载时立即启动，否则保持排队
  if (activeDownloads.value.size === 0) {
    startDownload(task)
  }
}

// ===== 整合包安装任务（进入下载中心队列） =====
function addModpackTask(config: {
  path: string
  displayName: string
  version: string
  customVersionName?: string
}) {
  const isDuplicate = tasks.value.some(t =>
    t.displayName === config.displayName &&
    t.status !== 'completed' &&
    t.status !== 'error'
  )
  if (isDuplicate) {
    return
  }
  const task: DownloadTask = {
    id: `pack-${Date.now()}-${config.version}`,
    version: config.version,
    displayName: config.displayName,
    loader: 'none',
    loaderVersion: '',
    installFabricApi: false,
    kind: 'modpack',
    packPath: config.path,
    customVersionName: config.customVersionName,
    progress: 0,
    speed: '0 MB/s',
    remaining: t('downloadDetail.computing'),
    status: 'queued',
    stage: t('downloadDetail.queuing'),
    totalSize: 0,
    downloaded: 0,
    startTime: 0,
    lastUpdateTime: Date.now(),
    lastProgress: 0,
    lastDownloaded: 0
  }
  tasks.value.unshift(task)
  if (activeDownloads.value.size === 0) {
    startDownload(task)
  }
}

// ===== 启动任务（把游戏启动进度并入下载中心，详情面板展示当前启动阶段） =====
function addLaunchTask(version: string) {
  // 每次启动只保留一个启动任务，移除旧的
  tasks.value = tasks.value.filter(t => t.kind !== 'launch')
  const task: DownloadTask = {
    id: `launch-${Date.now()}`,
    version,
    displayName: version,
    loader: 'none',
    loaderVersion: '',
    installFabricApi: false,
    kind: 'launch',
    progress: 0,
    speed: '',
    remaining: '',
    status: 'downloading',
    stage: t('home.launching'),
    totalSize: 0,
    downloaded: 0,
    startTime: Date.now(),
    lastUpdateTime: Date.now(),
    lastProgress: 0,
    lastDownloaded: 0
  }
  tasks.value.unshift(task)
}

function updateLaunchStage(stage: string) {
  const task = tasks.value.find(t => t.kind === 'launch' && t.status === 'downloading')
  if (task) task.stage = stage
}

function completeLaunchTask() {
  const task = tasks.value.find(t => t.kind === 'launch' && t.status === 'downloading')
  if (task) {
    task.status = 'completed'
    task.progress = 100
    task.stage = t('downloadDetail.launched')
  }
}

function failLaunchTask(error?: string) {
  const task = tasks.value.find(t => t.kind === 'launch' && t.status === 'downloading')
  if (task) {
    task.status = 'error'
    task.progress = 0
    task.stage = t('downloadDetail.launchError')
    task.error = error || t('downloadDetail.launchError')
  }
}

async function startDownload(task: DownloadTask) {
  if (activeDownloads.value.has(task.id)) return
  activeDownloads.value.add(task.id)
  task.status = 'downloading'
  task.startTime = Date.now()
  task.lastUpdateTime = Date.now()
  task.stage = t('downloadDetail.starting')

  // 启动看门狗：若 20 秒内进度仍为 0，判定为启动失败，避免无限“正在启动下载”
  let skippedByWatchdog = false
  const watchdog = window.setTimeout(() => {
    if (task.progress <= 0 && activeDownloads.value.has(task.id)) {
      console.error('[watchdog] 下载启动超时', task.displayName)
      skippedByWatchdog = true
      task.status = 'error'
      task.stage = t('downloadDetail.startFailed')
      task.error = t('downloadDetail.noProgress')
      activeDownloads.value.delete(task.id)
      emit('progress-update', overallProgress.value)
      // 队列串行：跳过失败任务，启动下一个排队任务
      startNextQueued()
    }
  }, 20000)

  try {
    console.log('开始下载:', task)
    // 组件级全局监听器已统一处理 download-progress（见 onMounted），这里只发起下载
    if (task.kind === 'modpack') {
      await installModpack(task.packPath || '', task.customVersionName || '')
    } else {
      await downloadGame({
        version: task.version,
        displayName: task.displayName,
        loader: task.loader,
        loaderVersion: task.loaderVersion,
        installFabricApi: task.installFabricApi
      })
    }
    
    // 若 watchdog 已判定失败，不再覆盖为完成
    if (skippedByWatchdog || task.status !== 'downloading') return

    task.status = 'completed'
    task.progress = 100
    task.stage = t('downloadDetail.completed')
    task.speed = '0 MB/s'
    task.remaining = t('downloadDetail.done')
    activeDownloads.value.delete(task.id)
    console.log(`[下载日志] ${task.displayName}: 下载完成`)
    emit('download-complete', task.version)
    
  } catch (err: any) {
    console.error('下载失败:', err)
    if (skippedByWatchdog) return
    if (err.toString().includes('取消')) {
      task.status = 'cancelled'
      task.stage = t('downloadDetail.cancelled')
      task.error = t('downloadDetail.cancelByUser')
    } else {
      task.status = 'error'
      task.stage = t('downloadDetail.downloadFailed')
      task.error = err.toString()
    }
    console.log(`[下载日志] ${task.displayName}: 失败 - ${task.error}`)
    activeDownloads.value.delete(task.id)
  } finally {
    window.clearTimeout(watchdog)
    emit('progress-update', overallProgress.value)
    // 队列串行：启动下一个排队任务（watchdog 已跳过时不再重复启动）
    if (!skippedByWatchdog) {
      startNextQueued()
    }
  }
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes.toFixed(0)} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(2)} MB`
}

function updateTaskProgress(taskId: string, payload: DownloadProgressPayload) {
  const task = tasks.value.find(t => t.id === taskId)
  if (!task) return
  if (task.status === 'cancelled') return

  // 下载日志（控制台）：每个进度事件都记录
  console.log(`[下载日志] ${task.displayName}: stage=${payload.stage} progress=${payload.progress} downloaded=${payload.downloaded ?? '-'} total=${payload.total ?? '-'}`)

  const progress = payload.progress ?? 0
  const stage = payload.stage ?? ''
  const now = Date.now()

  // 后端在下载大文件（客户端 jar）时携带真实字节数
  if (typeof payload.total === 'number' && payload.total > 0) {
    task.totalSize = payload.total
    task.downloaded = payload.downloaded ?? task.downloaded
  }

  const progressDelta = progress - task.lastProgress
  const timeDelta = now - task.lastUpdateTime
  const downloadedDelta = task.downloaded - task.lastDownloaded

  task.progress = Math.min(progress, 100)
  task.stage = stage
  task.lastProgress = progress
  task.lastUpdateTime = now

  // 暂停时仅更新进度，不计算速度
  const isPaused = task.status === 'paused'

  // 优先用真实字节差计算速度（最准确）
  if (!isPaused && timeDelta > 0 && downloadedDelta >= 0 && task.totalSize > 0 && downloadedDelta > 0) {
    const bytesPerMs = downloadedDelta / timeDelta
    const bytesPerSec = bytesPerMs * 1000
    task.speed = `${formatBytes(bytesPerSec)}/s`
    const remainingBytes = Math.max(task.totalSize - task.downloaded, 0)
    const remainingSec = remainingBytes / bytesPerSec
    if (remainingSec < 60) {
      task.remaining = t('downloadDetail.seconds', { n: remainingSec.toFixed(0) })
    } else if (remainingSec < 3600) {
      task.remaining = t('downloadDetail.minutes', { n: (remainingSec / 60).toFixed(1) })
    } else {
      task.remaining = t('downloadDetail.hours', { n: (remainingSec / 3600).toFixed(1) })
    }
  }
  // 无字节信息时，退化为按百分比估算
  else if (!isPaused && timeDelta > 0 && progressDelta > 0 && task.totalSize > 0) {
    const downloadedDeltaEst = (task.totalSize * progressDelta) / 100
    const bytesPerMs = downloadedDeltaEst / timeDelta
    const bytesPerSec = bytesPerMs * 1000
    if (bytesPerSec > 0) {
      task.speed = `${formatBytes(bytesPerSec)}/s`
      const remainingBytes = task.totalSize * (100 - task.progress) / 100
      const remainingSec = remainingBytes / bytesPerSec
      if (remainingSec < 60) {
        task.remaining = t('downloadDetail.seconds', { n: remainingSec.toFixed(0) })
      } else if (remainingSec < 3600) {
        task.remaining = t('downloadDetail.minutes', { n: (remainingSec / 60).toFixed(1) })
      } else {
        task.remaining = t('downloadDetail.hours', { n: (remainingSec / 3600).toFixed(1) })
      }
    }
  }

  task.lastDownloaded = task.downloaded

  // 后端发出 100% 完成事件时，立即标记完成（不用等 invoke 返回）
  if (task.progress >= 100 && task.status === 'downloading') {
    task.status = 'completed'
    task.stage = t('downloadDetail.completed')
    task.speed = '0 MB/s'
    task.remaining = t('downloadDetail.done')
    activeDownloads.value.delete(task.id)
    emit('download-complete', task.version)
  }

  emit('progress-update', overallProgress.value)
}

const handlePause = (id: string) => {
  const task = tasks.value.find(t => t.id === id)
  if (!task) return
  if (task.status === 'downloading') {
    task.status = 'paused'
    task.speed = '0 MB/s'
  } else if (task.status === 'paused') {
    task.status = 'downloading'
  }
  emit('progress-update', overallProgress.value)
}

const handleCancel = async (id: string) => {
  const task = tasks.value.find(t => t.id === id)
  if (!task) return
  
  if (task.status === 'downloading') {
    try {
      const { cancelDownload } = await import('../utils/tauri')
      await cancelDownload()
      task.status = 'cancelled'
      task.stage = t('downloadDetail.cancelling')
      // 后端返回 Err 后 startDownload 的 catch/finally 会接手清理并启动下一个
    } catch (err) {
      console.error('取消下载失败:', err)
    }
  } else if (task.status === 'queued') {
    // 队列中的任务直接移除，不占用下载
    tasks.value = tasks.value.filter(t => t.id !== id)
    emit('progress-update', overallProgress.value)
  } else {
    tasks.value = tasks.value.filter(t => t.id !== id)
    emit('progress-update', overallProgress.value)
  }
}

const handleRetry = (id: string) => {
  const task = tasks.value.find(t => t.id === id)
  if (!task) return
  task.status = 'queued'
  task.progress = 0
  task.error = undefined
  task.speed = '0 MB/s'
  task.remaining = t('downloadDetail.computing')
  task.stage = t('downloadDetail.queuing')
  task.lastProgress = 0
  task.lastDownloaded = 0
  task.downloaded = 0
  task.totalSize = 0
  // 串行队列：无活动下载则立即启动，否则排队
  if (activeDownloads.value.size === 0) {
    startDownload(task)
  }
  emit('progress-update', overallProgress.value)
}

const statusText = (task: DownloadTask) => {
  if (task.kind === 'launch') {
    switch (task.status) {
      case 'downloading': return t('downloadDetail.launching')
      case 'completed': return t('downloadDetail.launched')
      case 'error': return t('downloadDetail.launchError')
      default: return ''
    }
  }
  switch (task.status) {
    case 'downloading': return t('downloadDetail.statusDownloading')
    case 'paused': return t('downloadDetail.statusPaused')
    case 'queued': return t('downloadDetail.statusQueued')
    case 'completed': return t('downloadDetail.statusCompleted')
    case 'error': return t('downloadDetail.statusError')
    case 'cancelled': return t('downloadDetail.statusCancelled')
  }
}

const DRAG_DISMISS_THRESHOLD = 80
const isDragging = ref(false)
const isDismissing = ref(false)
const dragOffset = ref(0)
let startY = 0

const startDismiss = () => {
  if (isDismissing.value) return
  isDismissing.value = true
  window.setTimeout(() => {
    isDismissing.value = false
    dragOffset.value = 0
    emit('close')
  }, 300)
}

const panelStyle = computed(() => {
  if (isDismissing.value) {
    return {
      transform: 'translateY(100%)',
      transition: 'transform 0.3s cubic-bezier(0.4, 0, 1, 1)'
    }
  }
  if (isDragging.value) {
    return {
      transform: `translateY(${dragOffset.value}px)`,
      transition: 'none'
    }
  }
  return {}
})

const handleDragStart = (e: MouseEvent | TouchEvent) => {
  if (isDismissing.value) return
  isDragging.value = true
  startY = 'touches' in e ? e.touches[0].clientY : e.clientY
  document.addEventListener('mousemove', handleDragMove)
  document.addEventListener('mouseup', handleDragEnd)
  document.addEventListener('touchmove', handleDragMove, { passive: false })
  document.addEventListener('touchend', handleDragEnd)
}

const handleDragMove = (e: MouseEvent | TouchEvent) => {
  if (!isDragging.value) return
  if ('touches' in e) e.preventDefault()
  const currentY = 'touches' in e ? e.touches[0].clientY : e.clientY
  const raw = currentY - startY
  dragOffset.value = raw <= 0 ? 0 : raw > 100 ? 100 + (raw - 100) * 0.5 : raw
}

const handleDragEnd = () => {
  isDragging.value = false
  document.removeEventListener('mousemove', handleDragMove)
  document.removeEventListener('mouseup', handleDragEnd)
  document.removeEventListener('touchmove', handleDragMove)
  document.removeEventListener('touchend', handleDragEnd)
  if (dragOffset.value > DRAG_DISMISS_THRESHOLD) {
    startDismiss()
  } else {
    dragOffset.value = 0
  }
}

onUnmounted(() => {
  document.removeEventListener('mousemove', handleDragMove)
  document.removeEventListener('mouseup', handleDragEnd)
  document.removeEventListener('touchmove', handleDragMove)
  document.removeEventListener('touchend', handleDragEnd)
  unlistenProgress?.()
})

// ===== 组件级全局进度监听：挂载时注册一次，事件按当前下载中的任务路由 =====
// 串行队列保证同一时刻只有一个任务在真正下载，因此事件统一转发给当前 active 任务。
let unlistenProgress: (() => void) | null = null

onMounted(async () => {
  try {
    unlistenProgress = await listenDownloadProgress((payload: any) => {
      const activeTask = tasks.value.find(t => t.status === 'downloading' && activeDownloads.value.has(t.id))
      if (!activeTask) return
      updateTaskProgress(activeTask.id, payload as DownloadProgressPayload)
    })
  } catch (e) {
    console.error('[DownloadDetailPanel] 注册全局进度监听失败:', e)
  }
})

defineExpose({
  addDownloadTask,
  addModpackTask,
  addLaunchTask,
  updateLaunchStage,
  completeLaunchTask,
  failLaunchTask
})
</script>

<template>
  <div v-if="visible" class="detail-overlay" @click.self="startDismiss">
    <div class="detail-panel" :style="panelStyle" role="dialog" aria-modal="true">
      <div
        class="panel-grabber"
        @mousedown="handleDragStart"
        @touchstart="handleDragStart"
      >
        <div class="grabber-handle"></div>
      </div>

      <div class="panel-header">
        <div class="header-left">
          <span class="panel-title">{{ $t('downloadDetail.title') }}</span>
          <span class="panel-subtitle">{{ $t('downloadDetail.subtitle', { p: overallProgress, n: tasks.length }) }}</span>
        </div>
        <button class="close-detail-btn" @click="startDismiss" :aria-label="$t('downloadDetail.ariaClose')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
            <line x1="18" y1="6" x2="6" y2="18"/>
            <line x1="6" y1="6" x2="18" y2="18"/>
          </svg>
        </button>
      </div>

      <div class="panel-body">
        <div v-if="tasks.length === 0" class="empty-state">
          <span class="empty-text">{{ $t('downloadDetail.empty') }}</span>
          <span class="empty-hint">{{ $t('downloadDetail.emptyHint') }}</span>
        </div>

        <div v-else class="task-list">
          <div v-for="task in tasks" :key="task.id" class="task-card" :class="{ 
            'task-completed': task.status === 'completed',
            'task-error': task.status === 'error'
          }">
            <div class="task-header">
              <div class="task-info">
                <span class="task-name">{{ task.kind === 'launch' ? $t('downloadDetail.launchTaskName', { v: task.version }) : task.version }}</span>
                <span class="task-stage">{{ task.stage }}</span>
              </div>
              <span class="task-status" :class="task.status">{{ statusText(task) }}</span>
            </div>

            <div class="task-progress">
              <div class="task-progress-bar">
                <div v-if="task.kind === 'launch' && task.status === 'downloading'"
                     class="task-progress-fill indeterminate"></div>
                <div v-else class="task-progress-fill" :style="{ width: `${task.progress}%` }"></div>
              </div>
              <span v-if="task.kind !== 'launch' || task.status !== 'downloading'" class="task-percent">{{ task.progress }}%</span>
            </div>

            <div class="task-meta">
              <template v-if="task.kind !== 'launch'">
                <span class="meta-item">{{ $t('downloadDetail.speed', { s: task.speed }) }}</span>
                <span class="meta-item">{{ $t('downloadDetail.remaining', { r: task.remaining }) }}</span>
              </template>
              <span v-if="task.error" class="meta-item error-text">{{ task.error }}</span>
            </div>

            <div class="task-actions">
              <template v-if="task.kind === 'launch'">
                <button v-if="task.status !== 'downloading'" class="task-btn" @click="handleCancel(task.id)">{{ $t('downloadDetail.remove') }}</button>
              </template>
              <template v-else>
                <template v-if="task.status === 'completed'">
                  <button class="task-btn success" @click="handleCancel(task.id)">{{ $t('downloadDetail.remove') }}</button>
                </template>
                <template v-else-if="task.status === 'error'">
                  <button class="task-btn" @click="handleRetry(task.id)">{{ $t('downloadDetail.retry') }}</button>
                  <button class="task-btn danger" @click="handleCancel(task.id)">{{ $t('downloadDetail.cancel') }}</button>
                </template>
                <template v-else>
                  <button class="task-btn" @click="handlePause(task.id)">
                    {{ task.status === 'paused' ? $t('downloadDetail.resume') : $t('downloadDetail.pause') }}
                  </button>
                  <button class="task-btn danger" @click="handleCancel(task.id)">{{ $t('downloadDetail.cancel') }}</button>
                </template>
              </template>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.detail-overlay {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: flex-end;
  z-index: 100;
  pointer-events: auto;
  overflow: hidden;
}
.detail-panel {
  width: 100%;
  height: 65%;
  background: var(--panel-bg, #1a1a2e);
  border-top-left-radius: 20px;
  border-top-right-radius: 20px;
  border: 1px solid var(--border-color, #2d2d44);
  border-bottom: none;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  box-shadow: 0 -4px 20px rgba(0, 0, 0, 0.3);
  will-change: transform;
  pointer-events: auto;
  animation: sheet-enter 0.4s cubic-bezier(0.34, 1.2, 0.64, 1);
}
.detail-panel.dragging,
.detail-panel.dismissing {
  animation: none;
}
@keyframes sheet-enter {
  from { transform: translateY(100%); }
  to { transform: translateY(0); }
}
.panel-grabber {
  width: 100%;
  height: 18px;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: grab;
  flex-shrink: 0;
  touch-action: none;
  user-select: none;
}
.panel-grabber:active {
  cursor: grabbing;
}
.grabber-handle {
  width: 36px;
  height: 5px;
  background: var(--text-color, #ffffff);
  opacity: 0.25;
  border-radius: 2.5px;
  transition: opacity 0.2s ease;
  pointer-events: none;
}
.panel-grabber:hover .grabber-handle {
  opacity: 0.4;
}
.panel-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 4px 20px 12px;
  border-bottom: 1px solid var(--border-color, #2d2d44);
  flex-shrink: 0;
}
.header-left {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.panel-title {
  font-family: Inter, 'PingFang SC', sans-serif;
  font-size: 18px;
  font-weight: 600;
  color: var(--text-color, #ffffff);
  line-height: 22px;
}
.panel-subtitle {
  font-family: Inter, 'PingFang SC', sans-serif;
  font-size: 12px;
  color: var(--text-color, #ffffff);
  opacity: 0.65;
  line-height: 14px;
}
.close-detail-btn {
  background: var(--button-bg, #2d2d44);
  border: none;
  cursor: pointer;
  padding: 6px;
  border-radius: 8px;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: background-color 0.15s ease, transform 100ms ease;
}
.close-detail-btn:hover {
  background: var(--button-active, #3d3d5c);
}
.close-detail-btn:active {
  transform: scale(0.92);
}
.close-detail-btn svg {
  width: 16px;
  height: 16px;
  color: var(--text-color, #ffffff);
}
.panel-body {
  flex: 1;
  overflow-y: auto;
  padding: 12px 16px 16px;
}
.empty-state {
  height: 100%;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
}
.empty-text {
  font-family: Inter, 'PingFang SC', sans-serif;
  font-size: 16px;
  color: var(--text-color, #ffffff);
  opacity: 0.7;
}
.empty-hint {
  font-family: Inter, 'PingFang SC', sans-serif;
  font-size: 13px;
  color: var(--text-color, #ffffff);
  opacity: 0.4;
}
.task-list {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.task-card {
  background: var(--card-bg, #16213e);
  border-radius: 12px;
  padding: 12px 14px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.task-card.task-completed {
  opacity: 0.7;
}
.task-card.task-error {
  border-left: 3px solid #ff3b30;
}
.task-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}
.task-info {
  display: flex;
  align-items: baseline;
  gap: 8px;
  flex-wrap: wrap;
}
.task-name {
  font-family: Inter, 'PingFang SC', sans-serif;
  font-size: 14px;
  font-weight: 500;
  color: var(--text-color, #ffffff);
}
.task-stage {
  font-family: Inter, 'PingFang SC', sans-serif;
  font-size: 11px;
  color: var(--text-color, #ffffff);
  opacity: 0.5;
}
.task-status {
  font-family: Inter, 'PingFang SC', sans-serif;
  font-size: 11px;
  padding: 2px 10px;
  border-radius: 6px;
  background: var(--button-bg, #2d2d44);
  color: var(--text-color, #ffffff);
  flex-shrink: 0;
}
.task-status.downloading {
  color: #ffffff;
  background: #e94560;
}
.task-status.paused {
  opacity: 0.5;
}
.task-status.completed {
  color: #ffffff;
  background: #2ecc71;
}
.task-status.error {
  color: #ffffff;
  background: #ff3b30;
}
.task-progress {
  display: flex;
  align-items: center;
  gap: 10px;
}
.task-progress-bar {
  flex: 1;
  height: 6px;
  background: var(--button-bg, #2d2d44);
  border-radius: 3px;
  overflow: hidden;
}
.task-progress-fill {
  height: 100%;
  background: linear-gradient(90deg, #e94560, #ff6b81);
  border-radius: 3px;
  transition: width 0.3s ease;
}
.task-card.task-completed .task-progress-fill {
  background: linear-gradient(90deg, #2ecc71, #82e0aa);
}
.task-progress-fill.indeterminate {
  width: 40%;
  background: linear-gradient(90deg, #6fa8ff, #9cc3ff);
  animation: task-indeterminate 1.1s ease-in-out infinite;
}
@keyframes task-indeterminate {
  0% { transform: translateX(-100%); }
  100% { transform: translateX(250%); }
}
.task-percent {
  font-family: Inter, 'PingFang SC', sans-serif;
  font-size: 12px;
  color: var(--text-color, #ffffff);
  min-width: 36px;
  text-align: right;
  font-variant-numeric: tabular-nums;
}
.task-meta {
  display: flex;
  gap: 16px;
  flex-wrap: wrap;
}
.meta-item {
  font-family: Inter, 'PingFang SC', sans-serif;
  font-size: 11px;
  color: var(--text-color, #ffffff);
  opacity: 0.65;
  font-variant-numeric: tabular-nums;
}
.meta-item.error-text {
  color: #ff3b30;
  opacity: 1;
}
.task-actions {
  display: flex;
  gap: 8px;
  justify-content: flex-end;
}
.task-btn {
  padding: 4px 14px;
  border: none;
  border-radius: 6px;
  background: var(--button-bg, #2d2d44);
  font-family: Inter, 'PingFang SC', sans-serif;
  font-size: 12px;
  color: var(--text-color, #ffffff);
  cursor: pointer;
  transition: background-color 0.15s ease, transform 100ms ease;
}
.task-btn:hover {
  background: var(--button-active, #3d3d5c);
}
.task-btn:active {
  transform: scale(0.96);
}
.task-btn.danger {
  color: #ff3b30;
}
.task-btn.danger:hover {
  background: rgba(255, 59, 48, 0.15);
}
.task-btn.success {
  color: #2ecc71;
}
.task-btn.success:hover {
  background: rgba(46, 204, 113, 0.15);
}
</style>
