<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { inspectModpack, getLocalVersions, type ModpackInfo } from '../utils/tauri'

const { t } = useI18n()

const props = defineProps<{
  visible: boolean
  packPath: string
}>()

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'install-request', payload: { path: string; name: string; mcVersion: string; customVersionName: string }): void
}>()

const step = ref<'inspecting' | 'info' | 'error'>('inspecting')
const info = ref<ModpackInfo | null>(null)
const error = ref('')

// ===== 同名版本检测 =====
const localVersions = ref<string[]>([])
const customName = ref('')

const hasCollision = computed(() =>
  !!info.value && localVersions.value.includes(info.value.mcVersion)
)

const customNameInvalid = computed(() => {
  const name = customName.value.trim()
  return !name || localVersions.value.includes(name)
})

async function loadLocalVersions() {
  try {
    localVersions.value = (await getLocalVersions()).map(v => v.name)
  } catch {
    localVersions.value = []
  }
}

const FORMAT_LABELS: Record<string, string> = {
  modrinth: 'Modrinth',
  curseforge: 'CurseForge',
}

const LOADER_LABELS: Record<string, string> = {
  fabric: 'Fabric',
  forge: 'Forge',
  neoforge: 'NeoForge',
  quilt: 'Quilt',
  '': '',
}

watch(() => props.visible, (v) => {
  if (v) {
    step.value = 'inspecting'
    error.value = ''
    customName.value = ''
    inspect()
  }
})

async function inspect() {
  try {
    info.value = await inspectModpack(props.packPath)
    await loadLocalVersions()
    // 同名冲突时默认建议：整合包名作为新版本名
    if (hasCollision.value && info.value) {
      customName.value = info.value.name
    }
    step.value = 'info'
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err)
    step.value = 'error'
  }
}

function requestInstall() {
  if (!info.value) return
  if (hasCollision.value && customNameInvalid.value) {
    error.value = t('modpack.collisionAlert')
    return
  }
  emit('install-request', {
    path: props.packPath,
    name: info.value.name,
    mcVersion: info.value.mcVersion,
    customVersionName: hasCollision.value ? customName.value.trim() : '',
  })
  emit('close')
}

function finish() {
  emit('close')
}
</script>

<template>
  <Transition name="modal">
    <div v-if="visible" class="modal-overlay" @click.self="finish">
      <div class="modal-panel">
        <div class="modal-header">
          <span class="modal-title">{{ t('modpack.title') }}</span>
          <button class="close-btn" @click="finish">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <line x1="18" y1="6" x2="6" y2="18"/>
              <line x1="6" y1="6" x2="18" y2="18"/>
            </svg>
          </button>
        </div>

        <div class="modal-body">
          <div v-if="step === 'inspecting'" class="center-state">
            <span class="state-text">{{ t('modpack.inspecting') }}</span>
          </div>

          <div v-else-if="step === 'error'" class="center-state">
            <span class="state-text error-text">{{ error }}</span>
            <button class="action-btn" @click="finish">{{ t('modpack.close') }}</button>
          </div>

          <div v-else-if="step === 'info' && info" class="pack-info">
            <div class="info-row">
              <span class="info-label">{{ t('modpack.rowType') }}</span>
              <span class="format-badge" :class="info.format">{{ FORMAT_LABELS[info.format] }}</span>
            </div>
            <div class="info-row">
              <span class="info-label">{{ t('modpack.rowName') }}</span>
              <span class="info-value">{{ info.name }}</span>
            </div>
            <div class="info-row">
              <span class="info-label">{{ t('modpack.rowMc') }}</span>
              <span class="info-value">{{ info.mcVersion || t('modpack.undeclared') }}</span>
            </div>
            <div class="info-row">
              <span class="info-label">{{ t('modpack.rowLoader') }}</span>
              <span class="info-value">{{ info.loader ? (LOADER_LABELS[info.loader] || info.loader) : t('modpack.loaderVanilla') }}{{ info.loaderVersion ? ' ' + info.loaderVersion : '' }}</span>
            </div>
            <div class="info-row">
              <span class="info-label">{{ t('modpack.rowMods') }}</span>
              <span class="info-value">{{ info.fileCount }}</span>
            </div>

            <!-- 同名版本冲突：必须填写不重复的版本名 -->
            <div v-if="hasCollision" class="collision-box">
              <div class="collision-warning">{{ t('modpack.collisionWarning', { v: info.mcVersion }) }}</div>
              <input
                type="text"
                v-model="customName"
                class="collision-input"
                :placeholder="t('modpack.collisionPlaceholder')"
              />
              <div v-if="customNameInvalid" class="collision-error">{{ t('modpack.collisionError') }}</div>
            </div>

            <div class="info-hint">{{ t('modpack.hint') }}</div>
          </div>
        </div>

        <div v-if="step === 'info'" class="modal-footer">
          <button class="action-btn secondary" @click="finish">{{ t('modpack.cancel') }}</button>
          <button class="action-btn primary" @click="requestInstall" :disabled="hasCollision && customNameInvalid">{{ t('modpack.startInstall') }}</button>
        </div>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
