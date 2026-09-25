<script setup lang="ts">
import { ref, computed } from 'vue'
import { getAppVersion, markChangelogSeen, getConfig } from '../utils/tauri'
import { useI18n } from 'vue-i18n'

const { t, locale } = useI18n()

const visible = ref(false)
const currentVersion = ref('')

interface ChangelogEntry {
  lang: string
  title: string
  items: string[]
}

const changelogs: ChangelogEntry[] = [
  {
    lang: 'zh-CN',
    title: 'Lumia 公测版正式发布',
    items: [
      'Lumia公测版正式发布了！Lumia是一款极致轻量的启动器。',
      '好好享受吧 :)',
    ],
  },
  {
    lang: 'en',
    title: 'Lumia Public Beta is here!',
    items: [
      'Lumia Public Beta is officially out — an ultra-lightweight launcher.',
      'Enjoy! :)',
    ],
  },
  {
    lang: 'ja',
    title: 'Lumia 公開ベータ版リリース',
    items: [
      'Lumia 公開ベータ版が正式にリリースされました！超軽量なランチャーです。',
      'ぜひお楽しみください :)',
    ],
  },
  {
    lang: 'fr',
    title: 'La bêta publique de Lumia est là !',
    items: [
      'La bêta publique de Lumia est officiellement sortie — un lanceur ultra-léger.',
      'Profitez-en bien :)',
    ],
  },
]

const currentChangelog = computed(() => {
  const lang = locale.value
  const exact = changelogs.find(c => c.lang === lang)
  if (exact) return exact
  // fallback: try parent language (e.g. "zh" for "zh-CN")
  const parentPrefix = lang.split('-')[0]
  const parent = changelogs.find(c => c.lang.startsWith(parentPrefix))
  if (parent) return parent
  // finally English
  return changelogs[1]
})

async function check() {
  try {
    const version = await getAppVersion()
    currentVersion.value = version

    const config = await getConfig()
    if (config.last_seen_version !== version) {
      visible.value = true
    }
  } catch {
    // 静默失败
  }
}

async function handleClose() {
  visible.value = false
  try {
    await markChangelogSeen(currentVersion.value)
  } catch {
    // 静默失败
  }
}

defineExpose({ check })
</script>

<template>
  <Transition name="modal">
      <div v-if="visible" class="modal-overlay" @click.self="handleClose">
        <div class="modal-card">
          <div class="modal-header">
            <span class="modal-title">{{ currentChangelog.title }}</span>
            <button class="close-btn" @click="handleClose" :aria-label="t('common.close')">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="22" height="22">
                <line x1="18" y1="6" x2="6" y2="18"/>
                <line x1="6" y1="6" x2="18" y2="18"/>
              </svg>
            </button>
          </div>
          <div class="modal-body">
            <p class="changelog-subtitle">{{ t('app.whatsNewSubtitle') }}</p>
            <ul class="changelog-list">
              <li v-for="(item, idx) in currentChangelog.items" :key="idx" class="changelog-item">{{ item }}</li>
            </ul>
          </div>
          <div class="modal-footer">
            <button class="confirm-btn" @click="handleClose">{{ t('app.whatsNewGotIt') }}</button>
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
  background: var(--popup-bg, #ffffff);
  border: 1px solid var(--border-color);
  border-radius: 16px;
  width: 460px;
  max-width: 90vw;
  box-shadow: var(--popup-shadow);
  overflow: hidden;
}

.modal-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 18px 20px 12px;
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

.close-btn:hover {
  opacity: 1;
}

.close-btn:active {
  background: var(--button-active);
}

.modal-body {
  padding: 4px 20px 16px;
}

.changelog-subtitle {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 14px;
  color: var(--label-color);
  margin-bottom: 12px;
}

.changelog-list {
  list-style: none;
  padding: 0;
  margin: 0;
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.changelog-item {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 14px;
  line-height: 20px;
  color: var(--text-color);
  padding-left: 16px;
  position: relative;
}

.changelog-item::before {
  content: '';
  position: absolute;
  left: 0;
  top: 8px;
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--accent-color);
}

.modal-footer {
  padding: 8px 20px 18px;
  display: flex;
  justify-content: flex-end;
}

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

.confirm-btn:active {
  transform: scale(0.96);
}

/* Transition */
.modal-enter-active {
  transition: opacity 0.25s ease;
}
.modal-leave-active {
  transition: opacity 0.2s ease;
}
.modal-enter-from,
.modal-leave-to {
  opacity: 0;
}
.modal-enter-active .modal-card {
  transition: transform 0.25s cubic-bezier(0.34, 1.2, 0.64, 1);
}
.modal-leave-active .modal-card {
  transition: transform 0.2s ease;
}
.modal-enter-from .modal-card {
  transform: scale(0.9) translateY(10px);
}
.modal-leave-to .modal-card {
  transform: scale(0.95) translateY(5px);
}
</style>