<script setup lang="ts">
import { ref, computed, onUnmounted } from 'vue'
import {
  checkForUpdates,
  openUrl,
  getConfig,
  prepareSelfUpdate,
  applySelfUpdate,
  listenDownloadFileProgress,
  listenSelfUpdateStage,
  type UpdateCheckInfo,
  type PreparedUpdate,
} from '../utils/tauri'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

/**
 * 弹窗状态机：
 *   available ──点「下载并安装」──▶ working ──就绪──▶ ready ──点「立即重启并更新」──▶ applying
 *                                     └──失败──▶ error ──重试──▶ working
 * 下载与校验过程中随时可以关掉弹窗，下载不会中断；下次打开直接跳到 ready。
 */
type Phase = 'available' | 'working' | 'ready' | 'error' | 'applying'

const visible = ref(false)
const info = ref<UpdateCheckInfo | null>(null)
const phase = ref<Phase>('available')
const stageText = ref('')
const progress = ref(0)
const errorMsg = ref('')
const prepared = ref<PreparedUpdate | null>(null)
/** 下载进行中（跨弹窗开关保留，用来避免重复触发下载） */
const busy = ref(false)
/** 设置项：下载完成后是否直接更新重启（默认关） */
const autoRestart = ref(false)

/** 能走自更新：服务端提供了更新包，且路径闸门通过 */
const canSelfUpdate = computed(
  () => !!info.value?.updatePackage && info.value?.capability?.supported !== false,
)

const primaryText = computed(() => {
  if (!canSelfUpdate.value) return t('app.update.openSite')
  switch (phase.value) {
    case 'working':
      // 下载中按钮只是占位并禁用，别显示成「立即重启并更新」
      return t('app.update.downloading')
    case 'ready':
    case 'applying':
      return t('app.update.installNow')
    case 'error':
      return t('common.retry')
    default:
      return t('app.update.downloadAndInstall')
  }
})

const primaryDisabled = computed(
  () => canSelfUpdate.value && (phase.value === 'working' || phase.value === 'applying'),
)

// ===== 事件监听（下载进度 + 校验/解压阶段）=====
let unlistenProgress: (() => void) | null = null
let unlistenStage: (() => void) | null = null
let listenersReady = false

async function ensureListeners() {
  if (listenersReady) return
  listenersReady = true
  unlistenProgress = await listenDownloadFileProgress((p: any) => {
    if (!busy.value) return
    if (typeof p?.progress === 'number') {
      progress.value = Math.min(100, Math.max(progress.value, Math.round(p.progress)))
    }
    if (p?.stage) stageText.value = String(p.stage)
  })
  unlistenStage = await listenSelfUpdateStage((p) => {
    if (!busy.value) return
    if (p?.message) stageText.value = p.message
  })
}

function releaseListeners() {
  unlistenProgress?.()
  unlistenStage?.()
  unlistenProgress = null
  unlistenStage = null
  listenersReady = false
}

onUnmounted(releaseListeners)

// ===== 启动检查 =====
/** 启动时调用：有新版才弹窗 */
async function check() {
  try {
    const result = await checkForUpdates()
    if (!result.hasUpdate) return
    info.value = result

    // 上一次自动更新没走完（helper 写了 FAIL 日志）→ 直接把原因摆出来
    if (result.lastApplyError) {
      errorMsg.value = result.lastApplyError
      phase.value = 'error'
    } else if (busy.value) {
      // 下载还在进行中，保持当前阶段
    } else if (prepared.value && prepared.value.version === result.latest) {
      phase.value = 'ready'
    } else {
      phase.value = 'available'
      progress.value = 0
      stageText.value = ''
      errorMsg.value = ''
    }
    visible.value = true

    // 自动下载（默认开）：让用户看到进度条，而不是被丢去浏览器
    if (!busy.value && phase.value === 'available') {
      try {
        const config = await getConfig()
        autoRestart.value = config.auto_restart_update === true
        if (config.auto_download_update !== false && canSelfUpdate.value) {
          await startInstall()
        }
      } catch {
        // 读配置失败就不自动下载
      }
    }
  } catch {
    // 网络失败静默，不打扰启动
  }
}

