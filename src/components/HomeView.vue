<script setup lang="ts">
import { ref, computed, onMounted, watch, onUnmounted } from 'vue'
import { getLocalVersions, launchGame, getConfig, saveConfig, listenLaunchStatus, openUrl, getOfflineUuid, getIconUrl, offlineModeAllowed } from '../utils/tauri'
import { cleanSvg } from '../utils/svg'
import { useAuth } from '../utils/auth'
import { useI18n } from 'vue-i18n'
import { toast } from '../utils/toast'
import CrashReport from './CrashReport.vue'
import PluginInject from './PluginInject.vue'
import microsoftSvg from '../assets/icons/microsoft.svg?raw'
import offlineSvg from '../assets/icons/offline.svg?raw'
import steveAvatar from '../assets/icons/steve.png'

const { t } = useI18n()

const microsoftSvgCleaned = cleanSvg(microsoftSvg)
const offlineSvgCleaned = cleanSvg(offlineSvg)

const {
  isLoggedIn,
  msUsername,
  msUuid,
  authStep,
  deviceCode,
  deviceCodeMessage,
  verificationUri,
  authError,
  initAuth,
  startLogin,
  handleLogout,
} = useAuth()

const username = ref('Player')
const selectedVersion = ref('')
// 默认离线登录：未登录正版账号时直接以离线模式使用（首次启动体验）
const isOnline = ref(false)
/** 离线模式是否允许（仅中国大陆 IP 允许；Mojang 许可条款合规） */
const offlineAllowed = ref(true)
const versions = ref<string[]>([])
const loading = ref(false)
const showCrashReport = ref(false)
/** 游戏启动中（进度接入顶部下载条 + 详情面板） */
const launching = ref(false)

const pluginOverrides = ref<Record<string, string>>({})

function handlePluginOverrides(overrides: Record<string, string>) {
  pluginOverrides.value = { ...pluginOverrides.value, ...overrides }
}

const emit = defineEmits<{
  (e: 'launch-state', payload: LaunchStatePayload): void
}>()

const props = defineProps<{
  initialVersion?: string
}>()

interface LaunchStatePayload {
  phase: 'start' | 'update' | 'success' | 'fail'
  version?: string
  stage?: string
  error?: string
}
// 离线默认皮肤头像：18 选 1，与游戏内选择算法一致（1.21.5+ 新默认皮肤）
const offlineAvatarUrl = ref('')
const offlineSkinName = ref('steve')
let unlistenLaunchStatus: (() => void) | null = null

let uuidTimer: ReturnType<typeof setTimeout> | null = null
async function refreshOfflineSkinModel() {
  const name = username.value.trim()
  if (!name) {
    offlineAvatarUrl.value = ''
    offlineSkinName.value = 'steve'
    return
  }
  try {
    const info = await getOfflineUuid(name)
    offlineAvatarUrl.value = await getIconUrl(`skins/${info.skinIndex}.png`)
    offlineSkinName.value = info.skinName
  } catch {
    offlineAvatarUrl.value = ''
    offlineSkinName.value = 'steve'
  }
}

const displayName = computed(() => {
  if (isOnline.value && isLoggedIn.value) return msUsername.value
  return username.value
})

/** 正版模式下未登录：不显示头像/用户名/开始按钮，只留登录按钮（境外强制正版时的干净界面） */
const showProfile = computed(() => !isOnline.value || isLoggedIn.value)

const greetingText = computed(() => {
  const hour = new Date().getHours()
  if (hour >= 5 && hour < 12) return t('home.morning')
  else if (hour >= 12 && hour < 18) return t('home.afternoon')
  else return t('home.evening')
})

async function loadVersions() {
  try {
    loading.value = true
    const list = await getLocalVersions()
    versions.value = list.map(v => v.name)
    if (versions.value.length > 0) {
      selectedVersion.value = props.initialVersion && versions.value.includes(props.initialVersion)
        ? props.initialVersion
        : versions.value[0]
    }
    const config = await getConfig()
    if (config.username) username.value = config.username
  } catch (err) {
    console.error('加载版本失败:', err)
  } finally {
    loading.value = false
  }
}

