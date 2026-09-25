<script setup lang="ts">
import { ref } from 'vue'
import { checkForUpdates, openUrl, type UpdateCheckInfo } from '../utils/tauri'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

const visible = ref(false)
const info = ref<UpdateCheckInfo | null>(null)

/** 启动时调用：有新版才弹窗 */
async function check() {
  try {
    const result = await checkForUpdates()
    if (result.hasUpdate) {
      info.value = result
      visible.value = true
    }
  } catch {
    // 网络失败静默，不打扰启动
  }
}

function handleClose() {
  visible.value = false
}

async function handleDownload() {
  // 优先使用后端按当前平台解析好的地址（winDownload / macDownload → 渠道字段回退）；
  // 老后端或字段缺失时回退到按渠道选择，最后兜底官网首页。
  const info0 = info.value
  const fallback = info0?.channel === 'stable' ? info0?.stableDownload : info0?.betaDownload
  const target = info0?.downloadUrl || fallback || 'https://lumialauncher.cn'
  visible.value = false
  try {
    await openUrl(target)
  } catch {
    // 打不开就静默
  }
}

defineExpose({ check })
</script>

<template>
  <Transition name="modal">
    <div v-if="visible" class="modal-overlay" @click.self="handleClose">
      <div class="modal-card">
        <div class="modal-header">
          <span class="modal-title">{{ t('update.newVersion', { version: info?.latest }) }}</span>
          <button class="close-btn" @click="handleClose" :aria-label="t('common.close')">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="22" height="22">
              <line x1="18" y1="6" x2="6" y2="18"/>
              <line x1="6" y1="6" x2="18" y2="18"/>
            </svg>
          </button>
        </div>
        <div class="modal-body">
          <p class="update-line">{{ t('update.current') }} <strong>{{ info?.current }}</strong></p>
          <p class="update-line latest">{{ t('update.latest') }} <strong>{{ info?.latest }}</strong></p>
        </div>
        <div class="modal-footer">
          <button class="ghost-btn" @click="handleClose">{{ t('update.later') }}</button>
          <button class="confirm-btn" @click="handleDownload">{{ t('update.download') }}</button>
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

/* Transition */
.modal-enter-active { transition: opacity 0.25s ease; }
.modal-leave-active { transition: opacity 0.2s ease; }
.modal-enter-from, .modal-leave-to { opacity: 0; }
.modal-enter-active .modal-card { transition: transform 0.25s cubic-bezier(0.34, 1.2, 0.64, 1); }
.modal-leave-active .modal-card { transition: transform 0.2s ease; }
.modal-enter-from .modal-card { transform: scale(0.9) translateY(10px); }
.modal-leave-to .modal-card { transform: scale(0.95) translateY(5px); }
</style>