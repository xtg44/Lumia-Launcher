<script setup lang="ts">
import { ref, onMounted, onBeforeUnmount } from 'vue'
import { pluginUiTree, pluginUiEvent } from '../utils/tauri'
import type { PluginControlNode, PluginNotify } from '../utils/tauri'
import { useI18n } from 'vue-i18n'
import AppSelect from './AppSelect.vue'

const { t } = useI18n()

const props = defineProps<{
  pluginId: string
  pluginName: string
}>()

const controls = ref<PluginControlNode[]>([])
const loading = ref(true)
const error = ref('')

// 表单控件输入暂存（CreateInput）
const inputValues = ref<Record<string, string>>({})

// 后端动作产生的通知（popup/toast）→ 前端浮层
const popup = ref<string | null>(null)
const toasts = ref<PluginNotify[]>([])

function showNotify(n: PluginNotify) {
  if (n.kind === 'popup') {
    popup.value = n.text
  } else {
    toasts.value.push(n)
    setTimeout(() => {
      toasts.value = toasts.value.filter((x) => x !== n)
    }, 3000)
  }
}

/** 触发插件控件事件，刷新控件树并显示后端动作通知 */
function fireEvent(control: string, event: string) {
  pluginUiEvent(props.pluginId, control, event)
    .then((result) => {
      controls.value = result.tree
      result.notify.forEach((n) => showNotify(n))
    })
    .catch((err) => {
      const msg = err instanceof Error ? err.message : String(err)
      toasts.value.push({ kind: 'toast', text: msg })
      setTimeout(() => {
        toasts.value = toasts.value.filter((x) => x.text !== msg)
      }, 3000)
    })
}

async function loadTree() {
  loading.value = true
  try {
    controls.value = await pluginUiTree(props.pluginId)
    error.value = ''
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err)
  } finally {
    loading.value = false
  }
}

onMounted(async () => {
  await loadTree()
})

onBeforeUnmount(() => {
  // 无活跃监听需要清理
})

function layoutClass(layout: string): string {
  if (layout === 'center') return 'ctl-center'
  if (layout === 'right') return 'ctl-right'
  return 'ctl-left'
}

function listOptions(c: PluginControlNode) {
  return (c.list || []).map(v => ({ value: v, label: v }))
}

function closePopup() {
  popup.value = null
}
</script>