async function handleLaunch() {
  if (!selectedVersion.value) {
    toast(t('home.selectVersionFirst'), 'error')
    return
  }
  if (!displayName.value.trim()) {
    toast(t('home.enterName'), 'error')
    return
  }
  launching.value = true
  emit('launch-state', {
    phase: 'start',
    version: selectedVersion.value,
    stage: t('home.launching'),
  })
  try {
    // 启动进度由 launch-status 事件驱动（详情面板实时展示阶段），
    // 后端会在检测到游戏窗口真正出现后才发「游戏已启动」。
    await launchGame({
      version: selectedVersion.value,
      username: displayName.value.trim(),
    })
  } catch (err: any) {
    toast(t('home.launchFailed', { msg: String(err) }), 'error')
    launching.value = false
    emit('launch-state', { phase: 'fail', error: String(err) })
  }
}

async function saveUsername() {
  try {
    const config = await getConfig()
    config.username = username.value
    await saveConfig(config)
  } catch (err) {
    console.error('保存用户名失败:', err)
  }
}

function onOnlineClick() {
  isOnline.value = true
  // 未登录且不在轮询中时发起登录（失败后再次点击也可重试）
  if (!isLoggedIn.value && authStep.value !== 'polling') {
    startLogin()
  }
}

async function copyDeviceCode() {
  try {
    await navigator.clipboard.writeText(deviceCode.value)
    deviceCodeMessage.value = t('home.codeCopied')
  } catch {
    toast(t('home.copyFailed', { code: deviceCode.value }), 'error')
  }
}

function reopenVerification() {
  openUrl(verificationUri.value || 'https://microsoft.com/devicelogin')
}

function onOfflineClick() {
  if (!offlineAllowed.value) {
    toast(t('home.offlineRegionDisabled'), 'error')
    return
  }
  isOnline.value = false
}

watch(username, () => {
  saveUsername()
  if (uuidTimer) clearTimeout(uuidTimer)
  uuidTimer = setTimeout(refreshOfflineSkinModel, 300)
})

// 登录成功（会话中完成正版授权）后自动切回正版模式
watch(authStep, (s) => {
  if (s === 'complete') {
    isOnline.value = true
  }
})

onMounted(async () => {
  await initAuth()
  // 已登录正版账号 → 切回正版模式；否则保持默认离线
  if (isLoggedIn.value) {
    isOnline.value = true
  }
  // 合规检查：IP 非中国大陆 → 禁用离线模式并强制切到正版（后端 launch_game 也会拦截）
  try {
    offlineAllowed.value = await offlineModeAllowed()
  } catch {
    offlineAllowed.value = true // 查询失败时默认开放离线模式（与后端一致）
  }
  if (!offlineAllowed.value) {
    isOnline.value = true
  }
  loadVersions()
  refreshOfflineSkinModel()
  
  unlistenLaunchStatus = await listenLaunchStatus((data: any) => {
    const stage = data.stage ? String(data.stage) : ''
    // 启动进度：后端实时阶段文字同步到详情面板
    if (stage && launching.value) {
      emit('launch-state', { phase: 'update', stage })
    }
    if (stage === '游戏已启动') {
      // 游戏窗口出现 → 启动完成
      launching.value = false
      emit('launch-state', { phase: 'success', stage })
    } else if (stage === '游戏异常退出') {
      showCrashReport.value = true
      launching.value = false
      emit('launch-state', { phase: 'fail', stage, error: stage })
    } else if (stage === '游戏已退出') {
      const code = data.exit_code ?? 0
      if (code !== 0) {
        showCrashReport.value = true
        launching.value = false
        emit('launch-state', { phase: 'fail', stage, error: stage })
      } else {
        // 正常退出（此前通常已「游戏已启动」）；若窗口未出现就退出，也结束启动态
        launching.value = false
        emit('launch-state', { phase: 'success', stage })
      }
    }
  })
})

onUnmounted(() => {
  if (unlistenLaunchStatus) {
    unlistenLaunchStatus()
  }
})

// 提供给父组件（App.vue / Touch Bar 启动按钮）调用
defineExpose({ handleLaunch, selectedVersion })
</script>

