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
    title: 'Lumia v1.0 RC 1',
    items: [
      '根据国家法律法规，中国大陆地区不再提供 Minecraft 游戏本体下载；已安装的版本仍可正常启动，模组、资源包、光影包与整合包不受影响',
      '改进版本比较：正确识别 beta / RC / 正式版的先后顺序，避免候选版被误判为「没有新版本」',
      '新增 Linux 版安装包（.deb）：Debian / Ubuntu 可直接安装',
      '修复启动游戏闪退：Java 版本不满足要求时会自动切换到合适的 Java',
      '修复「打开日志文件」无效，以及游戏设置被重置（options.txt 不再每次被覆盖）',
    ],
  },
  {
    lang: 'en',
    title: 'Lumia v1.0 RC 1',
    items: [
      'In accordance with local laws and regulations, Minecraft game downloads are no longer available in mainland China. Installed versions still launch, and mods, resource packs, shader packs and modpacks are unaffected',
      'Improved version comparison: beta / RC / stable ordering is now understood, so release candidates are no longer mistaken for "no update"',
      'New Linux package (.deb) for Debian / Ubuntu',
      'Fixed crashing on launch: automatically switches to a suitable Java when the configured version is too old',
      'Fixed the "Open Log File" action and game settings being reset (options.txt is no longer overwritten on every launch)',
    ],
  },
  {
    lang: 'ja',
    title: 'Lumia v1.0 RC 1',
    items: [
      '現地の法令に基づき、中国大陸では Minecraft 本体のダウンロードを提供しなくなりました。インストール済みのバージョンは引き続き起動でき、Mod・リソースパック・シェーダー・Modpack は影響を受けません',
      'バージョン比較を改善：beta / RC / 正式版の順序を正しく認識し、RC が「更新なし」と誤判定されなくなりました',
      'Linux 版パッケージ（.deb）を追加：Debian / Ubuntu にそのままインストールできます',
      '起動時にクラッシュする問題を修正：Java バージョンが要件を満たさない場合は自動的に適切な Java に切り替えます',
      '「ログファイルを開く」が機能しない問題と、ゲーム設定がリセットされる問題を修正（options.txt が毎回上書きされなくなります）',
    ],
  },
  {
    lang: 'fr',
    title: 'Lumia v1.0 RC 1',
    items: [
      'Conformément à la réglementation locale, le téléchargement du jeu Minecraft n\'est plus disponible en Chine continentale. Les versions installées restent jouables ; les mods, resource packs, shaders et modpacks ne sont pas affectés',
      "Comparaison de versions améliorée : l'ordre bêta / RC / stable est désormais reconnu, une RC n'est plus prise pour « aucune mise à jour »",
      'Nouveau paquet Linux (.deb) pour Debian / Ubuntu',
      'Correctif : plantage au lancement du jeu — bascule automatique vers un Java adapté si nécessaire',
      "Correctif : le bouton « Ouvrir le fichier journal » et la réinitialisation des paramètres du jeu (options.txt n'est plus écrasé à chaque lancement)",
    ],
  },
  {
    lang: 'ko',
    title: 'Lumia v1.0 RC 1',
    items: [
      '현지 법규에 따라 중국 본토에서는 Minecraft 게임 본체 다운로드를 더 이상 제공하지 않습니다. 설치된 버전은 계속 실행할 수 있으며 모드, 리소스 팩, 셰이더, 모드팩은 영향을 받지 않습니다',
      '버전 비교 개선: beta / RC / 정식 버전의 순서를 올바르게 인식하여 RC가 "업데이트 없음"으로 잘못 판정되지 않습니다',
      'Linux 패키지(.deb) 추가: Debian / Ubuntu에서 바로 설치 가능',
      '게임 실행 시 강제 종료되는 문제 수정: Java 버전이 요구 사항에 미치지 못하면 알맞은 Java로 자동 전환합니다',
      '"로그 파일 열기" 및 게임 설정 초기화 문제 수정 (options.txt가 더 이상 매번 덮어쓰이지 않습니다)',
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