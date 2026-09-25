<script setup lang="ts">
import { ref, computed, watch, onMounted, onUnmounted } from 'vue'
import {
  getConfig,
  terracottaStart,
  terracottaStatus,
  terracottaHost,
  terracottaJoin,
  terracottaStop,
  terracottaBack,
  terracottaNodesStatus,
  isGameRunning,
} from '../utils/tauri'
import type { TerracottaNodeStatus } from '../utils/tauri'
import { useI18n } from 'vue-i18n'
import { toast } from '../utils/toast'

const { t } = useI18n()

const port = ref<number | null>(null)
const playerName = ref('')
const statusText = ref(t('terracotta.notStarted'))
const roomCodeValue = ref('')
const joinInput = ref('')
const error = ref('')
const warning = ref('')
const busy = ref(false)
const profiles = ref<any[]>([])
const scanning = ref(false)
/** 当前陶瓦状态（waiting / host-scanning / host-ok / exception 等），决定哪些操作可用 */
const tcState = ref('')
/** 自定义公共节点（用户自己的 easytier 共享节点，可多填，用逗号/换行分隔） */
const customNodeInput = ref('')
/** 公共节点可达性检查结果 */
const nodeStatus = ref<TerracottaNodeStatus[]>([])
const nodesChecking = ref(false)

const CUSTOM_NODES_KEY = 'lumia-terracotta-custom-nodes'

let pollTimer: ReturnType<typeof setInterval> | null = null

/** 解析自定义节点输入（逗号/分号/换行分隔，去空项） */
function customNodeList(): string[] {
  return customNodeInput.value
    .split(/[,，;；\n]+/)
    .map((s) => s.trim())
    .filter((s) => s.length > 0)
}

/** 所有已探测节点是否全部明确离线（公共节点明确全挂时组网无法建立；未知状态的节点不算挂） */
const allNodesDead = computed(() => nodeStatus.value.length > 0 && nodeStatus.value.every((n) => n.ok === false))

watch(customNodeInput, (v) => {
  try {
    localStorage.setItem(CUSTOM_NODES_KEY, v)
  } catch {
    // localStorage 不可用时忽略（仅记忆输入，不影响功能）
  }
})

onMounted(async () => {
  try {
    customNodeInput.value = localStorage.getItem(CUSTOM_NODES_KEY) || ''
  } catch {
    customNodeInput.value = ''
  }
  try {
    const config = await getConfig()
    playerName.value = config.username || config.ms_username || 'Player'
  } catch {
    playerName.value = 'Player'
  }
  checkNodes()
})

onUnmounted(() => {
  stopPolling()
})

/** 检查公共节点可达性（TCP 层），用于展示陶瓦组网是否可能成功 */
async function checkNodes() {
  if (nodesChecking.value) return
  nodesChecking.value = true
  try {
    nodeStatus.value = await terracottaNodesStatus(customNodeList())
  } catch {
    nodeStatus.value = []
  } finally {
    nodesChecking.value = false
  }
}

function startPolling() {
  stopPolling()
  pollTimer = setInterval(async () => {
    if (port.value === null) return
    try {
      const s = await terracottaStatus(port.value)
      applyStatus(s)
    } catch {
      // 后台可能暂未就绪
    }
  }, 2000)
}

function stopPolling() {
  if (pollTimer) {
    clearInterval(pollTimer)
    pollTimer = null
  }
}

function stateType(s: any): string {
  // 注意：s.type 是 exception 的错误码（数字，如 4=PingServerRst），不是状态名！
  // 状态名在 s.state（字符串，如 "exception"、"host-ok"）。之前写 s?.type || s?.state
  // 导致 exception 时返回数字 4，switch 匹配不到，界面显示"状态：4"。
  return s?.state || (typeof s?.type === 'string' ? s?.type : 'unknown')
}