// ===== 下载 → 校验 → 解压 =====
async function startInstall() {
  const pkg = info.value?.updatePackage
  if (!pkg || !canSelfUpdate.value || busy.value) return

  busy.value = true
  phase.value = 'working'
  progress.value = 0
  stageText.value = t('app.update.downloading')
  errorMsg.value = ''
  await ensureListeners()

  try {
    const result = await prepareSelfUpdate(pkg.url, pkg.sha256, info.value!.latest)
    prepared.value = result
    progress.value = 100
    phase.value = 'ready'
    // 用户勾了「下载完成后自动重启」才会自动走下去
    if (autoRestart.value) await applyNow()
  } catch (err) {
    errorMsg.value = String(err)
    phase.value = 'error'
  } finally {
    busy.value = false
  }
}

// ===== 退出并安装 =====
async function applyNow() {
  const version = info.value?.latest
  if (!version) return
  phase.value = 'applying'
  try {
    // 后端派发更新进程后会让应用退出；能走到 catch 说明没派发成功
    await applySelfUpdate(version)
  } catch (err) {
    errorMsg.value = String(err)
    phase.value = 'error'
  }
}

/** 不支持自更新 / 失败兜底：打开官网下载页 */
async function handleOpenSite() {
  const info0 = info.value
  const fallback = info0?.channel === 'stable' ? info0?.stableDownload : info0?.betaDownload
  const target = info0?.updatePackage?.url || info0?.downloadUrl || fallback || 'https://lumialauncher.cn'
  visible.value = false
  try {
    await openUrl(target)
  } catch {
    // 打不开就静默
  }
}

async function handlePrimary() {
  if (!canSelfUpdate.value) return handleOpenSite()
  if (phase.value === 'available' || phase.value === 'error') return startInstall()
  if (phase.value === 'ready') return applyNow()
}

function handleClose() {
  visible.value = false
  // 已下载的包不删除：下次打开弹窗会直接复用（后端按版本号识别）
}

defineExpose({ check })
</script>

<template>
  <Transition name="modal">
    <div v-if="visible" class="modal-overlay" @click.self="handleClose">
      <div class="modal-card">
        <div class="modal-header">
          <span class="modal-title">{{ t('app.update.newVersion', { version: info?.latest }) }}</span>
          <button class="close-btn" @click="handleClose" :aria-label="t('common.close')">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="22" height="22">
              <line x1="18" y1="6" x2="6" y2="18"/>
              <line x1="6" y1="6" x2="18" y2="18"/>
            </svg>
          </button>
        </div>
        <div class="modal-body">
          <p class="update-line">{{ t('app.update.current') }} <strong>{{ info?.current }}</strong></p>
          <p class="update-line latest">{{ t('app.update.latest') }} <strong>{{ info?.latest }}</strong></p>

          <!-- 下载 / 校验 / 解压 -->
          <template v-if="phase === 'working'">
            <div class="update-progress-head">
              <span class="update-hint progress">{{ stageText }}</span>
              <span class="update-percent">{{ progress }}%</span>
            </div>
            <div class="update-progress-bar">
              <div class="update-progress-fill" :style="{ width: progress + '%' }"></div>
            </div>
          </template>

          <!-- 已就绪，等用户点确认 -->
          <p v-else-if="phase === 'ready'" class="update-hint ready">
            <strong>{{ t('app.update.readyTitle') }}</strong><br />
            {{ t('app.update.readyHint') }}
          </p>

          <!-- 已经在退出了 -->
          <p v-else-if="phase === 'applying'" class="update-hint">{{ t('app.update.readyHint') }}</p>

          <!-- 失败 -->
          <p v-else-if="phase === 'error'" class="update-hint error">
            <strong>{{ t('app.update.failedTitle') }}</strong><br />
            {{ errorMsg }}
          </p>

          <!-- 安装方式不支持自动更新（开发模式 / 只读位置 / 直接从 dmg 运行） -->
          <p v-else-if="!canSelfUpdate" class="update-hint">
            {{ t('app.update.unsupported') }}<br />
            <span v-if="info?.capability?.reason" class="update-detail">{{ info.capability.reason }}</span>
          </p>
        </div>
        <div class="modal-footer">
          <button class="ghost-btn" @click="handleClose">{{ t('app.update.later') }}</button>
          <button class="confirm-btn" :disabled="primaryDisabled" @click="handlePrimary">
            {{ primaryText }}
          </button>
        </div>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
