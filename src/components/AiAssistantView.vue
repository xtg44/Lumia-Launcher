<script setup lang="ts">
import { ref, reactive, nextTick } from 'vue'
import { aiChat, aiExecuteFileOps } from '../utils/tauri'
import type { AiChatMessage, AiFileOp, AiUsage } from '../utils/tauri'
import { listen } from '@tauri-apps/api/event'
import { useI18n } from 'vue-i18n'
import MarkdownIt from 'markdown-it'

const { t } = useI18n()

// Markdown 渲染：不解析原始 HTML；链接只允许 http(s)/mailto（防 AI 生成 javascript: 链接）
const md = new MarkdownIt({ html: false, linkify: true, breaks: true })
md.validateLink = (url: string) => /^(https?:|mailto:)/i.test(url)

function renderMd(content: string): string {
  return md.render(content)
}

interface ChatMsg {
  role: 'user' | 'assistant'
  content: string
  usage?: AiUsage
  /** 工具执行状态行 */
  statuses?: string[]
}

const messages = ref<ChatMsg[]>([])
const input = ref('')
const sending = ref(false)
const error = ref('')
/** 待用户审批的文件操作 */
const pendingOps = ref<AiFileOp[] | null>(null)
const executing = ref(false)

const chatListRef = ref<HTMLElement | null>(null)

/** 回答必须带 Wiki 来源标注（合规）；仅当确实注入了 Wiki 资料时才补，且模型漏标时兜底 */
function withAttribution(reply: string, usedWiki: boolean): string {
  if (!reply.trim() || !usedWiki) return reply
  if (reply.includes('Minecraft Wiki')) return reply
  return reply + '\n\n' + t('ai.attribution')
}

async function scrollToBottom() {
  await nextTick()
  chatListRef.value?.scrollTo({ top: chatListRef.value.scrollHeight })
}

async function send(text?: string) {
  const content = (text ?? input.value).trim()
  if (!content || sending.value) return
  input.value = ''
  error.value = ''
  messages.value.push({ role: 'user', content })
  // 占位 assistant 消息，流式事件逐字填充。
  // 必须用 reactive()：直接把事件写进原始对象不会触发 Vue 重渲染（之前流式全程看不见）
  const assistantMsg = reactive<ChatMsg>({ role: 'assistant', content: '', statuses: [] })
  messages.value.push(assistantMsg)
  sending.value = true
  await scrollToBottom()
  try {
    // 历史消息不含占位
    const history: AiChatMessage[] = messages.value
      .filter((m) => m !== assistantMsg)
      .map((m) => ({ role: m.role, content: m.content }))
    const unlisten = await listen<any>('ai-stream', (e) => {
      const p = e.payload
      if (p.kind === 'content') {
        assistantMsg.content += p.text
      } else if (p.kind === 'status') {
        const label =
          p.tool === 'list_dir' ? t('ai.toolList') : p.tool === 'read_file' ? t('ai.toolRead') : p.tool
        assistantMsg.statuses = [...(assistantMsg.statuses || []), label + (p.path ? ' ' + p.path : '')]
      }
    })
    const result = await aiChat(history)
    await unlisten()
    // 流式已填充内容；用 result.reply 兜底 + 补 Wiki 标注
    assistantMsg.content = withAttribution(result.reply || assistantMsg.content, result.usedWiki)
    assistantMsg.usage = result.usage
    // 解析工具提议（文件操作）
    const ops: AiFileOp[] = []
    for (const call of result.toolCalls || []) {
      if (call.name === 'modify_files' && call.arguments?.operations) {
        for (const op of call.arguments.operations) {
          if (op && op.action && op.path) {
            ops.push({ action: op.action, path: op.path, content: op.content })
          }
        }
      }
    }
    if (ops.length > 0) {
      pendingOps.value = ops
    }
  } catch (err) {
    assistantMsg.content = assistantMsg.content || t('ai.errorPrefix', { msg: err instanceof Error ? err.message : String(err) })
    error.value = t('ai.errorPrefix', { msg: err instanceof Error ? err.message : String(err) })
  } finally {
    sending.value = false
    await scrollToBottom()
  }
}

/** 分析崩溃日志：让 AI 自己用工具查找并读取日志（不再依赖单一 crash-reports 路径） */
async function analyzeCrash() {
  if (sending.value) return
  await send(t('ai.findCrashPrompt'))
}

/** 用户批准后执行文件操作 */
async function approveOps() {
  if (!pendingOps.value || executing.value) return
  executing.value = true
  error.value = ''
  try {
    await aiExecuteFileOps(pendingOps.value)
    pendingOps.value = null
    messages.value.push({ role: 'assistant', content: t('ai.execDone') })
  } catch (err) {
    error.value = t('ai.errorPrefix', { msg: err instanceof Error ? err.message : String(err) })
  } finally {
    executing.value = false
    await scrollToBottom()
  }
}

function rejectOps() {
  if (executing.value) return
  pendingOps.value = null
  messages.value.push({ role: 'assistant', content: t('ai.execRejected') })
}
</script>