function applyStatus(s: any) {
  const type = stateType(s)
  tcState.value = type
  profiles.value = s?.profiles || []
  switch (type) {
    case 'waiting':
      statusText.value = t('terracotta.waiting')
      roomCodeValue.value = ''
      scanning.value = false
      break
    case 'host-scanning':
      // 扫描局域网世界：等待用户在游戏内开启「对局域网开放」（参考 HMCL 文案）
      statusText.value = t('terracotta.scanning')
      scanning.value = true
      warning.value = t('terracotta.scanHint')
      break
    case 'host-starting':
      statusText.value = t('terracotta.hostStarting')
      break
    case 'host-ok':
      statusText.value = t('terracotta.hostOk')
      warning.value = ''
      scanning.value = false
      roomCodeValue.value = s?.code || ''
      break
    case 'guest-connecting':
      statusText.value = t('terracotta.guestConnecting')
      break
    case 'guest-starting':
      statusText.value = t('terracotta.guestStarting')
      break
    case 'guest-ok':
      statusText.value = t('terracotta.guestOk')
      warning.value = ''
      scanning.value = false
      roomCodeValue.value = s?.code || ''
      break
    case 'exception':
      // Terracotta Exception.Type 枚举（见 TerracottaState.java）：
      // 0 PING_HOST_FAIL 1 PING_HOST_RST 2 GUEST_ET_CRASH 3 HOST_ET_CRASH
      // 4 PING_SERVER_RST 5 SCAFFOLDING_INVALID_RESPONSE
      const typeMap: Record<number, string> = {
        0: t('terracotta.err0'),
        1: t('terracotta.err1'),
        2: t('terracotta.err2'),
        3: t('terracotta.err3'),
        4: t('terracotta.err4'),
        5: t('terracotta.err5'),
      }
      const reason = typeMap[Number(s?.type)] || s?.message || s?.reason || t('terracotta.unknownError')
      statusText.value = t('terracotta.errorPrefix', { reason })
      error.value = statusText.value
      scanning.value = false
      // 房间码在日志里已生成过，即使 easytier 建隧道失败也展示出来
      if (s?.code) {
        roomCodeValue.value = s?.code
      }
      break
    default:
      statusText.value = t('terracotta.statusPrefix', { type })
  }
}

async function start() {
  busy.value = true
  error.value = ''
  warning.value = ''
  try {
    port.value = await terracottaStart()
    statusText.value = t('terracotta.starting')
    tcState.value = 'waiting'
    startPolling()
    const s = await terracottaStatus(port.value)
    applyStatus(s)
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err)
  } finally {
    busy.value = false
  }
}

async function host() {
  if (port.value === null || busy.value) return
  busy.value = true
  error.value = ''
  warning.value = ''
  try {
    // 参考 HMCL：游戏未运行时提示先启动游戏（但不强制，可在别的启动器开着）
    const running = await isGameRunning()
    if (!running) {
      warning.value = t('terracotta.gameNotRunning')
    }
    await terracottaHost(port.value, playerName.value.trim() || 'Player', customNodeList())
    statusText.value = t('terracotta.scanning')
    tcState.value = 'host-scanning'
    scanning.value = true
    startPolling()
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err)
  } finally {
    busy.value = false
  }
}

/** 停止扫描/退出房间，回到等待状态（HMCL 的 setWaiting） */
async function backToWaiting() {
  if (port.value === null || busy.value) return
  busy.value = true
  try {
    await terracottaBack(port.value)
    warning.value = ''
    scanning.value = false
    tcState.value = 'waiting'
    statusText.value = t('terracotta.stoppedScanning')
    roomCodeValue.value = ''
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err)
  } finally {
    busy.value = false
  }
}

async function join() {
  if (port.value === null || busy.value) return
  const code = joinInput.value.trim().toUpperCase()
  if (!code) {
    error.value = t('terracotta.enterCode')
    return
  }
  // 客户端先校验格式，避免 400：U/XXXX-XXXX-XXXX-XXXX（20 字符，无 I/O）
  const fmt = /^U\/[0-9A-HJ-NP-Z]{4}-[0-9A-HJ-NP-Z]{4}-[0-9A-HJ-NP-Z]{4}-[0-9A-HJ-NP-Z]{4}$/
  if (!fmt.test(code)) {
    error.value = t('terracotta.badCode')
    return
  }
  busy.value = true
  error.value = ''
  try {
    await terracottaJoin(port.value, code, playerName.value.trim() || 'Player', customNodeList())
    statusText.value = t('terracotta.guestStarting')
    startPolling()
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err)
  } finally {
    busy.value = false
  }
}

async function stop() {
  busy.value = true
  try {
    await terracottaStop()
    port.value = null
    roomCodeValue.value = ''
    statusText.value = t('terracotta.disconnected')
    error.value = ''
    warning.value = ''
    scanning.value = false
    tcState.value = ''
    stopPolling()
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err)
  } finally {
    busy.value = false
  }
}

async function copyRoomCode() {
  if (!roomCodeValue.value) return
  try {
    await navigator.clipboard.writeText(roomCodeValue.value)
  } catch {
    toast(t('terracotta.copyFailed', { code: roomCodeValue.value }), 'error')
  }
}
</script>

