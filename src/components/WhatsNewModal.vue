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
    title: 'Lumia v1.0',
    items: [
      '恢复 Minecraft 游戏本体下载：版本、Mod、整合包、资源包与光影包全部可正常安装',
      '新增「导出整合包」：把已安装的版本打包成 .mrpack，可分享给其他启动器',
      '修复 Fabric 模组启动崩溃（ClassNotFoundException）：补上缺失的 intermediary 映射依赖',
      '界面全局隐藏滚动条，观感更简洁',
    ],
  },
  {
    lang: 'en',
    title: 'Lumia v1.0',
    items: [
      'Minecraft game downloads are available again: versions, mods, modpacks, resource packs and shader packs all install normally',
      'New "Export modpack": package an installed version as a .mrpack to share with other launchers',
      'Fixed Fabric mods crashing on launch (ClassNotFoundException) by adding the missing intermediary mapping dependency',
      'Scrollbars are now hidden throughout the interface for a cleaner look',
    ],
  },
  {
    lang: 'ja',
    title: 'Lumia v1.0',
    items: [
      'Minecraft 本体のダウンロードを再開：バージョン、Mod、Modpack、リソースパック、シェーダーパックがすべて通常どおりインストールできます',
      '新機能「Modpack を書き出す」：インストール済みのバージョンを .mrpack にまとめて他のランチャーと共有できます',
      'Fabric の Mod が起動時にクラッシュする問題（ClassNotFoundException）を修正：不足していた intermediary マッピングを追加',
      'インターフェース全体でスクロールバーを非表示にし、よりすっきりした見た目に',
    ],
  },
  {
    lang: 'fr',
    title: 'Lumia v1.0',
    items: [
      "Le téléchargement du jeu Minecraft est de nouveau disponible : versions, mods, modpacks, resource packs et shaders s'installent normalement",
      'Nouveau « Exporter un modpack » : empaquetez une version installée en .mrpack pour la partager avec d\'autres lanceurs',
      'Correctif : plantage des mods Fabric au lancement (ClassNotFoundException) — ajout de la dépendance de mappage intermediary manquante',
      "Les barres de défilement sont désormais masquées dans toute l'interface, pour un rendu plus épuré",
    ],
  },
  {
    lang: 'ko',
    title: 'Lumia v1.0',
    items: [
      'Minecraft 게임 본체 다운로드 재개: 버전, 모드, 모드팩, 리소스 팩, 셰이더 팩 모두 정상 설치 가능',
      '새 기능 "모드팩 내보내기": 설치된 버전을 .mrpack으로 패키징해 다른 런처와 공유 가능',
      'Fabric 모드 실행 시 강제 종료(ClassNotFoundException) 수정: 누락된 intermediary 매핑 의존성 추가',
      '인터페이스 전체에서 스크롤바를 숨겨 더 깔끔한 화면',
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