<template>
  <div class="ai-content">
    <div class="ai-header">
      <div class="ai-header-text">
        <span class="ai-title">{{ t('ai.title') }}</span>
        <span class="ai-hint">{{ t('ai.hint') }}</span>
      </div>
      <button class="ai-crash-btn" :disabled="sending" @click="analyzeCrash">
        {{ sending ? t('ai.sending') : t('ai.analyzeCrash') }}
      </button>
    </div>

    <div class="ai-chat" ref="chatListRef">
      <div v-if="messages.length === 0" class="ai-empty">
        <div class="ai-empty-text">{{ t('ai.emptyHint') }}</div>
      </div>

      <div v-for="(m, i) in messages" :key="i" class="ai-msg" :class="m.role">
        <template v-if="m.role === 'assistant'">
          <div v-if="m.statuses && m.statuses.length" class="ai-statuses">
            <div v-for="(st, si) in m.statuses" :key="si" class="ai-status-line">→ {{ st }}</div>
          </div>
          <div class="ai-bubble" v-html="renderMd(m.content)"></div>
        </template>
        <div v-else class="ai-bubble">{{ m.content }}</div>
        <div v-if="m.role === 'assistant' && m.usage && m.usage.totalTokens" class="ai-usage">
          ↑{{ m.usage.promptTokens }} ↓{{ m.usage.completionTokens }} tokens
        </div>
      </div>

      <div v-if="sending" class="ai-msg assistant">
        <div class="ai-bubble ai-thinking">{{ t('ai.sending') }}</div>
      </div>
    </div>

    <!-- 文件操作审批 -->
    <div v-if="pendingOps && pendingOps.length" class="ai-approval">
      <div class="ai-approval-title">{{ t('ai.toolTitle') }}</div>
      <div v-for="(op, i) in pendingOps" :key="i" class="ai-op">
        <span class="ai-op-badge" :class="op.action">
          {{ op.action === 'delete_file' ? t('ai.opDelete') : t('ai.opWrite') }}
        </span>
        <span class="ai-op-path">{{ op.path }}</span>
      </div>
      <div v-if="pendingOps.some((o) => o.action === 'write_file' && o.content)" class="ai-op-preview">
        <pre>{{ pendingOps.find((o) => o.action === 'write_file' && o.content)?.content }}</pre>
      </div>
      <div class="ai-approval-actions">
        <button class="ai-btn danger" :disabled="executing" @click="rejectOps">{{ t('ai.reject') }}</button>
        <button class="ai-btn primary" :disabled="executing" @click="approveOps">
          {{ executing ? t('ai.executing') : t('ai.approve') }}
        </button>
      </div>
    </div>

    <div v-if="error" class="ai-error">{{ error }}</div>

    <div class="ai-input-row">
      <input
        type="text"
        v-model="input"
        class="ai-input"
        :placeholder="t('ai.inputPlaceholder')"
        :disabled="sending"
        @keyup.enter="send()"
      />
      <button class="ai-send-btn" :disabled="sending || !input.trim()" @click="send()">
        {{ sending ? t('ai.sending') : t('ai.send') }}
      </button>
    </div>
  </div>
</template>

<style scoped>
.ai-content {
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
  padding: 10px 17px;
  box-sizing: border-box;
  gap: 10px;
  overflow: hidden;
}
.ai-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  flex-shrink: 0;
}
.ai-header-text {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}
.ai-title {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 18px;
  font-weight: 600;
  color: var(--text-color);
}
.ai-hint {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 11px;
  color: var(--label-color);
}
.ai-crash-btn {
  flex-shrink: 0;
  padding: 8px 16px;
  border: none;
  border-radius: 9px;
  background: var(--button-bg);
  color: var(--text-color);
  font-size: 13px;
  cursor: pointer;
  transition: background-color 0.15s;
}
.ai-crash-btn:hover:not(:disabled) {
  background: var(--button-active);
}
.ai-crash-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.ai-chat {
  flex: 1;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 4px 2px;
  min-height: 0;
}
.ai-empty {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  opacity: 0.55;
}
.ai-empty-text {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 13px;
  color: var(--text-color);
  text-align: center;
  max-width: 300px;
  line-height: 1.6;
}
.ai-msg {
  display: flex;
  flex-direction: column;
}
.ai-msg.user {
  align-items: flex-end;
}
.ai-msg.assistant {
  align-items: flex-start;
}
.ai-bubble {
  max-width: 78%;
  padding: 9px 13px;
  border-radius: 12px;
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 13px;
  line-height: 1.6;
  white-space: normal;
  word-break: break-word;
  color: var(--text-color);
}
.ai-msg.user .ai-bubble {
  background: var(--accent-color);
  color: var(--accent-text-color);
  border-bottom-right-radius: 3px;
}
.ai-msg.assistant .ai-bubble {
  background: var(--card-bg);
  border-bottom-left-radius: 3px;
}
.ai-usage {
  margin-top: 2px;
  font-size: 10px;
  color: var(--label-color);
  opacity: 0.6;
  padding: 0 4px;
}
.ai-statuses {
  display: flex;
  flex-direction: column;
  gap: 2px;
  margin-bottom: 4px;
}
.ai-status-line {
  font-size: 11px;
  color: var(--label-color);
  opacity: 0.7;
  font-family: 'SF Mono', 'Menlo', monospace;
}
.ai-thinking {
  opacity: 0.6;
}