<template>
  <div class="home-content">
    <div class="greeting">
      <span class="greeting-text">{{ pluginOverrides.greeting || t('home.greeting', { greeting: greetingText, name: displayName }) }}</span>
    </div>

    <div class="login-type">
      <button class="login-btn" :class="{ active: isOnline }" @click="onOnlineClick">
        <span class="login-icon" v-html="microsoftSvgCleaned"></span>
        <span>{{ $t('home.online') }}</span>
      </button>
      <button class="login-btn" :class="{ active: !isOnline }" @click="onOfflineClick" :disabled="!offlineAllowed">
        <span class="login-icon" v-html="offlineSvgCleaned"></span>
        <span>{{ $t('home.offline') }}</span>
      </button>
    </div>

    <!-- 设备代码认证状态 -->
    <div v-if="isOnline && authStep === 'polling'" class="auth-status">
      <div class="device-code">{{ deviceCode }}</div>
      <div class="auth-actions">
        <button class="auth-mini-btn" @click="copyDeviceCode">{{ $t('home.copyCode') }}</button>
        <button class="auth-mini-btn" @click="reopenVerification">{{ $t('home.openWeb') }}</button>
      </div>
      <div class="device-message">{{ deviceCodeMessage }}</div>
      <div class="device-uri">{{ t('home.verifyUrl', { url: verificationUri }) }}</div>
    </div>
    <div v-else-if="isOnline && authError" class="auth-error">
      {{ authError }}
      <button class="retry-btn" @click="startLogin">{{ $t('common.retry') }}</button>
    </div>

    <div v-if="showProfile" class="avatar-section">
      <img
        v-if="isOnline && isLoggedIn && msUuid"
        :src="`https://visage.surgeplay.com/face/128/${msUuid}`"
        class="avatar-image"
        :alt="t('home.avatar')"
      />
      <img
        v-else-if="!isOnline"
        :src="offlineAvatarUrl || steveAvatar"
        class="avatar-image"
        :alt="offlineSkinName"
      />
      <div v-else class="avatar-placeholder">
        <span class="avatar-text">{{ $t('home.avatar') }}</span>
      </div>
    </div>

    <div v-if="showProfile" class="name-section">
      <input
        v-if="!isOnline"
        type="text"
        v-model="username"
        :placeholder="t('home.namePlaceholder')"
        class="name-input"
      />
      <span v-else class="name-text">{{ displayName }}</span>
    </div>

    <button
      v-if="isOnline && isLoggedIn"
      class="logout-btn"
      @click="handleLogout"
    >
      {{ $t('home.logout') }}
    </button>
    <!-- 正版模式下未登录：给一个明确的登录按钮（用户点击才开始设备码流程，不自动打开网页） -->
    <button
      v-else-if="isOnline && !isLoggedIn && authStep !== 'polling'"
      class="login-ms-btn"
      @click="startLogin"
    >
      {{ t('home.signInMicrosoft') }}
    </button>

    <button
      v-if="showProfile"
      class="start-btn"
      @click="handleLaunch"
      :disabled="loading || launching || !selectedVersion"
    >
      <span class="start-text">{{ launching ? t('home.launching') : (pluginOverrides.start_btn || $t('home.startGame')) }}</span>
      <span class="version-text">{{ pluginOverrides.start_sub || t('home.versionLabel', { v: selectedVersion || t('home.notSelected') }) }}</span>
    </button>

    <CrashReport :visible="showCrashReport" @close="showCrashReport = false" />
    <PluginInject page="home" @overrides="handlePluginOverrides" />
  </div>
</template>

<style scoped>
.home-content {
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
  align-items: center;
  padding: 20px;
}

.greeting {
  margin-top: 20px;
}

.greeting-text {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 36px;
  color: var(--text-color);
  line-height: 44px;
  letter-spacing: 0;
}

.login-type {
  display: flex;
  gap: 17px;
  margin-top: 20px;
}

.login-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 4px 12px;
  border-radius: 10px;
  background: var(--card-bg);
  border: none;
  cursor: pointer;
  height: 28px;
  transition: background-color 0.2s;
  color: var(--text-color);
}

.login-btn:active {
  transform: scale(0.95);
}

.login-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.login-ms-btn {
  margin-top: 12px;
  padding: 8px 24px;
  border-radius: 20px;
  border: none;
  background: var(--accent-color);
  color: var(--accent-text-color);
  cursor: pointer;
  font-size: 14px;
  font-weight: 500;
  transition: opacity 0.15s, transform 100ms ease;
}