.modal-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.45);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 10000;
}

.modal-card {
  background: var(--panel-bg, #1c1c1e);
  border: 1px solid var(--border-color);
  border-radius: 16px;
  width: 460px;
  max-width: 90vw;
  box-shadow: 0 20px 60px rgba(0, 0, 0, 0.3);
  overflow: hidden;
}

.modal-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 18px 20px 8px;
}

.modal-title {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 20px;
  font-weight: 600;
  color: var(--text-color);
}

.close-btn {
  background: none;
  border: none;
  cursor: pointer;
  padding: 4px;
  border-radius: 6px;
  color: var(--text-color);
  opacity: 0.6;
  transition: opacity 0.15s, background-color 0.15s;
}

.close-btn:hover { opacity: 1; }
.close-btn:active { background: var(--button-active); }

.modal-body {
  padding: 10px 20px 16px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.update-line {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 14px;
  color: var(--label-color);
}

.update-line strong {
  font-weight: 600;
  color: var(--text-color);
}

.update-line.latest strong {
  color: var(--accent-color);
}

/* 下载中：阶段文案 + 右侧百分比，进度条在下面。
   百分比不放在条内 —— 条内文字要同时压在两段底色上，浅色主题下根本看不清。 */
.update-progress-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 12px;
}

.update-hint.progress {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.update-percent {
  flex: none;
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 13px;
  font-weight: 600;
  color: var(--text-color);
}

/* 进度条：与设置页工具箱下载器同一套视觉 */
.update-progress-bar {
  position: relative;
  width: 100%;
  height: 18px;
  background: var(--button-bg);
  border-radius: 9px;
  overflow: hidden;
}

.update-progress-fill {
  height: 100%;
  background: var(--accent-color);
  border-radius: 9px;
  transition: width 0.3s ease;
}

.update-hint {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 13px;
  line-height: 1.55;
  color: var(--label-color);
  word-break: break-word;
}

.update-hint strong {
  font-weight: 600;
  color: var(--text-color);
}

.update-hint.ready strong {
  color: var(--accent-color);
}

.update-detail {
  font-size: 12px;
  opacity: 0.75;
}

.modal-footer {
  padding: 8px 20px 18px;
  display: flex;
  justify-content: flex-end;
  gap: 10px;
}

.ghost-btn {
  padding: 8px 20px;
  border-radius: 20px;
  border: 1px solid var(--border-color);
  background: none;
  color: var(--text-color);
  cursor: pointer;
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 14px;
  transition: background-color 0.15s;
}

.ghost-btn:hover { background: var(--button-bg); }

.confirm-btn {
  padding: 8px 24px;
  border-radius: 20px;
  border: none;
  background: var(--accent-color);
  color: var(--accent-text-color);
  cursor: pointer;
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 14px;
  font-weight: 500;
  transition: opacity 0.15s, transform 100ms ease;
}

.confirm-btn:active { transform: scale(0.96); }

/* 禁用态必须在 :active 之后 —— 同权重靠源码顺序决胜 */
.confirm-btn:disabled {
  opacity: 0.55;
  cursor: default;
}

.confirm-btn:disabled:active { transform: none; }

/* Transition */
.modal-enter-active { transition: opacity 0.25s ease; }
.modal-leave-active { transition: opacity 0.2s ease; }
.modal-enter-from, .modal-leave-to { opacity: 0; }
.modal-enter-active .modal-card { transition: transform 0.25s cubic-bezier(0.34, 1.2, 0.64, 1); }
.modal-leave-active .modal-card { transition: transform 0.2s ease; }
.modal-enter-from .modal-card { transform: scale(0.9) translateY(10px); }
.modal-leave-to .modal-card { transform: scale(0.95) translateY(5px); }

/* 减弱动效：收口放在样式表末尾（同权重后写者胜） */
@media (prefers-reduced-motion: reduce) {
  .confirm-btn,
  .confirm-btn:active,
  .modal-enter-active .modal-card,
  .modal-leave-active .modal-card,
  .update-progress-fill { transition: none; transform: none; }
}
</style>
