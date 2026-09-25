<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { pluginPageInjects, pluginUiEvent } from '../utils/tauri'
import type { PageInjectEntry, PluginControlNode, PluginNotify } from '../utils/tauri'

const props = defineProps<{
  page: string
}>()

const emit = defineEmits<{
  (e: 'overrides', overrides: Record<string, string>): void
}>()

const injects = ref<PageInjectEntry[]>([])
const inputValues = ref<Record<string, string>>({})
const toasts = ref<PluginNotify[]>([])

async function loadInjects() {
  try {
    injects.value = await pluginPageInjects(props.page)
    const merged: Record<string, string> = {}
    for (const entry of injects.value) {
      if (entry.overrides) {
        Object.assign(merged, entry.overrides)
      }
    }
    if (Object.keys(merged).length > 0) {
      emit('overrides', merged)
    }
  } catch (err) {
    console.error('加载页面注入失败:', err)
    injects.value = []
  }
}

async function fireEvent(pluginId: string, control: string, event: string) {
  try {
    const result = await pluginUiEvent(pluginId, control, event)
    const toShow = result.notify || []
    toShow.forEach((n) => {
      toasts.value.push(n)
      setTimeout(() => {
        toasts.value = toasts.value.filter((x) => x !== n)
      }, 3000)
    })
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err)
    toasts.value.push({ kind: 'toast', text: msg })
    setTimeout(() => {
      toasts.value = toasts.value.filter((x) => x.text !== msg)
    }, 3000)
  }
}

function layoutClass(layout: string): string {
  if (layout === 'center') return 'ctl-center'
  if (layout === 'right') return 'ctl-right'
  return 'ctl-left'
}

function controlKey(entry: PageInjectEntry, c: PluginControlNode): string {
  return `${entry.pluginId}-${c.name}`
}

onMounted(loadInjects)
</script>

<template>
  <div v-if="injects.length > 0" class="plugin-inject">
    <template v-for="entry in injects" :key="entry.pluginId">
      <div class="inject-group">
        <div class="inject-label">{{ entry.pluginName }}</div>
        <div class="inject-controls">
          <template v-for="c in entry.controls" :key="controlKey(entry, c)">
            <button
              v-if="c.type === 'button'"
              :class="['cmp-btn', layoutClass(c.layout)]"
              @click="fireEvent(entry.pluginId, c.name, 'click')"
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
              v-model="inputValues[controlKey(entry, c)]"
              @change="fireEvent(entry.pluginId, c.name, 'input')"
            />
            <label v-else-if="c.type === 'toggle'" :class="['cmp-toggle', layoutClass(c.layout)]">
              <input
                type="checkbox"
                :checked="c.checked"
                @change="fireEvent(entry.pluginId, c.name, c.checked ? 'uncheck' : 'check')"
              />
              <span>{{ c.text || c.name }}</span>
            </label>
            <hr
              v-else-if="c.type === 'line' && !c.portrait"
              class="cmp-line"
              :style="c.lineLength ? { width: c.lineLength + 'px' } : undefined"
            />
          </template>
        </div>
      </div>
    </template>

    <div v-if="toasts.length > 0" class="inject-toasts">
      <div v-for="(tt, i) in toasts" :key="i" class="inject-toast">{{ tt.text }}</div>
    </div>
  </div>
</template>

<style scoped>
.plugin-inject {
  display: flex;
  flex-direction: column;
  gap: 10px;
  margin-top: 16px;
  width: 100%;
  max-width: 480px;
}

.inject-group {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.inject-label {
  font-size: 11px;
  opacity: 0.45;
  padding-left: 2px;
}

.inject-controls {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.cmp-btn {
  padding: 6px 14px;
  border-radius: 7px;
  border: none;
  background: var(--accent, #4f8cff);
  color: #fff;
  font-size: 13px;
  cursor: pointer;
}

.cmp-text {
  font-size: 13px;
  opacity: 0.75;
}

.cmp-input {
  padding: 6px 10px;
  border-radius: 6px;
  border: 1px solid var(--border, #333);
  background: var(--bg-input, #1e1e1e);
  color: inherit;
  font-size: 13px;
  min-width: 160px;
}

.cmp-toggle {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  cursor: pointer;
}

.cmp-line {
  border: none;
  border-top: 1px solid var(--border, #333);
  align-self: center;
  margin: 2px 0;
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

.inject-toasts {
  position: fixed;
  bottom: 24px;
  left: 50%;
  transform: translateX(-50%);
  display: flex;
  flex-direction: column;
  gap: 8px;
  z-index: 100;
}

.inject-toast {
  background: var(--bg-toast, #2a2a2a);
  color: #fff;
  padding: 9px 16px;
  border-radius: 8px;
  font-size: 13px;
  box-shadow: 0 4px 14px rgba(0, 0, 0, 0.35);
}
</style>