.modal-overlay {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  background: rgba(0, 0, 0, 0.55);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
}
.modal-panel {
  width: 460px;
  max-width: 92vw;
  background: var(--panel-bg);
  border-radius: 16px;
  border: 1px solid var(--border-color);
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
.modal-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 14px 20px;
  border-bottom: 1px solid var(--border-color);
}
.modal-title {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 17px;
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
  opacity: 0.7;
}
.close-btn:hover { opacity: 1; }
.close-btn svg { width: 18px; height: 18px; }

.modal-body {
  padding: 18px 20px;
  min-height: 160px;
  display: flex;
  flex-direction: column;
  justify-content: center;
}
.center-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 14px;
}
.state-text {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 14px;
  color: var(--text-color);
  text-align: center;
}
.state-text.error-text { color: #ff3b30; }
.state-text.done-text { color: #2ecc71; }

.pack-info {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.info-row {
  display: flex;
  align-items: center;
  gap: 12px;
}
.info-label {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 13px;
  color: var(--label-color);
  width: 70px;
  flex-shrink: 0;
}
.info-value {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 13px;
  color: var(--text-color);
  font-weight: 500;
  word-break: break-all;
}
.format-badge {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 11px;
  font-weight: 600;
  padding: 2px 10px;
  border-radius: 6px;
}
.format-badge.modrinth { color: #1bd96a; background: rgba(27, 217, 106, 0.15); }
.format-badge.curseforge { color: #ff7849; background: rgba(241, 100, 54, 0.15); }
.info-hint {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 11px;
  color: var(--label-color);
  margin-top: 4px;
}

.collision-box {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 10px 12px;
  border: 1px solid rgba(255, 149, 0, 0.45);
  border-radius: 10px;
  background: rgba(255, 149, 0, 0.08);
}
.collision-warning {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 12px;
  line-height: 1.6;
  color: #ff9500;
}
.collision-input {
  padding: 9px 14px;
  border: 1px solid var(--border-color);
  border-radius: 8px;
  background: var(--button-bg);
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 13px;
  color: var(--text-color);
  outline: none;
}
.collision-input:focus {
  border-color: #ff9500;
}
.collision-error {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 11px;
  color: #ff3b30;
}

.modal-footer {
  display: flex;
  gap: 10px;
  padding: 14px 20px;
  border-top: 1px solid var(--border-color);
  justify-content: flex-end;
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
.action-btn.primary { background: var(--accent-color); color: var(--accent-text-color); }
.action-btn.secondary { background: var(--button-bg); color: var(--text-color); }

.modal-enter-active {
  transition: opacity 0.25s ease;
}
.modal-enter-active .modal-panel {
  transition: all 0.35s cubic-bezier(0.34, 1.2, 0.64, 1);
}
.modal-enter-from { opacity: 0; }
.modal-enter-from .modal-panel {
  transform: scale(0.9) translateY(20px);
  opacity: 0;
}
</style>