<template>
  <div class="plugin-page">
    <div class="plugin-head">
      <h2 class="plugin-title">{{ props.pluginName }}</h2>
      <span class="plugin-id">{{ props.pluginId }}</span>
    </div>

    <div v-if="loading" class="plugin-loading">{{ t('plugin.loading') }}</div>
    <div v-else-if="error" class="plugin-error">{{ error }}</div>
    <div v-else-if="controls.length === 0" class="plugin-empty">
      {{ t('plugin.empty') }}
    </div>

    <div v-else class="plugin-controls">
      <template v-for="c in controls" :key="c.name">
        <button
          v-if="c.type === 'button'"
          :class="['cmp-btn', layoutClass(c.layout)]"
          @click="fireEvent(c.name, 'click')"
        >
          {{ c.text || c.name }}
        </button>
        <div v-else-if="c.type === 'text'" :class="['cmp-text', layoutClass(c.layout)]">
          {{ c.text || c.name }}
        </div>
        <input
          v-else-if="c.type === 'input'"
          :class="['cmp-input', layoutClass(c.layout)]"
          :placeholder="c.text"
          v-model="inputValues[c.name]"
          @change="fireEvent(c.name, 'input')"
        />
        <AppSelect
          v-else-if="c.type === 'list'"
          :model-value="c.selected ?? ''"
          :options="listOptions(c)"
          :class="layoutClass(c.layout)"
          @change="fireEvent(c.name, 'change')"
        />
        <label v-else-if="c.type === 'toggle'" :class="['cmp-toggle', layoutClass(c.layout)]">
          <input
            type="checkbox"
            :checked="c.checked"
            @change="fireEvent(c.name, c.checked ? 'uncheck' : 'check')"
          />
          <span>{{ c.text || c.name }}</span>
        </label>
        <div v-else-if="c.type === 'image'" :class="['cmp-image', layoutClass(c.layout)]">
          <span class="img-cap">{{ c.text || c.name }}</span>
        </div>
        <hr
          v-else-if="c.type === 'line' && !c.portrait"
          class="cmp-line"
          :style="c.lineLength ? { width: c.lineLength + 'px' } : undefined"
        />
        <div
          v-else-if="c.type === 'line' && c.portrait"
          class="cmp-line portrait"
          :style="c.lineLength ? { height: c.lineLength + 'px' } : undefined"
        />
      </template>
    </div>

    <!-- toast 通知 -->
    <div v-if="toasts.length > 0" class="plugin-toasts">
      <div v-for="(tt, i) in toasts" :key="i" class="plugin-toast">{{ tt.text }}</div>
    </div>

    <!-- popup 弹窗 -->
    <div v-if="popup" class="plugin-popup-mask" @click.self="closePopup">
      <div class="plugin-popup">
        <div class="plugin-popup-text">{{ popup }}</div>
        <button class="cmp-btn" @click="closePopup">{{ t('plugin.ok') }}</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.plugin-page {
  display: flex;
  flex-direction: column;
  gap: 14px;
  padding: 18px 20px;
  height: 100%;
  overflow-y: auto;
}
.plugin-head {
  display: flex;
  align-items: baseline;
  gap: 10px;
}
.plugin-title {
  margin: 0;
  font-size: 20px;
  font-weight: 600;
}
.plugin-id {
  font-size: 12px;
  opacity: 0.55;
}
.plugin-loading,
.plugin-error,
.plugin-empty {
  padding: 24px 8px;
  font-size: 13px;
  opacity: 0.7;
}
.plugin-error {
  color: #e5484d;
  opacity: 1;
}
.plugin-controls {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.ctl-left {
  align-self: flex-start;
}
.ctl-center {
  align-self: center;
}
.ctl-right {
  align-self: flex-end;
}
.cmp-btn {
  padding: 8px 18px;
  border-radius: 8px;
  border: none;
  background: var(--accent, #4f8cff);
  color: #fff;
  font-size: 14px;
  cursor: pointer;
}
.cmp-btn:hover {
  filter: brightness(1.08);
}
.cmp-text {
  font-size: 14px;
}
.cmp-input,
.cmp-list {
  padding: 7px 10px;
  border-radius: 7px;
  border: 1px solid var(--border, #333);
  background: var(--bg-input, #1e1e1e);
  color: inherit;
  font-size: 13px;
  min-width: 180px;
}
.cmp-toggle {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  font-size: 14px;
  cursor: pointer;
}
.cmp-image {
  display: flex;
  flex-direction: column;
  gap: 6px;
  align-items: flex-start;
}
.img-cap {
  font-size: 12px;
  opacity: 0.6;
}
.cmp-line {
  border: none;
  border-top: 1px solid var(--border, #333);
  align-self: center;
  margin: 2px 0;
}
.cmp-line.portrait {
  border-top: none;
  border-left: 1px solid var(--border, #333);
}
.plugin-toasts {
  position: fixed;
  bottom: 24px;
  left: 50%;
  transform: translateX(-50%);
  display: flex;
  flex-direction: column;
  gap: 8px;
  z-index: 100;
}
.plugin-toast {
  background: var(--bg-toast, #2a2a2a);
  color: #fff;
  padding: 9px 16px;
  border-radius: 8px;
  font-size: 13px;
  box-shadow: 0 4px 14px rgba(0, 0, 0, 0.35);
}
.plugin-popup-mask {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.45);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 200;
}
.plugin-popup {
  background: var(--bg-popup, #262626);
  border-radius: 12px;
  padding: 22px 26px;
  max-width: 360px;
  display: flex;
  flex-direction: column;
  gap: 16px;
  box-shadow: 0 8px 30px rgba(0, 0, 0, 0.4);
  align-items: center;
}
.plugin-popup-text {
  font-size: 14px;
  line-height: 1.5;
}
</style>