.login-ms-btn:active {
  transform: scale(0.96);
}

.login-btn.active {
  background: var(--accent-color);
  color: var(--accent-text-color);
}

.login-icon {
  width: 14px;
  height: 14px;
  display: block;
  color: var(--text-color);
}

.login-icon svg {
  width: 100%;
  height: 100%;
  display: block;
  color: var(--text-color);
}

.login-btn.active .login-icon {
  color: var(--accent-text-color);
}

.avatar-section {
  margin-top: 30px;
}

.avatar-placeholder {
  width: 90px;
  height: 90px;
  background: var(--card-bg);
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 12px;
}

.avatar-image {
  width: 90px;
  height: 90px;
  border-radius: 12px;
  object-fit: cover;
}

.avatar-text {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 36px;
  color: var(--text-color);
}

/* 设备代码认证状态 */
.auth-status {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  margin-top: 16px;
}

.device-code {
  font-size: 28px;
  font-weight: 600;
  color: var(--accent-color);
  letter-spacing: 4px;
  padding: 8px 20px;
  background: var(--card-bg);
  border-radius: 10px;
}

.device-message {
  font-size: 14px;
  color: var(--text-color);
  opacity: 0.7;
}

.device-uri {
  font-size: 12px;
  color: var(--label-color);
  opacity: 0.8;
  word-break: break-all;
  text-align: center;
}

.auth-actions {
  display: flex;
  gap: 8px;
}

.auth-mini-btn {
  padding: 4px 14px;
  border-radius: 8px;
  border: 1px solid var(--accent-color);
  background: transparent;
  color: var(--accent-color);
  cursor: pointer;
  font-size: 12px;
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  transition: background 0.15s;
}
.auth-mini-btn:hover {
  background: var(--accent-color);
  color: var(--accent-text-color);
}

.auth-error {
  margin-top: 16px;
  font-size: 14px;
  color: var(--accent-color);
  text-align: center;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
}

.retry-btn {
  padding: 4px 16px;
  border-radius: 8px;
  border: 1px solid var(--accent-color);
  background: transparent;
  color: var(--accent-color);
  cursor: pointer;
  font-size: 13px;
  transition: background 0.15s;
}

.retry-btn:hover {
  background: var(--accent-color);
  color: var(--accent-text-color);
}

.logout-btn {
  margin-top: 12px;
  padding: 4px 16px;
  border-radius: 8px;
  border: 1px solid var(--border-color);
  background: transparent;
  color: var(--text-color);
  cursor: pointer;
  font-size: 13px;
  opacity: 0.6;
  transition: opacity 0.15s;
}

.logout-btn:hover {
  opacity: 1;
}

.name-section {
  margin-top: 20px;
}

.name-text {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 36px;
  color: var(--text-color);
}

/* 输入框样式（与名字文本一致） */
.name-input {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 36px;
  color: var(--text-color);
  background: transparent;
  border: none;
  border-bottom: 2px solid var(--accent-color);
  text-align: center;
  width: 250px;
  outline: none;
  padding: 0 8px;
  user-select: text;
  -webkit-user-select: text;
}

.name-input:focus {
  border-bottom-color: var(--accent-color);
}

.start-btn {
  display: flex;
  flex-direction: column;
  align-items: center;
  padding: 5px 59px 7px;
  border-radius: 25px;
  background: var(--card-bg);
  border: none;
  cursor: pointer;
  margin-top: 30px;
  transition: background-color 0.2s, transform 100ms ease;
}

.start-btn:hover {
  background: var(--accent-color);
}

.start-btn:active {
  transform: scale(0.97);
}

.start-btn:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

.start-btn:disabled:hover {
  background: var(--card-bg);
}

.start-btn:disabled:hover .start-text,
.start-btn:disabled:hover .version-text {
  color: var(--text-color);
}

.start-btn:hover .start-text,
.start-btn:hover .version-text {
  color: var(--accent-text-color);
}

.start-text {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 32px;
  color: var(--text-color);
}

.version-text {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 16px;
  color: var(--text-color);
  margin-top: 7px;
  line-height: 19px;
}


</style>