/* ===== 气泡内 Markdown 排版 ===== */
.ai-bubble p {
  margin: 0 0 6px;
}
.ai-bubble p:last-child {
  margin-bottom: 0;
}
.ai-bubble h1,
.ai-bubble h2,
.ai-bubble h3,
.ai-bubble h4 {
  margin: 8px 0 4px;
  font-size: 14px;
  font-weight: 600;
}
.ai-bubble ul,
.ai-bubble ol {
  margin: 4px 0;
  padding-left: 18px;
}
.ai-bubble li {
  margin: 2px 0;
}
.ai-bubble code {
  font-family: 'SF Mono', 'Menlo', monospace;
  font-size: 12px;
  background: var(--button-bg);
  border-radius: 4px;
  padding: 1px 5px;
}
.ai-bubble pre {
  margin: 6px 0;
  padding: 8px 10px;
  background: var(--button-bg);
  border-radius: 8px;
  overflow-x: auto;
}
.ai-bubble pre code {
  background: transparent;
  padding: 0;
}
.ai-bubble a {
  color: var(--accent-color);
}
.ai-bubble blockquote {
  margin: 6px 0;
  padding: 2px 10px;
  border-left: 3px solid var(--accent-color);
  opacity: 0.8;
}
.ai-bubble table {
  border-collapse: collapse;
  margin: 6px 0;
  font-size: 12px;
}
.ai-bubble th,
.ai-bubble td {
  border: 1px solid var(--border-color);
  padding: 4px 8px;
}
.ai-bubble hr {
  border: none;
  border-top: 1px solid var(--border-color);
  margin: 8px 0;
}
.ai-approval {
  flex-shrink: 0;
  border: 1px solid rgba(255, 149, 0, 0.45);
  background: rgba(255, 149, 0, 0.08);
  border-radius: 10px;
  padding: 10px 12px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.ai-approval-title {
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 12px;
  color: #ff9500;
}
.ai-op {
  display: flex;
  align-items: center;
  gap: 8px;
}
.ai-op-badge {
  flex-shrink: 0;
  font-size: 11px;
  padding: 2px 8px;
  border-radius: 5px;
  color: #fff;
}
.ai-op-badge.delete_file {
  background: #ff3b30;
}
.ai-op-badge.write_file {
  background: #0a84ff;
}
.ai-op-path {
  font-family: 'SF Mono', 'Menlo', monospace;
  font-size: 12px;
  color: var(--text-color);
  word-break: break-all;
}
.ai-op-preview pre {
  margin: 0;
  max-height: 120px;
  overflow: auto;
  font-size: 11px;
  color: var(--text-color);
  opacity: 0.8;
  background: var(--button-bg);
  border-radius: 6px;
  padding: 6px 8px;
  white-space: pre-wrap;
  word-break: break-all;
}
.ai-approval-actions {
  display: flex;
  gap: 8px;
  justify-content: flex-end;
}
.ai-btn {
  padding: 7px 16px;
  border: none;
  border-radius: 8px;
  font-size: 13px;
  cursor: pointer;
  transition: opacity 0.15s;
}
.ai-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.ai-btn.primary {
  background: var(--accent-color);
  color: var(--accent-text-color);
}
.ai-btn.danger {
  background: rgba(255, 59, 48, 0.15);
  color: #ff3b30;
}
.ai-error {
  flex-shrink: 0;
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 12px;
  color: #ff3b30;
  text-align: center;
}
.ai-input-row {
  display: flex;
  gap: 8px;
  flex-shrink: 0;
}
.ai-input {
  flex: 1;
  padding: 10px 14px;
  border: 1px solid var(--border-color);
  border-radius: 10px;
  background: var(--input-bg);
  font-family: Inter, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', SimHei, Arial, Helvetica, sans-serif;
  font-size: 14px;
  color: var(--text-color);
  outline: none;
}
.ai-input:focus {
  border-color: var(--accent-color);
}
.ai-input:disabled {
  opacity: 0.6;
}
.ai-send-btn {
  padding: 10px 20px;
  border: none;
  border-radius: 10px;
  background: var(--accent-color);
  color: var(--accent-text-color);
  font-size: 14px;
  font-weight: 500;
  cursor: pointer;
  flex-shrink: 0;
}
.ai-send-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.ai-chat::-webkit-scrollbar {
  width: 4px;
}
.ai-chat::-webkit-scrollbar-track {
  background: transparent;
}
.ai-chat::-webkit-scrollbar-thumb {
  background: var(--button-bg);
  border-radius: 2px;
}
</style>