<template>
  <div class="tc-content">
    <div class="tc-header">
      <span class="tc-title">{{ t('terracotta.title') }}</span>
      <span class="tc-badge">Terracotta</span>
      <span class="tc-credit" v-html="t('terracotta.credit')"></span>
    </div>

    <div class="tc-body">
      <!-- 未启动 -->
      <div v-if="port === null" class="tc-start">
        <div class="tc-status">{{ statusText }}</div>
        <button class="tc-btn primary" :disabled="busy" @click="start">
          {{ busy ? t('terracotta.starting') : t('terracotta.startBtn') }}
        </button>
        <div class="tc-hint">{{ t('terracotta.firstUseHint') }}</div>
      </div>

      <!-- 已启动 -->
      <div v-else class="tc-running">
        <div class="tc-status-row">
          <span class="tc-status" :class="{ ok: roomCodeValue }">{{ statusText }}</span>
          <button class="tc-btn small danger" @click="stop">{{ t('terracotta.disconnect') }}</button>
        </div>

        <!-- 房间码（host-ok / exception 时展示） -->
        <div v-if="roomCodeValue" class="tc-room">
          <div class="tc-room-code" @click="copyRoomCode" :title="t('terracotta.tapToCopy')">{{ roomCodeValue }}</div>
          <div class="tc-room-hint">{{ t('terracotta.roomHint') }}</div>
          <div class="tc-game-hint">{{ t('terracotta.gameHint') }}</div>
          <!-- 房间开着（host-ok）时给"退出房间"按钮 -->
          <button v-if="tcState === 'host-ok'" class="tc-btn small" :disabled="busy" @click="backToWaiting">{{ t('terracotta.exitRoom') }}</button>
          <div v-if="allNodesDead && tcState === 'host-ok'" class="tc-warning">
            {{ t('terracotta.allNodesDeadHint') }}
          </div>
        </div>

        <!-- 仅 waiting 状态允许开房/加入；其他状态（host-*/guest-*/exception）不允许，
             否则陶瓦 /state/guesting 会对非 waiting 状态返回 400 -->
        <div v-else-if="tcState === 'waiting'" class="tc-actions">
          <div class="tc-field">
            <label class="tc-label">{{ t('terracotta.playerName') }}</label>
            <input type="text" v-model="playerName" class="tc-input" :placeholder="t('terracotta.playerPlaceholder')" />
          </div>

          <button class="tc-btn primary" :disabled="busy" @click="host">
            {{ busy ? t('terracotta.processing') : t('terracotta.hostRoom') }}
          </button>

          <div class="tc-divider">{{ t('terracotta.orJoin') }}</div>

          <div class="tc-field">
            <input
              type="text"
              v-model="joinInput"
              class="tc-input"
              :placeholder="t('terracotta.joinPlaceholder')"
              @keyup.enter="join"
            />
          </div>
          <button class="tc-btn" :disabled="busy" @click="join">{{ t('terracotta.joinRoom') }}</button>

          <div class="tc-field">
            <label class="tc-label">{{ t('terracotta.customNodes') }}</label>
            <input
              type="text"
              v-model="customNodeInput"
              class="tc-input"
              :placeholder="t('terracotta.customNodesPlaceholder')"
            />
          </div>

          <div class="tc-nodes">
            <div class="tc-nodes-head">
              <span class="tc-nodes-title">{{ t('terracotta.nodeStatus') }}</span>
              <button class="tc-btn small" :disabled="nodesChecking" @click="checkNodes">
                {{ nodesChecking ? t('terracotta.checking') : t('terracotta.recheck') }}
              </button>
            </div>
            <div v-if="nodeStatus.length === 0" class="tc-hint">{{ t('terracotta.noNodes') }}</div>
            <div v-for="n in nodeStatus" :key="n.url" class="tc-node-row">
              <span
                class="tc-node-dot"
                :class="{ ok: n.ok === true, dead: n.ok === false, unknown: n.ok === null }"
                :title="n.ok === true ? t('terracotta.nodesOk') : n.ok === false ? t('terracotta.nodesDead') : t('terracotta.nodesUnknown')"
              ></span>
              <span class="tc-node-url">{{ n.url }}</span>
            </div>
            <div v-if="allNodesDead" class="tc-nodes-dead">
              {{ t('terracotta.nodesAllDead') }}
            </div>
          </div>
        </div>

        <!-- 扫描中的操作提示与取消 -->
        <div v-if="scanning" class="tc-scanning">
          <div class="tc-scanning-hint">{{ warning || t('terracotta.scanning') }}</div>
          <button class="tc-btn small" :disabled="busy" @click="backToWaiting">{{ t('terracotta.stopScanning') }}</button>
        </div>

        <div v-if="warning && !scanning" class="tc-warning">{{ warning }}</div>
        <div v-if="error" class="tc-error">{{ error }}</div>
        <div v-if="profiles.length" class="tc-profiles">
          {{ t('terracotta.profilesLabel') }}
          <span v-for="p in profiles" :key="p.name || p.uuid" class="tc-profile">{{ p.name || p.uuid }}</span>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.tc-content {
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
  padding: 10px 17px;
  box-sizing: border-box;
}
.tc-header {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 0;
  border-bottom: 1px solid var(--border-color);
  flex-shrink: 0;
}
.tc-title {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 20px;
  font-weight: 600;
  color: var(--text-color);
}
.tc-badge {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 11px;
  color: #1bd96a;
  background: rgba(27, 217, 106, 0.15);
  padding: 2px 10px;
  border-radius: 6px;
}
.tc-credit {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 11px;
  color: var(--label-color);
  margin-left: auto;
}
.tc-credit a {
  color: var(--accent-color);
  text-decoration: none;
}
.tc-body {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 20px 0;
}
.tc-start {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 16px;
}
.tc-running {
  width: 100%;
  max-width: 460px;
  display: flex;
  flex-direction: column;
  gap: 14px;
}
.tc-status-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
}
.tc-status {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 14px;
  color: var(--text-color);
  opacity: 0.8;
}
.tc-status.ok {
  color: #2ecc71;
  opacity: 1;
}
.tc-btn {
  padding: 10px 22px;
  border-radius: 9px;
  border: none;
  cursor: pointer;
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 14px;
  font-weight: 500;
  background: var(--button-bg);
  color: var(--text-color);
  transition: transform 100ms ease, opacity 150ms ease;
}
.tc-btn.primary {
  background: var(--accent-color);
  color: var(--accent-text-color);
}
.tc-btn.small {
  padding: 6px 14px;
  font-size: 12px;
}
.tc-btn.danger {
  background: rgba(255, 59, 48, 0.15);
  color: #ff3b30;
}
.tc-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.tc-hint {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 12px;
  color: var(--label-color);
  text-align: center;
}
.tc-room {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  padding: 20px;
  background: var(--card-bg);
  border-radius: 12px;
}
.tc-room-code {
  font-family: 'SF Mono', 'Menlo', monospace;
  font-size: 34px;
  font-weight: 700;
  letter-spacing: 4px;
  color: var(--accent-color);
  cursor: pointer;
  user-select: text;
  -webkit-user-select: text;
}
.tc-room-hint {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 12px;
  color: var(--label-color);
}
.tc-game-hint {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 12px;
  color: var(--label-color);
  text-align: center;
  line-height: 1.6;
}
.tc-actions {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.tc-field {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.tc-label {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 12px;
  color: var(--label-color);
}
.tc-input {
  padding: 10px 14px;
  border: 1px solid var(--border-color);
  border-radius: 9px;
  background: var(--button-bg);
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 14px;
  color: var(--text-color);
  outline: none;
  box-sizing: border-box;
}
.tc-input:focus {
  border-color: var(--accent-color);
}
.tc-divider {
  text-align: center;
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 12px;
  color: var(--label-color);
  margin: 4px 0;
}
.tc-nodes {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 10px 12px;
  background: var(--card-bg);
  border-radius: 10px;
}
.tc-nodes-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
}
.tc-nodes-title {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 12px;
  color: var(--label-color);
}
.tc-node-row {
  display: flex;
  align-items: center;
  gap: 8px;
}
.tc-node-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: #ff3b30;
  flex-shrink: 0;
}
.tc-node-dot.ok {
  background: #2ecc71;
}
.tc-node-dot.dead {
  background: #ff3b30;
}
.tc-node-dot.unknown {
  background: #f5b041;
}
.tc-node-url {
  font-family: 'SF Mono', 'Menlo', monospace;
  font-size: 11px;
  color: var(--text-color);
  opacity: 0.75;
  word-break: break-all;
}
.tc-nodes-dead {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 12px;
  color: #ff3b30;
  line-height: 1.6;
}
.tc-error {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 12px;
  color: #ff3b30;
  text-align: center;
}
.tc-warning {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 12px;
  color: #e8a33d;
  text-align: center;
  line-height: 1.6;
}
.tc-scanning {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 10px;
  padding: 14px;
  background: rgba(232, 163, 61, 0.08);
  border-radius: 10px;
}
.tc-scanning-hint {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 12px;
  color: var(--text-color);
  text-align: center;
  line-height: 1.7;
}
.tc-profiles {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 12px;
  color: var(--label-color);
  text-align: center;
}
.tc-profile {
  color: var(--text-color);
  margin-left: 6px;
}
</style>
