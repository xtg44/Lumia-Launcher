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

// 每条目按语言索引；数组第 1 项必须是 en：locale 完全匹配不上时会兜底到 changelogs[1]。
// 发布新版本时记得同步更新 title 与条目内容（与 Release 说明保持一致即可）。
const changelogs: ChangelogEntry[] = [
  {
    lang: 'zh-CN',
    title: 'Lumia v1.0 beta 4',
    items: [
      '新增 Linux 版安装包（.deb）：Debian / Ubuntu 可直接安装',
      '修复启动游戏闪退：Java 版本不满足要求时会自动切换到合适的 Java',
      '修复「打开日志文件」无效的问题',
      '修复游戏设置被重置的问题：options.txt 不再每次启动都被覆盖',
    ],
  },
  {
    lang: 'en',
    title: 'Lumia v1.0 beta 4',
    items: [
      'New Linux package (.deb) for Debian / Ubuntu',
      'Fixed crashing on launch: automatically switches to a suitable Java when the configured version is too old',
      'Fixed the "Open Log File" action not working',
      'Fixed game settings being reset: options.txt is no longer overwritten on every launch',
    ],
  },
  {
    lang: 'ja',
    title: 'Lumia v1.0 beta 4',
    items: [
      'Linux 版パッケージ（.deb）を追加：Debian / Ubuntu にそのままインストールできます',
      '起動時にクラッシュする問題を修正：Java バージョンが要件を満たさない場合は自動的に適切な Java に切り替えます',
      '「ログファイルを開く」が機能しない問題を修正',
      'ゲーム設定がリセットされる問題を修正：options.txt が毎回上書きされなくなります',
    ],
  },
  {
    lang: 'fr',
    title: 'Lumia v1.0 beta 4',
    items: [
      'Nouveau paquet Linux (.deb) pour Debian / Ubuntu',
      'Correctif : plantage au lancement du jeu — bascule automatique vers un Java adapté si nécessaire',
      'Correctif : le bouton « Ouvrir le fichier journal » ne fonctionnait pas',
      "Correctif : les paramètres du jeu étaient réinitialisés — options.txt n'est plus écrasé à chaque lancement",
    ],
  },
  {
    lang: 'ko',
    title: 'Lumia v1.0 beta 4',
    items: [
      'Linux 패키지(.deb) 추가: Debian / Ubuntu에서 바로 설치 가능',
      '게임 실행 시 강제 종료되는 문제 수정: Java 버전이 요구 사항에 미치지 못하면 알맞은 Java로 자동 전환합니다',
      '"로그 파일 열기"가 작동하지 않는 문제 수정',
      '게임 설정이 초기화되는 문제 수정: 이제 options.txt는 매번 덮어쓰이지 않습니다',
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